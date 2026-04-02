use cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1;
use cosmic_protocols::toplevel_management::v1::client::zcosmic_toplevel_manager_v1;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use wayland_client::{
    protocol::{wl_output, wl_seat},
    Connection, EventQueue,
};
use wayland_protocols::ext::workspace::v1::client::{
    ext_workspace_group_handle_v1, ext_workspace_handle_v1,
};

mod dispatch;

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
}

const HELP: &str = "\
Usage: cos-cli [COMMAND]

A CLI utility for COSMIC Wayland toplevel and workspace management.

Commands:
  info                          List active apps, workspaces, and outputs
  move                          Move an application to a specific workspace
  activate                      Activate an application on a specific seat
  state                         Set state of an application

Options for 'move':
  -a, --app-id <ID>             The Application ID (partial match, case-insensitive)
  -i, --index <INDEX>           The Application index from 'info' command
  -w, --workspace <NAME>        The name of the target workspace
  -g, --workspace-group <INDEX> The workspace group index from 'info' command (optional)
  -o, --output-index <INDEX>    The output index from 'info' command (optional)
  --wait <SECONDS>              Wait for the app to appear (optional, only for --app-id)

Options for 'activate':
  -i, --index <INDEX>           The Application index from 'info' command
  -s, --seat <INDEX>            The Seat index from 'info' command (optional)

Options for 'state':
  -a, --app-id <ID>             The Application ID (partial match, case-insensitive)
  -i, --index <INDEX>           The Application index from 'info' command
  --wait <SECONDS>              Wait for the app to appear (optional, only for --app-id)
  --maximize
  --unmaximize
  --minimize
  --unminimize
  --fullscreen
  --unfullscreen
  --sticky
  --unsticky

Options for 'info':
  --json                        Output in JSON format

Examples:
  cos-cli info
  cos-cli info --json
  cos-cli move --app-id Firefox --workspace 2
  cos-cli move -i 0 -w 2
  cos-cli move -a terminal -w 2 --wait 5
  cos-cli move -a terminal -w 2 -o 1
  cos-cli move -a terminal -w 2 -g 1
  cos-cli activate -i 0 -s 0
  cos-cli activate -i 0
  cos-cli state -i 0 --maximize
  cos-cli state --app-id firefox --sticky --fullscreen
";

struct CliError(String);

impl CliError {
    fn new(message: String) -> Box<Self> {
        Self(message).into()
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Debug for CliError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CliError({})", self.0)
    }
}

impl Error for CliError {
    fn description(&self) -> &str {
        &self.0
    }
}

impl From<String> for CliError {
    fn from(s: String) -> Self {
        CliError(s)
    }
}

impl From<&str> for CliError {
    fn from(s: &str) -> Self {
        CliError(s.to_string())
    }
}

struct WorkspaceGroup {
    handle: ext_workspace_group_handle_v1::ExtWorkspaceGroupHandleV1,
    workspaces: Vec<(String, ext_workspace_handle_v1::ExtWorkspaceHandleV1)>,
}

struct AppState {
    available_interfaces: HashMap<String, (u32, u32)>,
    workspace_groups: Vec<WorkspaceGroup>,
    cosmic_toplevel_manager: Option<zcosmic_toplevel_manager_v1::ZcosmicToplevelManagerV1>,
    outputs: Vec<(wl_output::WlOutput, String)>,
    seats: Vec<(wl_seat::WlSeat, String)>,
    apps: Vec<App>,
}

#[derive(Debug, Clone)]
struct App {
    handle: zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1,
    title: Option<String>,
    app_id: Option<String>,
    outputs: Vec<wl_output::WlOutput>,
    workspaces: Vec<ext_workspace_handle_v1::ExtWorkspaceHandleV1>,
    state: Vec<State>,
}

#[derive(Serialize)]
struct JsonApp {
    index: usize,
    app_id: String,
    title: String,
    state: Vec<State>,
    outputs: Vec<String>,
    workspaces: Vec<usize>,
}

#[derive(Serialize)]
struct JsonWorkspace {
    index: usize,
    workspaces: Vec<String>,
}

#[derive(Serialize)]
struct JsonOutput {
    index: usize,
    name: String,
}

#[derive(Serialize)]
struct JsonSeat {
    index: usize,
    name: String,
}

#[derive(Serialize)]
struct JsonInfo {
    apps: Vec<JsonApp>,
    workspaces: Vec<JsonWorkspace>,
    outputs: Vec<JsonOutput>,
    seats: Vec<JsonSeat>,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Maximized = 0,
    Minimized = 1,
    Activated = 2,
    Fullscreen = 3,
}

impl TryFrom<u32> for State {
    type Error = ();

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(State::Maximized),
            1 => Ok(State::Minimized),
            2 => Ok(State::Activated),
            3 => Ok(State::Fullscreen),
            _ => Err(()),
        }
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            State::Maximized => "maximized",
            State::Minimized => "minimized",
            State::Fullscreen => "fullscreen",
            State::Activated => "activated",
        })
    }
}

#[derive(Debug)]
struct MoveArgs {
    app_id: Option<String>,
    app_index: Option<usize>,
    workspace_name: String,
    workspace_group_index: Option<usize>,
    output_index: Option<usize>,
    wait: Option<u64>,
}

#[derive(Debug)]
struct ActivateArgs {
    app_index: usize,
    seat_index: Option<usize>,
}

#[derive(Debug)]
struct StateArgs {
    app_id: Option<String>,
    app_index: Option<usize>,
    wait: Option<u64>,
    maximize: bool,
    unmaximize: bool,
    minimize: bool,
    unminimize: bool,
    fullscreen: bool,
    unfullscreen: bool,
    sticky: bool,
    unsticky: bool,
}

trait AppFinderArgs {
    fn app_index(&self) -> Option<usize>;
    fn app_id(&self) -> Option<&String>;
    fn wait(&self) -> Option<u64>;
}

impl AppFinderArgs for MoveArgs {
    fn app_index(&self) -> Option<usize> {
        self.app_index
    }

    fn app_id(&self) -> Option<&String> {
        self.app_id.as_ref()
    }

    fn wait(&self) -> Option<u64> {
        self.wait
    }
}

impl AppFinderArgs for StateArgs {
    fn app_index(&self) -> Option<usize> {
        self.app_index
    }

    fn app_id(&self) -> Option<&String> {
        self.app_id.as_ref()
    }

    fn wait(&self) -> Option<u64> {
        self.wait
    }
}

#[derive(Debug)]
struct InfoArgs {
    json: bool,
}

enum Command {
    Info(InfoArgs),
    Move(MoveArgs),
    Activate(ActivateArgs),
    State(StateArgs),
}

fn find_apps<T: AppFinderArgs>(
    state: &mut AppState,
    event_queue: &mut EventQueue<AppState>,
    args: &T,
) -> Result<Vec<App>, Box<dyn Error>> {
    if let Some(app_index) = args.app_index() {
        if let Some(app) = state.apps.get(app_index) {
            Ok(vec![app.clone()])
        } else {
            Err(CliError::new(format!("App index not found: {}", app_index)))
        }
    } else if let Some(app_id) = args.app_id() {
        let sleep = std::time::Duration::from_millis(500);
        let wait_dur = args.wait().map(std::time::Duration::from_secs);
        let now = std::time::Instant::now();
        let mut apps;
        loop {
            apps = state
                .apps
                .iter()
                .filter(|app| {
                    app.app_id
                        .as_ref()
                        .map(|v| v.to_lowercase().contains(&app_id.to_lowercase()))
                        .unwrap_or_default()
                })
                .cloned()
                .collect::<Vec<_>>();

            if !apps.is_empty() {
                break;
            }

            if let Some(wait) = wait_dur {
                if now.elapsed() > wait {
                    break;
                }
                std::thread::sleep(sleep);
                event_queue.roundtrip(state)?;
            } else {
                break;
            }
        }
        if apps.is_empty() {
            return Err(CliError::new(format!("App id not found: {}", app_id)));
        }
        Ok(apps)
    } else {
        unreachable!(); // Already handled by arg parsing
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    let mut pargs = pico_args::Arguments::from_env();
    if pargs.contains(["-v", "--version"]) {
        println!("Version: {}", VERSION);
        return Ok(());
    }

    let subcommand = pargs.subcommand()?;

    let command = match subcommand.as_deref() {
        Some("info") => Command::Info(InfoArgs {
            json: pargs.contains("--json"),
        }),
        Some("move") => {
            let app_id: Option<String> = pargs.opt_value_from_str(["-a", "--app-id"])?;
            let app_index: Option<usize> = pargs.opt_value_from_str(["-i", "--index"])?;

            if app_id.is_none() && app_index.is_none() {
                return Err(CliError::new(
                    "Either --app-id or --index must be provided for 'move' command.".into(),
                ));
            }
            if app_id.is_some() && app_index.is_some() {
                return Err(CliError::new(
                    "Only one of --app-id or --index can be provided for 'move' command.".into(),
                ));
            }

            Command::Move(MoveArgs {
                app_id,
                app_index,
                workspace_name: pargs.value_from_str(["-w", "--workspace"])?,
                workspace_group_index: pargs.opt_value_from_str(["-g", "--workspace-group"])?,
                output_index: pargs.opt_value_from_str(["-o", "--output-index"])?,
                wait: pargs.opt_value_from_fn("--wait", |v| v.parse())?,
            })
        }
        Some("activate") => Command::Activate(ActivateArgs {
            app_index: pargs.value_from_str(["-i", "--index"])?,
            seat_index: pargs.opt_value_from_str(["-s", "--seat"])?,
        }),
        Some("state") => {
            let app_id: Option<String> = pargs.opt_value_from_str(["-a", "--app-id"])?;
            let app_index: Option<usize> = pargs.opt_value_from_str(["-i", "--index"])?;

            if app_id.is_none() && app_index.is_none() {
                return Err(CliError::new(
                    "Either --app-id or --index must be provided for 'state' command.".into(),
                ));
            }
            if app_id.is_some() && app_index.is_some() {
                return Err(CliError::new(
                    "Only one of --app-id or --index can be provided for 'state' command.".into(),
                ));
            }

            let args = StateArgs {
                app_id,
                app_index,
                wait: pargs.opt_value_from_fn("--wait", |v| v.parse())?,
                maximize: pargs.contains("--maximize"),
                unmaximize: pargs.contains("--unmaximize"),
                minimize: pargs.contains("--minimize"),
                unminimize: pargs.contains("--unminimize"),
                fullscreen: pargs.contains("--fullscreen"),
                unfullscreen: pargs.contains("--unfullscreen"),
                sticky: pargs.contains("--sticky"),
                unsticky: pargs.contains("--unsticky"),
            };
            let num_actions = [
                args.maximize,
                args.unmaximize,
                args.minimize,
                args.unminimize,
                args.fullscreen,
                args.unfullscreen,
                args.sticky,
                args.unsticky,
            ]
            .iter()
            .filter(|&&x| x)
            .count();

            if num_actions == 0 {
                return Err(CliError::new(
                    "No action specified for 'state' command.".into(),
                ));
            }

            Command::State(args)
        }
        Some("help") | None => {
            println!("{HELP}");
            return Ok(());
        }
        Some(_) => {
            return Err(CliError::new(format!(
                "Unknown subcommand: {}",
                subcommand.unwrap_or_default()
            )));
        }
    };

    let conn = Connection::connect_to_env()?;
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();

    let mut state = AppState {
        available_interfaces: Default::default(),
        cosmic_toplevel_manager: None,
        workspace_groups: Default::default(),
        apps: Default::default(),
        outputs: Default::default(),
        seats: Default::default(),
    };
    let registry = conn.display().get_registry(&qh, ());

    event_queue.roundtrip(&mut state)?;
    dispatch::bind(&registry, &qh, &mut state);
    event_queue.roundtrip(&mut state)?;

    match command {
        Command::Info(args) => {
            if args.json {
                let json_info = JsonInfo {
                    apps: state
                        .apps
                        .iter()
                        .enumerate()
                        .map(|(i, app)| {
                            let outputs = app
                                .outputs
                                .iter()
                                .filter_map(|o| {
                                    state
                                        .outputs
                                        .iter()
                                        .find(|(wo, _)| wo == o)
                                        .map(|(_, n)| n.clone())
                                })
                                .collect();
                            let workspaces =
                                app.workspaces.iter().enumerate().map(|(j, _)| j).collect();
                            JsonApp {
                                index: i,
                                app_id: app.app_id.clone().unwrap_or_default(),
                                title: app.title.clone().unwrap_or_default(),
                                state: app.state.clone(),
                                outputs,
                                workspaces,
                            }
                        })
                        .collect(),
                    workspaces: state
                        .workspace_groups
                        .iter()
                        .enumerate()
                        .map(|(i, group)| JsonWorkspace {
                            index: i,
                            workspaces: group.workspaces.iter().map(|(n, _)| n.clone()).collect(),
                        })
                        .collect(),
                    outputs: state
                        .outputs
                        .iter()
                        .enumerate()
                        .map(|(i, (_, name))| JsonOutput {
                            index: i,
                            name: name.clone(),
                        })
                        .collect(),
                    seats: state
                        .seats
                        .iter()
                        .enumerate()
                        .map(|(i, (_, name))| JsonSeat {
                            index: i,
                            name: name.clone(),
                        })
                        .collect(),
                };
                println!("{}", serde_json::to_string(&json_info).unwrap());
            } else {
                println!("Apps:");
                for (i, app) in state.apps.iter().enumerate() {
                    let states = app
                        .state
                        .iter()
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    let output_names = app
                        .outputs
                        .iter()
                        .filter_map(|o| {
                            state
                                .outputs
                                .iter()
                                .find(|(wo, _)| wo == o)
                                .map(|(_, n)| n.as_str())
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    let workspaces = app
                        .workspaces
                        .iter()
                        .filter_map(|w| {
                            state
                                .workspace_groups
                                .iter()
                                .find_map(|g| g.workspaces.iter().find(|i| &i.1 == w))
                                .map(|(n, _)| n.as_str())
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    println!(
                        "\t[{}] {} (title: {}, state: [{}],  workspaces: [{}], outputs: [{}])",
                        i,
                        app.app_id.as_deref().unwrap_or_default(),
                        app.title.as_deref().unwrap_or_default(),
                        states,
                        workspaces,
                        output_names,
                    );
                }
                println!("Workspaces:");
                for (i, group) in state.workspace_groups.iter().enumerate() {
                    println!("\t[{i}] Group");
                    for (workspace, _) in group.workspaces.iter() {
                        println!("\t\tWorkspace: {workspace}");
                    }
                }
                println!("Outputs:");
                for (i, (_, name)) in state.outputs.iter().enumerate() {
                    println!("\t[{i}] Output: {name}");
                }

                println!("Seats:");
                for (i, (_, name)) in state.seats.iter().enumerate() {
                    println!("\t[{i}] Seat: {name}");
                }
            }
        }
        Command::Move(args) => {
            let apps_to_move = find_apps(&mut state, &mut event_queue, &args)?;

            let Some(manager) = &state.cosmic_toplevel_manager else {
                return Err(CliError::new(
                    "Compositor does not support workspace management protocol.".into(),
                ));
            };
            println!("Connected to cosmic toplevel manager!");

            let Some((_, workspace)) = (if let Some(group_index) = args.workspace_group_index {
                if let Some(group) = state.workspace_groups.get(group_index) {
                    group
                        .workspaces
                        .iter()
                        .find(|(w, _)| w == &args.workspace_name)
                } else {
                    return Err(CliError::new(format!(
                        "Workspace group not found: {}",
                        group_index
                    )));
                }
            } else {
                state
                    .workspace_groups
                    .iter()
                    .flat_map(|g| g.workspaces.iter())
                    .find(|(w, _)| w == &args.workspace_name)
            }) else {
                return Err(CliError::new(format!(
                    "Workspace not found: {}",
                    args.workspace_name
                )));
            };

            let output = if let Some(index) = args.output_index {
                if let Some(output) = state.outputs.get(index) {
                    output.0.clone()
                } else {
                    return Err(CliError::new(format!("Output index not found: {}", index)));
                }
            } else {
                if state.outputs.is_empty() {
                    return Err(CliError::new("No outputs found.".to_string()));
                }
                state.outputs[0].0.clone()
            };

            for app in apps_to_move {
                println!(
                    "Move {} to {}",
                    app.app_id.as_deref().unwrap_or_default(),
                    args.workspace_name,
                );
                manager.move_to_ext_workspace(&app.handle, workspace, &output);
            }

            conn.flush()?;
        }
        Command::Activate(args) => {
            let Some(manager) = &state.cosmic_toplevel_manager else {
                return Err(CliError::new(
                    "Compositor does not support toplevel management protocol.".into(),
                ));
            };
            let Some(app) = state.apps.get(args.app_index) else {
                return Err(CliError::new(format!(
                    "App index not found: {}",
                    args.app_index
                )));
            };
            let seat = if let Some(seat_index) = args.seat_index {
                state
                    .seats
                    .get(seat_index)
                    .ok_or_else(|| CliError::new(format!("Seat index not found: {}", seat_index)))?
            } else {
                state
                    .seats
                    .first()
                    .ok_or_else(|| CliError::new("No seats found.".to_string()))?
            };
            manager.activate(&app.handle, &seat.0);
            conn.flush()?;
        }
        Command::State(args) => {
            let apps_to_modify = find_apps(&mut state, &mut event_queue, &args)?;

            let Some(manager) = &state.cosmic_toplevel_manager else {
                return Err(CliError::new(
                    "Compositor does not support toplevel management protocol.".into(),
                ));
            };

            for app in apps_to_modify {
                if args.maximize {
                    manager.set_maximized(&app.handle);
                }
                if args.unmaximize {
                    manager.unset_maximized(&app.handle);
                }
                if args.minimize {
                    manager.set_minimized(&app.handle);
                }
                if args.unminimize {
                    manager.unset_minimized(&app.handle);
                }
                if args.fullscreen {
                    manager.set_fullscreen(&app.handle, None);
                }
                if args.unfullscreen {
                    manager.unset_fullscreen(&app.handle);
                }
                if args.sticky {
                    manager.set_sticky(&app.handle);
                }
                if args.unsticky {
                    manager.unset_sticky(&app.handle);
                }
            }

            conn.flush()?;
        }
    };

    Ok(())
}

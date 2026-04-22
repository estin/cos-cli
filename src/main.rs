use cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1;
use cosmic_protocols::toplevel_management::v1::client::zcosmic_toplevel_manager_v1;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use wayland_client::{
    Connection, EventQueue,
    backend::ObjectId,
    protocol::{wl_output, wl_seat},
};
use wayland_protocols::ext::workspace::v1::client::{
    // ext_workspace_group_handle_v1,
    ext_workspace_handle_v1,
};

use crate::server::Backend;

mod dispatch;
mod server;

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_ansi(std::env::var_os("NO_COLOR").is_none())
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
  serve                         Start a stdio JSON-RPC server

Options for 'move':
  -a, --app-id <ID>             The Application ID (partial match, case-insensitive)
  -i, --index <INDEX>           The Application index from 'info' command
  -w, --workspace <INDEX>       The index of the target workspace
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
  --discover-wg-output          Try to find info relation about workspace group and output

Examples:
  cos-cli info
  cos-cli info --json
  cos-cli info --discover-wg-output
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

#[derive(Clone)]
struct NamedHandle<T> {
    name: Option<String>,
    handle: T,
}

impl<T: Clone> NamedHandle<T> {
    // fn new(handle: T) -> Self {
    //     Self { name: None, handle }
    // }
    fn named(name: &str, handle: T) -> Self {
        Self {
            name: name.to_string().into(),
            handle,
        }
    }
}

#[derive(Default)]
struct HandleMap {
    // workspace_group_handle:
    //     HashMap<ObjectId, NamedHandle<ext_workspace_group_handle_v1::ExtWorkspaceGroupHandleV1>>,
    workspace_handle: HashMap<ObjectId, NamedHandle<ext_workspace_handle_v1::ExtWorkspaceHandleV1>>,
    output: HashMap<ObjectId, NamedHandle<wl_output::WlOutput>>,
    seat: HashMap<ObjectId, NamedHandle<wl_seat::WlSeat>>,
}

struct WorkspaceGroup {
    object_id: ObjectId,
    workspaces: Vec<ObjectId>,
    outputs: Vec<ObjectId>,
}

impl HandleMap {
    fn workspace_names(&self, group: &WorkspaceGroup) -> impl Iterator<Item = &str> {
        group
            .workspaces
            .iter()
            .filter_map(|w| self.workspace_handle.get(w))
            .filter_map(|nh| nh.name.as_deref())
    }
}

#[derive(Default)]
struct AppState {
    handle_map: HandleMap,
    cosmic_toplevel_manager: Option<zcosmic_toplevel_manager_v1::ZcosmicToplevelManagerV1>,
    available_interfaces: HashMap<String, Vec<(u32, u32)>>,
    workspace_groups: Vec<WorkspaceGroup>,
    outputs: Vec<ObjectId>,
    seats: Vec<ObjectId>,
    apps: Vec<App>,
}

impl AppState {
    fn new() -> Self {
        Self::default()
    }

    fn entities_count(&self) -> usize {
        self.outputs.len() + self.seats.len() + self.apps.len() + self.workspace_groups.len()
    }
}

#[derive(Debug, Clone)]
struct App {
    handle: zcosmic_toplevel_handle_v1::ZcosmicToplevelHandleV1,
    title: Option<String>,
    app_id: Option<String>,
    outputs: Vec<ObjectId>,
    workspaces: Vec<ObjectId>,
    state: Vec<State>,
}

#[derive(Serialize)]
struct JsonApp {
    index: usize,
    app_id: String,
    title: String,
    state: Vec<State>,
    outputs: Vec<JsonOutputRef>,
    workspaces: Vec<JsonWorkspaceRef>,
}

#[derive(Serialize)]
struct JsonOutputRef {
    index: usize,
    name: String,
}

#[derive(Serialize)]
struct JsonWorkspaceRef {
    group_index: usize,
    index: usize,
    workspace: String,
}

#[derive(Serialize)]
struct JsonWorkspace {
    index: usize,
    name: String,
}

#[derive(Serialize)]
struct JsonWorkspaceGroup {
    index: usize,
    workspaces: Vec<JsonWorkspace>,
    outputs: Vec<String>,
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
    workspace_groups: Vec<JsonWorkspaceGroup>,
    outputs: Vec<JsonOutput>,
    seats: Vec<JsonSeat>,
}

impl From<&AppState> for JsonInfo {
    fn from(state: &AppState) -> Self {
        Self {
            apps: state
                .apps
                .iter()
                .enumerate()
                .map(|(i, app)| {
                    let outputs = app
                        .outputs
                        .iter()
                        .filter_map(|o| {
                            state.handle_map.output.get(o).and_then(|h| {
                                JsonOutputRef {
                                    index: state.outputs.iter().position(|oid| oid == o)?,
                                    name: h.name.clone()?,
                                }
                                .into()
                            })
                        })
                        .collect();
                    let workspaces = app
                        .workspaces
                        .iter()
                        .filter_map(|w| {
                            Some(JsonWorkspaceRef {
                                index: state
                                    .workspace_groups
                                    .iter()
                                    .filter_map(|wg| wg.workspaces.iter().position(|i| i == w))
                                    .next()?,
                                group_index: state
                                    .workspace_groups
                                    .iter()
                                    .position(|wg| wg.workspaces.contains(w))?,
                                workspace: state
                                    .handle_map
                                    .workspace_handle
                                    .get(w)
                                    .and_then(|nh| nh.name.as_deref())
                                    .unwrap_or("not found")
                                    .to_string(),
                            })
                        })
                        .collect();
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
            workspace_groups: state
                .workspace_groups
                .iter()
                .enumerate()
                .map(|(i, group)| JsonWorkspaceGroup {
                    index: i,
                    workspaces: state
                        .handle_map
                        .workspace_names(group)
                        .map(ToOwned::to_owned)
                        .enumerate()
                        .map(|(index, name)| JsonWorkspace { index, name })
                        .collect(),
                    outputs: group
                        .outputs
                        .iter()
                        .filter_map(|oid| {
                            state
                                .handle_map
                                .output
                                .get(oid)
                                .and_then(|h| h.name.clone())
                        })
                        .collect(),
                })
                .collect(),
            outputs: state
                .outputs
                .iter()
                .enumerate()
                .filter_map(|(i, oid)| {
                    state.handle_map.output.get(oid).map(|h| JsonOutput {
                        index: i,
                        name: h.name.clone().unwrap_or_default(),
                    })
                })
                .collect(),
            seats: state
                .seats
                .iter()
                .enumerate()
                .filter_map(|(i, sid)| {
                    state.handle_map.seat.get(sid).map(|h| JsonSeat {
                        index: i,
                        name: h.name.clone().unwrap_or_default(),
                    })
                })
                .collect(),
        }
    }
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
    discover_wg_output: bool,
}

enum Command {
    Info(InfoArgs),
    Move(MoveArgs),
    Activate(ActivateArgs),
    State(StateArgs),
    Serve,
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
            discover_wg_output: pargs.contains("--discover-wg-output"),
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
        Some("serve") => Command::Serve,
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

    let mut state = AppState::new();
    let registry = conn.display().get_registry(&qh, ());

    event_queue.roundtrip(&mut state)?;
    dispatch::bind(&registry, &qh, &mut state);
    event_queue.roundtrip(&mut state)?;
    tracing::debug!("Discovered {}", state.entities_count(),);

    let mut entities_count = 0;
    for i in 0..10 {
        event_queue.roundtrip(&mut state)?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        let new_entities_count = state.entities_count();
        tracing::debug!("Step {i}. Discovered {new_entities_count} (previous: {entities_count})");

        if new_entities_count == entities_count {
            break;
        }
        entities_count = new_entities_count;
    }

    match command {
        Command::Info(args) => {
            for app in &state.apps {
                state
                    .workspace_groups
                    .iter_mut()
                    .filter(|wg| wg.workspaces.iter().any(|w| app.workspaces.contains(w)))
                    .for_each(|wg| {
                        for output in &app.outputs {
                            if !wg.outputs.contains(output) {
                                wg.outputs.push(output.clone());
                            }
                        }
                    });
            }

            if args.discover_wg_output {
                let Some(manager) = state.cosmic_toplevel_manager.clone() else {
                    return Err(CliError::new(
                        "Compositor does not support workspace management protocol.".into(),
                    ));
                };
                let Some(last_app) = state.apps.last().cloned() else {
                    return Err(CliError::new(
                        "No apps found to discover workspace group outputs".into(),
                    ));
                };

                let Some(initial_app_output) = last_app.outputs.first() else {
                    return Err(CliError::new("App without output".into()));
                };

                let Some(initial_app_workspace) = last_app.workspaces.first() else {
                    return Err(CliError::new("App without workspace".into()));
                };

                let Some(initial_app_group) = state
                    .workspace_groups
                    .iter()
                    .find(|wg| wg.workspaces.contains(initial_app_workspace))
                    .map(|wg| wg.object_id.to_owned())
                else {
                    return Err(CliError::new("Workspace without group.".into()));
                };

                let move_plan = state
                    .workspace_groups
                    .iter()
                    .enumerate()
                    .filter_map(|(index, g)| {
                        if g.object_id != initial_app_group {
                            Some((index, g.object_id.clone(), g.workspaces.first()?.clone()))
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();

                // start move app from one workspace group to another
                let mut was_moved = false;
                for (group_index, group_id, workspace_id) in move_plan {
                    let Some(nh) = state
                        .handle_map
                        .workspace_handle
                        .get(&workspace_id)
                        .cloned()
                    else {
                        continue;
                    };

                    for (output_index, output_id) in state.outputs.clone().into_iter().enumerate() {
                        if initial_app_output == &output_id {
                            continue;
                        }

                        let Some(output) = state.handle_map.output.get(&output_id).cloned() else {
                            continue;
                        };

                        tracing::debug!(
                            "Moving app to workspace group {group_index} workspace '{}' at output: {output_index}",
                            nh.name.as_deref().unwrap_or_default()
                        );
                        manager.move_to_ext_workspace(&last_app.handle, &nh.handle, &output.handle);
                        conn.flush()?;

                        std::thread::sleep(std::time::Duration::from_millis(300));
                        event_queue.roundtrip(&mut state)?;

                        // check new state
                        let Some(new_app_state) =
                            state.apps.iter().find(|a| a.handle == last_app.handle)
                        else {
                            continue;
                        };

                        // app was change output - associate it with workspace group
                        if new_app_state.outputs.contains(&output_id) {
                            was_moved = true;
                            tracing::debug!(
                                "App moved. Add output {output_index} to the workspace group"
                            );
                            let Some(wg) = state
                                .workspace_groups
                                .iter_mut()
                                .find(|g| g.object_id == group_id)
                            else {
                                continue;
                            };
                            if !wg.outputs.contains(&output_id) {
                                wg.outputs.push(output_id.clone())
                            }
                        } else {
                            tracing::debug!("App not changes output");
                        }
                    }
                }

                if was_moved
                    && let Some(workspace) = state
                        .handle_map
                        .workspace_handle
                        .get(initial_app_workspace)
                        .map(|nh| &nh.handle)
                    && let Some(output) = state
                        .handle_map
                        .output
                        .get(initial_app_output)
                        .map(|nh| &nh.handle)
                {
                    tracing::debug!("Discover output by app moving done");
                    // move app back
                    manager.move_to_ext_workspace(&last_app.handle, workspace, output);
                    conn.flush()?;

                    std::thread::sleep(std::time::Duration::from_millis(300));
                    event_queue.roundtrip(&mut state)?;
                } else {
                    tracing::debug!("Failed to discover output by app moving");
                }
            }

            if args.json {
                let json_info = JsonInfo::from(&state);
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
                                .handle_map
                                .output
                                .get(o)
                                .and_then(|h| h.name.as_deref())
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
                                .enumerate()
                                .find_map(|(gi, g)| {
                                    g.workspaces
                                        .iter()
                                        .position(|hw| hw == w)
                                        .map(|wi| format!("{}.\"{}\"", gi, wi))
                                })
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
                    let output_names = group
                        .outputs
                        .iter()
                        .filter_map(|oid| {
                            state
                                .handle_map
                                .output
                                .get(oid)
                                .and_then(|h| h.name.as_deref())
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    if output_names.is_empty() {
                        println!("\t[{i}] Group (outputs: undiscovered)");
                    } else {
                        println!("\t[{i}] Group (outputs: {output_names})");
                    }
                    for (j, _) in group.workspaces.iter().enumerate() {
                        println!("\t\tWorkspace: {j}");
                    }
                }
                println!("Outputs:");
                for (i, oid) in state.outputs.iter().enumerate() {
                    let name = state
                        .handle_map
                        .output
                        .get(oid)
                        .and_then(|h| h.name.as_deref())
                        .unwrap_or("unknown");
                    println!("\t[{i}] Output: {name}");
                }

                println!("Seats:");
                for (i, sid) in state.seats.iter().enumerate() {
                    let name = state
                        .handle_map
                        .seat
                        .get(sid)
                        .and_then(|h| h.name.as_deref())
                        .unwrap_or("unknown");
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

            let workspace_index = if let Some(group_index) = args.workspace_group_index {
                let group = state.workspace_groups.get(group_index).ok_or_else(|| {
                    CliError::new(format!("Workspace group not found: {}", group_index))
                })?;
                let idx = args.workspace_name.parse::<usize>().map_err(|_| {
                    CliError::new(format!("Invalid workspace index: {}", args.workspace_name))
                })?;
                if idx >= group.workspaces.len() {
                    return Err(CliError::new(format!(
                        "Workspace index {} out of range (group has {} workspaces)",
                        idx,
                        group.workspaces.len()
                    )));
                }
                (group_index, idx)
            } else {
                let mut found = None;
                for (gi, group) in state.workspace_groups.iter().enumerate() {
                    for (wi, _) in group.workspaces.iter().enumerate() {
                        if format!("{}", wi) == args.workspace_name {
                            found = Some((gi, wi));
                            break;
                        }
                    }
                    if found.is_some() {
                        break;
                    }
                }
                found.ok_or_else(|| {
                    CliError::new(format!("Workspace not found: {}", args.workspace_name))
                })?
            };

            let (group_index, idx) = workspace_index;
            let workspace_handle = &state.workspace_groups[group_index].workspaces[idx];
            let Some(workspace) = state.handle_map.workspace_handle.get(workspace_handle) else {
                return Err(CliError::new(
                    "Workspace handle not found in handle map".into(),
                ));
            };

            let output = if let Some(index) = args.output_index {
                let oid = state
                    .outputs
                    .get(index)
                    .ok_or_else(|| CliError::new(format!("Output index not found: {}", index)))?;
                state
                    .handle_map
                    .output
                    .get(oid)
                    .map(|h| h.handle.clone())
                    .ok_or_else(|| CliError::new("Output handle not found in handle map".into()))?
            } else {
                if state.outputs.is_empty() {
                    return Err(CliError::new("No outputs found.".to_string()));
                }
                let oid = &state.outputs[0];
                state
                    .handle_map
                    .output
                    .get(oid)
                    .map(|h| h.handle.clone())
                    .ok_or_else(|| CliError::new("Output handle not found in handle map".into()))?
            };

            for app in apps_to_move {
                println!(
                    "Move {} to {}",
                    app.app_id.as_deref().unwrap_or_default(),
                    args.workspace_name,
                );
                manager.move_to_ext_workspace(&app.handle, &workspace.handle, &output);
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
                let sid = state.seats.get(seat_index).ok_or_else(|| {
                    CliError::new(format!("Seat index not found: {}", seat_index))
                })?;
                state
                    .handle_map
                    .seat
                    .get(sid)
                    .map(|h| &h.handle)
                    .ok_or_else(|| CliError::new("Seat handle not found in handle map".into()))?
            } else {
                let sid = state
                    .seats
                    .first()
                    .ok_or_else(|| CliError::new("No seats found.".to_string()))?;
                state
                    .handle_map
                    .seat
                    .get(sid)
                    .map(|h| &h.handle)
                    .ok_or_else(|| CliError::new("Seat handle not found in handle map".into()))?
            };
            manager.activate(&app.handle, seat);
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
        Command::Serve => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(async move {
                server::run(Backend {
                    connection: conn,
                    event_queue,
                    app_state: state,
                })
                .await?;
                Ok::<_, Box<dyn std::error::Error>>(())
            })?;
        }
    };

    Ok(())
}

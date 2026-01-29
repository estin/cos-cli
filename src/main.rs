use cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1;
use cosmic_protocols::toplevel_management::v1::client::zcosmic_toplevel_manager_v1;
use std::error::Error;
use std::fmt;

use wayland_client::{
    Connection,
    protocol::{wl_output, wl_seat},
};
use wayland_protocols::ext::workspace::v1::client::ext_workspace_handle_v1;

mod dispatch;

const HELP: &str = "\
Usage: cos-cli [COMMAND]

A CLI utility for COSMIC Wayland toplevel and workspace management.

Commands:
  info                          List active apps, workspaces, and outputs
  move                          Move an application to a specific workspace
  activate                      Activate an application on a specific seat

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

Examples:
  cos-cli info
  cos-cli move --app-id Firefox --workspace 2
  cos-cli move -i 0 -w 2
  cos-cli move -a terminal -w 2 --wait 5
  cos-cli move -a terminal -w 2 -o 1
  cos-cli move -a terminal -w 2 -g 1
  cos-cli activate -i 0 -s 0
  cos-cli activate -i 0
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

struct AppState {
    workspace_group: Vec<Vec<(String, ext_workspace_handle_v1::ExtWorkspaceHandleV1)>>,
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
    // workspaces: Vec<zcosmic_workspace_handle_v1::ZcosmicWorkspaceHandleV1>,
    // state: Vec<State>,
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

enum Command {
    Info,
    Move(MoveArgs),
    Activate(ActivateArgs),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut pargs = pico_args::Arguments::from_env();

    let subcommand = pargs.subcommand()?;

    let command = match subcommand.as_deref() {
        Some("info") => Command::Info,
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
        cosmic_toplevel_manager: None,
        workspace_group: Vec::new(),
        apps: Vec::new(),
        outputs: Vec::new(),
        seats: Vec::new(),
    };
    let _registry = conn.display().get_registry(&qh, ());

    event_queue.roundtrip(&mut state)?;
    event_queue.roundtrip(&mut state)?;

    match command {
        Command::Info => {
            println!("Apps:");
            for (i, app) in state.apps.iter().enumerate() {
                println!(
                    "\t[{}] {} (title: {})",
                    i,
                    app.app_id.as_deref().unwrap_or_default(),
                    app.title.as_deref().unwrap_or_default()
                );
            }
            println!("Workspaces:");
            for (i, group) in state.workspace_group.iter().enumerate() {
                println!("\t[{i}] Group");
                for (workspace, _) in group {
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
        Command::Move(args) => {
            let apps_to_move: Vec<App> = if let Some(app_index) = args.app_index {
                if let Some(app) = state.apps.get(app_index) {
                    vec![app.clone()]
                } else {
                    return Err(CliError::new(format!("App index not found: {}", app_index)));
                }
            } else if let Some(app_id) = &args.app_id {
                let sleep = std::time::Duration::from_millis(500);
                let wait_dur = args.wait.map(std::time::Duration::from_secs);
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
                        event_queue.roundtrip(&mut state)?;
                    } else {
                        break;
                    }
                }
                if apps.is_empty() {
                    return Err(CliError::new(format!("App id not found: {}", app_id)));
                }
                apps
            } else {
                unreachable!(); // Already handled by arg parsing
            };

            let Some(manager) = &state.cosmic_toplevel_manager else {
                return Err(CliError::new(
                    "Compositor does not support workspace management protocol.".into(),
                ));
            };
            println!("Connected to cosmic toplevel manager!");

            let Some((_, workspace)) = (if let Some(group_index) = args.workspace_group_index {
                if let Some(group) = state.workspace_group.get(group_index) {
                    group.iter().find(|(w, _)| w == &args.workspace_name)
                } else {
                    return Err(CliError::new(format!(
                        "Workspace group not found: {}",
                        group_index
                    )));
                }
            } else {
                state
                    .workspace_group
                    .iter()
                    .flat_map(|v| v.iter())
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
    };

    Ok(())
}

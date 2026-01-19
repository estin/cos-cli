use cosmic_protocols::toplevel_info::v1::client::zcosmic_toplevel_handle_v1;
use cosmic_protocols::toplevel_management::v1::client::zcosmic_toplevel_manager_v1;
use std::error::Error;
use std::fmt;

use wayland_client::{Connection, protocol::wl_output};
use wayland_protocols::ext::workspace::v1::client::ext_workspace_handle_v1;

mod dispatch;

const HELP: &str = "\
Usage: cos-cli [COMMAND]

A CLI utility for COSMIC Wayland toplevel and workspace management.

Commands:
  info                  List active apps, workspaces, and outputs
  move                  Move an application to a specific workspace

Options for 'move':
  -a, --app-id <ID>      The Application ID (partial match, case-insensitive)
  -w, --workspace <NAME> The name of the target workspace
  --wait <SECONDS>       Wait for the app to appear (optional)

Examples:
  cos-cli info
  cos-cli move --app-id Firefox --workspace 2
  cos-cli move -a terminal -w 2 --wait 5
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
    workspaces: Vec<(String, ext_workspace_handle_v1::ExtWorkspaceHandleV1)>,
    cosmic_toplevel_manager: Option<zcosmic_toplevel_manager_v1::ZcosmicToplevelManagerV1>,
    outputs: Vec<(wl_output::WlOutput, String)>,
    apps: Vec<App>,
}

#[derive(Debug)]
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
    app_id: String,
    workspace_name: String,
    wait: Option<u64>,
}

enum Command {
    Info,
    Move(MoveArgs),
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut pargs = pico_args::Arguments::from_env();

    let subcommand = pargs.subcommand()?;

    let command = match subcommand.as_deref() {
        Some("info") => Command::Info,
        Some("move") => Command::Move(MoveArgs {
            app_id: pargs.value_from_str(["-a", "--app-id"])?,
            workspace_name: pargs.value_from_str(["-w", "--workspace"])?,
            wait: pargs.opt_value_from_fn("--wait", |v| v.parse())?,
        }),
        Some(_) => {
            return Err(CliError::new(format!(
                "Unknown subcommand: {}",
                subcommand.unwrap_or_default()
            )));
        }
        None => {
            println!("{HELP}");
            return Ok(());
        }
    };

    let conn = Connection::connect_to_env()?;
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();

    let mut state = AppState {
        cosmic_toplevel_manager: None,
        workspaces: Vec::new(),
        apps: Vec::new(),
        outputs: Vec::new(),
    };
    let _registry = conn.display().get_registry(&qh, ());

    event_queue.roundtrip(&mut state)?;
    event_queue.roundtrip(&mut state)?;

    match command {
        Command::Info => {
            println!("Apps:");
            for app in &state.apps {
                println!(
                    "\t{} (title: {})",
                    app.app_id.as_deref().unwrap_or_default(),
                    app.title.as_deref().unwrap_or_default()
                );
            }
            println!("Workspaces:");
            for (workspace, _) in &state.workspaces {
                println!("\tWorkspace: {workspace}");
            }
            println!("Outputs:");
            for (_, name) in &state.outputs {
                println!("\tOutput: {name}");
            }
        }
        Command::Move(args) => {
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
                            .map(|v| v.to_lowercase().contains(&args.app_id.to_lowercase()))
                            .unwrap_or_default()
                    })
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
                return Err(CliError::new(format!("App id not found: {}", args.app_id)));
            }

            let Some(manager) = &state.cosmic_toplevel_manager else {
                return Err(CliError::new(
                    "Compositor does not support workspace management protocol.".into(),
                ));
            };
            println!("Connected to cosmic toplevel manager!");

            let Some((_, workspace)) = state
                .workspaces
                .iter()
                .find(|(w, _)| w == &args.workspace_name)
            else {
                return Err(CliError::new(format!(
                    "Workspace not found: {}",
                    args.workspace_name
                )));
            };

            let output = state.outputs[0].0.clone();
            for app in apps {
                println!(
                    "Move {} to {}",
                    app.app_id.as_deref().unwrap_or_default(),
                    args.workspace_name,
                );
                manager.move_to_ext_workspace(&app.handle, workspace, &output);
            }

            conn.flush()?;
        }
    };

    Ok(())
}

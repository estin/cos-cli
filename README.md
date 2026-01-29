# cos-cli

A CLI tool for managing windows and workspaces on the COSMIC Desktop Environment (Wayland).

> **Note:** This is a third-party, unofficial tool. It is not affiliated with System76 or the official COSMIC project.

## Features
- **List Information**: View active applications, workspaces, and outputs.
- **Window Management**: Move applications between workspaces by their App ID.
- **Smart Wait**: Option to wait for an application to launch before moving it.

## Installation
Ensure you have the Rust toolchain installed.

````console
cargo build --release
````

or from github directly

````console
cargo install --git https://github.com/estin/cos-cli
````

## Usage
````console
cos-cli [COMMAND]
````

### Commands

#### `info`
List all available apps, workspaces, outputs and seats.
````console
cos-cli info
````
Example output:
````
Apps:
	[0] firefox (title: Gemini - Mozilla Firefox)
	[1] org.wezfurlong.wezterm (title: cos-cli)
Workspaces:
	[0] Group
		Workspace: 1
		Workspace: 2
		Workspace: 3
Outputs:
	[0] Output: eDP-1
Seats:
	[0] Seat: seat0
````

With `--json` option it will output all info in JSON format.
````console
cos-cli info --json
````
Example output:
````json
{"apps":[{"index":0,"app_id":"firefox","title":"Gemini - Mozilla Firefox"},{"index":1,"app_id":"org.wezfurlong.wezterm","title":"cos-cli"}],"workspaces":[{"index":0,"workspaces":[{"name":"1"},{"name":"2"},{"name":"3"}]}],"outputs":[{"index":0,"name":"eDP-1"}],"seats":[{"index":0,"name":"seat0"}]}
````

Using `jq` to find app index by pattern and activate app
````console
cos-cli activate -i $(cos-cli info --json | jq '.apps[] | select(.app_id | test("wezterm")) | .index')
````

#### `move`
Move an application to a specific workspace.
````console
cos-cli move --app-id <ID> --workspace <NAME>
````
Arguments:
  -a, --app-id <ID>             The Application ID (partial match, case-insensitive)
  -i, --index <INDEX>           The Application index from 'info' command
  -w, --workspace <NAME>        The name of the target workspace
  -g, --workspace-group <INDEX> The workspace group index from 'info' command (optional)
  -o, --output-index <INDEX>    The output index from 'info' command (optional)
  --wait <SECONDS>              Wait for the app to appear (optional, only for --app-id)

#### `activate`
Activate an application.
````console
cos-cli activate --index <INDEX>
````
Arguments:
  -i, --index <INDEX>           The Application index from 'info' command
  -s, --seat <INDEX>            The Seat index from 'info' command (optional)

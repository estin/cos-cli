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

## Move app to a specific workspace

````console
cos-cli move --app-id [ID] --workspace [NAME] [--wait SECONDS]
````

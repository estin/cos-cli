use futures::FutureExt;
use jsonrpc_stdio_server::jsonrpc_core::{Error, ErrorCode, IoHandler, Params};
use serde::Deserialize;
use std::error::Error as StdError;
use std::sync::Arc;
use std::thread;
use tokio::sync::mpsc::{Sender, channel};
use tokio::sync::oneshot;

use crate::{App, AppState, JsonInfo};

use wayland_client::{Connection, EventQueue};

/// Wraps a `JoinHandle` and checks for panics when dropped.
struct TaskGuard {
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl TaskGuard {
    fn new(handle: tokio::task::JoinHandle<()>) -> Self {
        Self {
            handle: Some(handle),
        }
    }
}

impl Drop for TaskGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take()
            && handle.is_finished()
            && let Some(Err(e)) = handle.now_or_never()
        {
            tracing::warn!("Background task panicked: {e:?}");
        }
    }
}

// ---------------------------------------------------------------------------
// Parameter types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct MoveParams {
    #[serde(default)]
    pub app_id: Option<String>,
    #[serde(default)]
    pub index: Option<usize>,
    pub workspace: usize,
    #[serde(default)]
    pub workspace_group: Option<usize>,
    #[serde(default)]
    pub output_index: Option<usize>,
    #[serde(default)]
    pub wait: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct ActivateParams {
    pub index: usize,
    #[serde(default)]
    pub seat: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct StateParams {
    #[serde(default)]
    pub app_id: Option<String>,
    #[serde(default)]
    pub index: Option<usize>,
    #[serde(default)]
    pub wait: Option<u64>,
    #[serde(default)]
    pub maximize: bool,
    #[serde(default)]
    pub unmaximize: bool,
    #[serde(default)]
    pub minimize: bool,
    #[serde(default)]
    pub unminimize: bool,
    #[serde(default)]
    pub fullscreen: bool,
    #[serde(default)]
    pub unfullscreen: bool,
    #[serde(default)]
    pub sticky: bool,
    #[serde(default)]
    pub unsticky: bool,
}

#[derive(Debug, Deserialize)]
pub struct ActivateWsParams {
    pub workspace: usize,
    #[serde(default)]
    pub workspace_group: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct CloseParams {
    #[serde(default)]
    pub app_id: Option<String>,
    #[serde(default)]
    pub index: Option<usize>,
}

pub enum BackendRequestParams {
    GetInfo,
    Move(MoveParams),
    Activate(ActivateParams),
    ActivateWs(ActivateWsParams),
    State(StateParams),
    Close(CloseParams),
}

pub struct BackendRequest {
    response_tx: oneshot::Sender<BackendResponse>,
    params: BackendRequestParams,
}

impl BackendRequest {
    fn request(params: BackendRequestParams) -> (oneshot::Receiver<BackendResponse>, Self) {
        let (response_tx, rx) = oneshot::channel();
        (
            rx,
            Self {
                params,
                response_tx,
            },
        )
    }
}

#[derive(serde::Serialize)]
#[serde(untagged)]
pub enum BackendResponse {
    Info(JsonInfo),
    Ok(String),
    Err(String),
}

fn internal_error(message: &str) -> Error {
    Error {
        code: ErrorCode::InternalError,
        message: message.to_string(),
        data: None,
    }
}

fn invalid_params(message: &str) -> Error {
    Error {
        code: ErrorCode::InvalidParams,
        message: message.to_string(),
        data: None,
    }
}

// ---------------------------------------------------------------------------
// Synchronous event-loop thread
// ---------------------------------------------------------------------------

/// Owned resources for the dedicated wayland event-loop thread.
struct WaylandThread {
    event_queue: EventQueue<AppState>,
    app_state: AppState,
    connection: Connection,
    request_rx: std::sync::mpsc::Receiver<BackendRequest>,
}

impl WaylandThread {
    fn run(mut self) {
        let sleep = std::time::Duration::from_millis(300);
        loop {
            // Process any pending commands first (non-blocking check)
            while let Ok(request) = self.request_rx.try_recv() {
                self.process(request);
            }

            // Blocking roundtrip – safe here because this is a dedicated OS thread
            let _ = self.event_queue.roundtrip(&mut self.app_state);
            let _ = self.connection.flush();

            thread::sleep(sleep);
        }
    }

    fn process(&mut self, request: BackendRequest) {
        use BackendRequestParams::*;
        let response = match request.params {
            GetInfo => BackendResponse::Info(JsonInfo::from(&self.app_state)),
            Move(p) => match self.handle_move(p) {
                Ok(msg) => BackendResponse::Ok(msg),
                Err(e) => BackendResponse::Err(e.to_string()),
            },
            Activate(p) => match self.handle_activate(p) {
                Ok(msg) => BackendResponse::Ok(msg),
                Err(e) => BackendResponse::Err(e.to_string()),
            },
            ActivateWs(p) => match self.handle_activate_ws(p) {
                Ok(msg) => BackendResponse::Ok(msg),
                Err(e) => BackendResponse::Err(e.to_string()),
            },
            State(p) => match self.handle_state(p) {
                Ok(msg) => BackendResponse::Ok(msg),
                Err(e) => BackendResponse::Err(e.to_string()),
            },
            Close(p) => match self.handle_close(p) {
                Ok(msg) => BackendResponse::Ok(msg),
                Err(e) => BackendResponse::Err(e.to_string()),
            },
        };
        let _ = request.response_tx.send(response);
    }

    fn handle_move(&mut self, params: MoveParams) -> Result<String, Box<dyn StdError>> {
        let apps = self.find_apps(params.app_id.clone(), params.index, params.wait)?;

        let manager = self
            .app_state
            .cosmic_toplevel_manager
            .as_ref()
            .ok_or_else(|| {
                "Compositor does not support workspace management protocol.".to_string()
            })?;

        let workspace_index = if let Some(group_index) = params.workspace_group {
            let group = self
                .app_state
                .workspace_groups
                .get(group_index)
                .ok_or_else(|| format!("Workspace group not found: {}", group_index))?;
            if params.workspace >= group.workspaces.len() {
                return Err(format!(
                    "Workspace index {} out of range (group has {} workspaces)",
                    params.workspace,
                    group.workspaces.len()
                )
                .into());
            }
            (group_index, params.workspace)
        } else {
            let mut found = None;
            for (gi, group) in self.app_state.workspace_groups.iter().enumerate() {
                for (wi, _) in group.workspaces.iter().enumerate() {
                    if wi == params.workspace {
                        found = Some((gi, wi));
                        break;
                    }
                }
                if found.is_some() {
                    break;
                }
            }
            found.ok_or_else(|| format!("Workspace not found: {}", params.workspace))?
        };

        let (group_index, idx) = workspace_index;
        let workspace_handle = &self.app_state.workspace_groups[group_index].workspaces[idx];
        let Some(workspace) = self
            .app_state
            .handle_map
            .workspace_handle
            .get(workspace_handle)
        else {
            return Err("Workspace handle not found in handle map".into());
        };

        let output = if let Some(index) = params.output_index {
            let oid = self
                .app_state
                .outputs
                .get(index)
                .ok_or_else(|| format!("Output index not found: {}", index))?;
            self.app_state
                .handle_map
                .output
                .get(oid)
                .map(|h| h.handle.clone())
                .ok_or("Output handle not found in handle map")?
        } else {
            if self.app_state.outputs.is_empty() {
                return Err("No outputs found.".to_string().into());
            }
            let oid = &self.app_state.outputs[0];
            self.app_state
                .handle_map
                .output
                .get(oid)
                .map(|h| h.handle.clone())
                .ok_or("Output handle not found in handle map")?
        };

        for app in &apps {
            manager.move_to_ext_workspace(&app.handle, &workspace.handle, &output);
        }

        self.connection.flush()?;

        let app_desc = params.app_id.clone().unwrap_or_else(|| "app".to_string());
        Ok(format!(
            "Moved {} to workspace {}",
            app_desc, params.workspace
        ))
    }

    fn handle_activate(&mut self, params: ActivateParams) -> Result<String, Box<dyn StdError>> {
        let manager = self
            .app_state
            .cosmic_toplevel_manager
            .as_ref()
            .ok_or_else(|| {
                "Compositor does not support toplevel management protocol.".to_string()
            })?;

        let Some(app) = self.app_state.apps.get(params.index) else {
            return Err(format!("App index not found: {}", params.index).into());
        };

        let seat = if let Some(seat_index) = params.seat {
            let sid = self
                .app_state
                .seats
                .get(seat_index)
                .ok_or_else(|| format!("Seat index not found: {}", seat_index))?;
            self.app_state
                .handle_map
                .seat
                .get(sid)
                .map(|h| &h.handle)
                .ok_or("Seat handle not found in handle map")?
        } else {
            let sid = self
                .app_state
                .seats
                .first()
                .ok_or_else(|| "No seats found.".to_string())?;
            self.app_state
                .handle_map
                .seat
                .get(sid)
                .map(|h| &h.handle)
                .ok_or("Seat handle not found in handle map")?
        };

        manager.activate(&app.handle, seat);
        self.connection.flush()?;

        Ok(format!("Activated app at index {}", params.index))
    }

    fn handle_activate_ws(
        &mut self,
        params: ActivateWsParams,
    ) -> Result<String, Box<dyn StdError>> {
        let Some(manager) = &self.app_state.workspace_manager else {
            return Err("Compositor does not support workspace management protocol.".into());
        };
        let workspace_index = if let Some(group_index) = params.workspace_group {
            let group = self
                .app_state
                .workspace_groups
                .get(group_index)
                .ok_or_else(|| format!("Workspace group not found: {}", group_index))?;
            if params.workspace >= group.workspaces.len() {
                return Err(format!(
                    "Workspace index {} out of range (group has {} workspaces)",
                    params.workspace,
                    group.workspaces.len()
                )
                .into());
            }
            (group_index, params.workspace)
        } else {
            let mut found = None;
            for (gi, group) in self.app_state.workspace_groups.iter().enumerate() {
                for (wi, _) in group.workspaces.iter().enumerate() {
                    if wi == params.workspace {
                        found = Some((gi, wi));
                        break;
                    }
                }
                if found.is_some() {
                    break;
                }
            }
            found.ok_or_else(|| format!("Workspace not found: {}", params.workspace))?
        };

        let (group_index, idx) = workspace_index;
        let workspace_handle = &self.app_state.workspace_groups[group_index].workspaces[idx];
        let Some(ws) = self
            .app_state
            .handle_map
            .workspace_handle
            .get(workspace_handle)
        else {
            return Err("Workspace handle not found in handle map".into());
        };

        ws.handle.activate();
        manager.commit();
        self.connection.flush()?;

        Ok(format!("Activated workspace {}", params.workspace))
    }

    fn handle_state(&mut self, params: StateParams) -> Result<String, Box<dyn StdError>> {
        let apps = self.find_apps(params.app_id.clone(), params.index, params.wait)?;

        let manager = self
            .app_state
            .cosmic_toplevel_manager
            .as_ref()
            .ok_or_else(|| {
                "Compositor does not support toplevel management protocol.".to_string()
            })?;

        let mut actions = Vec::new();
        if params.maximize {
            actions.push("maximize");
        }
        if params.unmaximize {
            actions.push("unmaximize");
        }
        if params.minimize {
            actions.push("minimize");
        }
        if params.unminimize {
            actions.push("unminimize");
        }
        if params.fullscreen {
            actions.push("fullscreen");
        }
        if params.unfullscreen {
            actions.push("unfullscreen");
        }
        if params.sticky {
            actions.push("sticky");
        }
        if params.unsticky {
            actions.push("unsticky");
        }

        let apps_count = apps.len();
        for app in &apps {
            if params.maximize {
                manager.set_maximized(&app.handle);
            }
            if params.unmaximize {
                manager.unset_maximized(&app.handle);
            }
            if params.minimize {
                manager.set_minimized(&app.handle);
            }
            if params.unminimize {
                manager.unset_minimized(&app.handle);
            }
            if params.fullscreen {
                manager.set_fullscreen(&app.handle, None);
            }
            if params.unfullscreen {
                manager.unset_fullscreen(&app.handle);
            }
            if params.sticky {
                manager.set_sticky(&app.handle);
            }
            if params.unsticky {
                manager.unset_sticky(&app.handle);
            }
        }

        self.connection.flush()?;

        Ok(format!(
            "Set state for {} apps: {}",
            apps_count,
            actions.join(", ")
        ))
    }

    fn handle_close(&mut self, params: CloseParams) -> Result<String, Box<dyn StdError>> {
        let apps = self.find_apps(params.app_id.clone(), params.index, None)?;

        let manager = self
            .app_state
            .cosmic_toplevel_manager
            .as_ref()
            .ok_or_else(|| {
                "Compositor does not support toplevel management protocol.".to_string()
            })?;

        for app in &apps {
            manager.close(&app.handle);
        }

        self.connection.flush()?;

        let desc = params.app_id.clone().unwrap_or_else(|| "app".to_string());
        Ok(format!("Close request sent for {}", desc))
    }

    fn find_apps(
        &mut self,
        app_id: Option<String>,
        app_index: Option<usize>,
        wait: Option<u64>,
    ) -> Result<Vec<App>, Box<dyn StdError>> {
        if let Some(index) = app_index {
            if let Some(app) = self.app_state.apps.get(index) {
                Ok(vec![app.clone()])
            } else {
                Err(format!("App index not found: {}", index).into())
            }
        } else if let Some(id) = app_id {
            let sleep = std::time::Duration::from_millis(500);
            let wait_dur = wait.map(std::time::Duration::from_secs);
            let now = std::time::Instant::now();
            let mut apps;
            loop {
                apps = self
                    .app_state
                    .apps
                    .iter()
                    .filter(|app| {
                        app.app_id
                            .as_ref()
                            .map(|v| v.to_lowercase().contains(&id.to_lowercase()))
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
                    thread::sleep(sleep);
                    let _ = self.event_queue.roundtrip(&mut self.app_state);
                } else {
                    break;
                }
            }

            if apps.is_empty() {
                return Err(format!("App id not found: {}", id).into());
            }
            Ok(apps)
        } else {
            Err("Either app_id or index must be provided".into())
        }
    }
}

// ---------------------------------------------------------------------------
// Public ServerHandler & run()
// ---------------------------------------------------------------------------

struct ServerHandler {
    tx: Sender<BackendRequest>,
}

impl ServerHandler {
    fn new(tx: Sender<BackendRequest>) -> Self {
        Self { tx }
    }

    async fn handle_request(
        self: Arc<Self>,
        request_params: BackendRequestParams,
    ) -> Result<serde_json::Value, Error> {
        let (response_tx, request) = BackendRequest::request(request_params);
        self.tx
            .send(request)
            .await
            .map_err(|e| internal_error(&e.to_string()))?;
        let response = response_tx
            .await
            .map_err(|e| internal_error(&e.to_string()))?;
        serde_json::to_value(response).map_err(|e| internal_error(&e.to_string()))
    }
}

pub async fn run(
    connection: Connection,
    event_queue: EventQueue<AppState>,
    mut app_state: AppState,
) -> Result<(), Box<dyn StdError>> {
    let mut io = IoHandler::new();

    let (request_tx, mut request_rx) = channel::<BackendRequest>(32);
    let (sync_tx, sync_rx) = std::sync::mpsc::channel::<BackendRequest>();

    // ------------------------------------------------------------------
    // Dedicated OS thread for the synchronous wayland event loop
    // ------------------------------------------------------------------
    let mut watch_rx = app_state.enable_notify();
    let wayland = WaylandThread {
        event_queue,
        app_state,
        connection,
        request_rx: sync_rx,
    };
    let _wayland_thread = thread::spawn(move || wayland.run());

    // Task that emits state_change JSON-RPC notifications only when
    // dispatch.rs calls AppState::notify() — i.e. on actual state changes.
    let _notify_guard = TaskGuard::new(tokio::task::spawn(async move {
        while watch_rx.changed().await.is_ok() {
            if let Some(ref info) = *watch_rx.borrow() {
                let notification = serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "state_change",
                    "params": {
                        "state": info,
                    }
                });
                println!("{}", notification);
            }
        }
    }));

    // ------------------------------------------------------------------
    // Async bridge: forwards JSON-RPC requests to the wayland thread
    // ------------------------------------------------------------------
    let _bridge_guard = TaskGuard::new(tokio::task::spawn(async move {
        while let Some(request) = request_rx.recv().await {
            if sync_tx.send(request).is_err() {
                tracing::warn!("Wayland thread exited");
                break;
            }
        }
    }));

    let server_handler = Arc::new(ServerHandler::new(request_tx));

    io.add_method("info", {
        let handler = server_handler.clone();
        move |_params: Params| {
            handler
                .clone()
                .handle_request(BackendRequestParams::GetInfo)
                .boxed()
        }
    });

    io.add_method("move", {
        let handler = server_handler.clone();
        move |params: Params| {
            let handler = handler.clone();
            async move {
                let p: MoveParams = params.parse().map_err(|e| invalid_params(&e.to_string()))?;
                handler.handle_request(BackendRequestParams::Move(p)).await
            }
            .boxed()
        }
    });

    io.add_method("activate", {
        let handler = server_handler.clone();
        move |params: Params| {
            let handler = handler.clone();
            async move {
                let p: ActivateParams =
                    params.parse().map_err(|e| invalid_params(&e.to_string()))?;
                handler
                    .handle_request(BackendRequestParams::Activate(p))
                    .await
            }
            .boxed()
        }
    });

    io.add_method("state", {
        let handler = server_handler.clone();
        move |params: Params| {
            let handler = handler.clone();
            async move {
                let p: StateParams = params.parse().map_err(|e| invalid_params(&e.to_string()))?;
                handler.handle_request(BackendRequestParams::State(p)).await
            }
            .boxed()
        }
    });

    io.add_method("ws_activate", {
        let handler = server_handler.clone();
        move |params: Params| {
            let handler = handler.clone();
            async move {
                let p: ActivateWsParams =
                    params.parse().map_err(|e| invalid_params(&e.to_string()))?;
                handler
                    .handle_request(BackendRequestParams::ActivateWs(p))
                    .await
            }
            .boxed()
        }
    });

    io.add_method("close", {
        let handler = server_handler.clone();
        move |params: Params| {
            let handler = handler.clone();
            async move {
                let p: CloseParams = params.parse().map_err(|e| invalid_params(&e.to_string()))?;
                handler.handle_request(BackendRequestParams::Close(p)).await
            }
            .boxed()
        }
    });

    let server = jsonrpc_stdio_server::ServerBuilder::new(io).build();
    server.await;

    Ok(())
}

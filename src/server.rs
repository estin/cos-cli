use futures::FutureExt;
use jsonrpc_stdio_server::jsonrpc_core::{Error, ErrorCode, IoHandler, Params};
use serde::Deserialize;
use std::error::Error as StdError;
use std::sync::Arc;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;

use crate::{AppState, JsonInfo};

use wayland_client::{Connection, DispatchError, EventQueue};

pub struct Backend {
    pub connection: Connection,
    pub event_queue: EventQueue<AppState>,
    pub app_state: AppState,
}

#[derive(Debug, Deserialize)]
pub struct MoveParams {
    #[serde(default)]
    pub app_id: Option<String>,
    #[serde(default)]
    pub index: Option<usize>,
    pub workspace: String,
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

pub enum BackendRequestParams {
    GetInfo,
    Move(MoveParams),
    Activate(ActivateParams),
    ActivateWs(ActivateWsParams),
    State(StateParams),
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

fn error_response(message: &str) -> Error {
    Error {
        code: ErrorCode::InternalError,
        message: message.to_string(),
        data: None,
    }
}

impl Backend {
    fn start(
        mut self,
    ) -> Result<
        (
            tokio::task::JoinHandle<Result<(), DispatchError>>,
            UnboundedSender<BackendRequest>,
        ),
        Box<dyn StdError>,
    > {
        let (request_tx, mut request_rx) = unbounded_channel::<BackendRequest>();

        let handle = tokio::task::spawn(async move {
            let sleep = tokio::time::Duration::from_millis(300);
            loop {
                tokio::select! {
                    Some(request) = request_rx.recv() => {
                        match request.params {
                            BackendRequestParams::GetInfo => {
                                let _ = request.response_tx.send(BackendResponse::Info(JsonInfo::from(&self.app_state)));
                            }
                            BackendRequestParams::Move(params) => {
                                match self.handle_move(params).await {
                                    Ok(msg) => {
                                        let _ = request.response_tx.send(BackendResponse::Ok(msg));
                                    }
                                    Err(e) => {
                                        let _ = request.response_tx.send(BackendResponse::Err(e.to_string()));
                                    }
                                }
                            }
                            BackendRequestParams::Activate(params) => {
                                match self.handle_activate(params).await {
                                    Ok(msg) => {
                                        let _ = request.response_tx.send(BackendResponse::Ok(msg));
                                    }
                                    Err(e) => {
                                        let _ = request.response_tx.send(BackendResponse::Err(e.to_string()));
                                    }
                                }
                            }
                            BackendRequestParams::ActivateWs(params) => {
                                match self.handle_activate_ws(params).await {
                                    Ok(msg) => {
                                        let _ = request.response_tx.send(BackendResponse::Ok(msg));
                                    }
                                    Err(e) => {
                                        let _ = request.response_tx.send(BackendResponse::Err(e.to_string()));
                                    }
                                }
                            }
                            BackendRequestParams::State(params) => {
                                match self.handle_state(params).await {
                                    Ok(msg) => {
                                        let _ = request.response_tx.send(BackendResponse::Ok(msg));
                                    }
                                    Err(e) => {
                                        let _ = request.response_tx.send(BackendResponse::Err(e.to_string()));
                                    }
                                }
                            }
                        }
                    }
                    _ = tokio::time::sleep(sleep) => {
                        self.event_queue.roundtrip(&mut self.app_state)?;
                        self.connection.flush()?;
                    }
                }
            }
        });
        Ok((handle, request_tx))
    }

    async fn handle_move(&mut self, params: MoveParams) -> Result<String, Box<dyn StdError>> {
        let apps = self
            .find_apps(params.app_id.clone(), params.index, params.wait)
            .await?;

        let Some(manager) = &self.app_state.cosmic_toplevel_manager.clone() else {
            return Err("Compositor does not support workspace management protocol.".into());
        };

        let workspace_index = if let Some(group_index) = params.workspace_group {
            let group = self
                .app_state
                .workspace_groups
                .get(group_index)
                .ok_or_else(|| format!("Workspace group not found: {}", group_index))?;
            let idx = params
                .workspace
                .parse::<usize>()
                .map_err(|_| format!("Invalid workspace index: {}", params.workspace))?;
            if idx >= group.workspaces.len() {
                return Err(format!(
                    "Workspace index {} out of range (group has {} workspaces)",
                    idx,
                    group.workspaces.len()
                )
                .into());
            }
            (group_index, idx)
        } else {
            let mut found = None;
            for (gi, group) in self.app_state.workspace_groups.iter().enumerate() {
                for (wi, _) in group.workspaces.iter().enumerate() {
                    if format!("{}", wi) == params.workspace {
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

    async fn handle_activate(
        &mut self,
        params: ActivateParams,
    ) -> Result<String, Box<dyn StdError>> {
        let Some(manager) = &self.app_state.cosmic_toplevel_manager.clone() else {
            return Err("Compositor does not support toplevel management protocol.".into());
        };

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

    async fn handle_activate_ws(
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

    async fn handle_state(&mut self, params: StateParams) -> Result<String, Box<dyn StdError>> {
        let apps = self
            .find_apps(params.app_id.clone(), params.index, params.wait)
            .await?;

        let Some(manager) = &self.app_state.cosmic_toplevel_manager.clone() else {
            return Err("Compositor does not support toplevel management protocol.".into());
        };

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

    async fn find_apps(
        &mut self,
        app_id: Option<String>,
        app_index: Option<usize>,
        wait: Option<u64>,
    ) -> Result<Vec<crate::App>, Box<dyn StdError>> {
        if let Some(index) = app_index {
            if let Some(app) = self.app_state.apps.get(index) {
                Ok(vec![app.clone()])
            } else {
                Err(format!("App index not found: {}", index).into())
            }
        } else if let Some(id) = app_id {
            let sleep = tokio::time::Duration::from_millis(500);
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
                    tokio::time::sleep(sleep).await;
                    // TODO use async methods
                    self.event_queue.roundtrip(&mut self.app_state)?;
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

struct ServerHandler {
    tx: UnboundedSender<BackendRequest>,
}

impl ServerHandler {
    fn new(tx: UnboundedSender<BackendRequest>) -> Self {
        Self { tx }
    }

    async fn handle_request(
        self: Arc<Self>,
        request_params: BackendRequestParams,
    ) -> Result<serde_json::Value, Error> {
        let (response_tx, request) = BackendRequest::request(request_params);
        self.tx
            .send(request)
            .map_err(|e| error_response(&e.to_string()))?;
        let response = response_tx
            .await
            .map_err(|e| error_response(&e.to_string()))?;
        serde_json::to_value(response).map_err(|e| error_response(&e.to_string()))
    }
}

pub async fn run(mut backend: Backend) -> Result<(), Box<dyn StdError>> {
    let mut io = IoHandler::new();

    let mut watch_rx = backend.app_state.enable_notify();
    let (_handle, request_tx) = backend.start()?;
    let server_handler = Arc::new(ServerHandler::new(request_tx));

    let _handle = tokio::spawn(async move {
        while watch_rx.changed().await.is_ok() {
            let notification = serde_json::json!({
                "jsonrpc": "2.0",
                "method": "state_change",
                "params": {
                    "state": *watch_rx.borrow(),
                }
            });
            println!("{}", notification);
        }
    });

    let handler_info = server_handler.clone();
    io.add_method("info", move |_params: Params| {
        handler_info
            .clone()
            .handle_request(BackendRequestParams::GetInfo)
            .boxed()
    });

    let handler_move = server_handler.clone();
    io.add_method("move", move |params: Params| {
        let handler = handler_move.clone();
        async move {
            let move_params: MoveParams =
                params.parse().map_err(|e| error_response(&e.to_string()))?;
            handler
                .handle_request(BackendRequestParams::Move(move_params))
                .await
        }
        .boxed()
    });

    let handler_activate = server_handler.clone();
    io.add_method("activate", move |params: Params| {
        let handler = handler_activate.clone();
        async move {
            let activate_params: ActivateParams =
                params.parse().map_err(|e| error_response(&e.to_string()))?;
            handler
                .handle_request(BackendRequestParams::Activate(activate_params))
                .await
        }
        .boxed()
    });

    let handler_state = server_handler.clone();
    io.add_method("state", move |params: Params| {
        let handler = handler_state.clone();
        async move {
            let state_params: StateParams =
                params.parse().map_err(|e| error_response(&e.to_string()))?;
            handler
                .handle_request(BackendRequestParams::State(state_params))
                .await
        }
        .boxed()
    });

    let handler_ws = server_handler.clone();
    io.add_method("ws_activate", move |params: Params| {
        let handler = handler_ws.clone();
        async move {
            let ws_params: ActivateWsParams =
                params.parse().map_err(|e| error_response(&e.to_string()))?;
            handler
                .handle_request(BackendRequestParams::ActivateWs(ws_params))
                .await
        }
        .boxed()
    });

    let server = jsonrpc_stdio_server::ServerBuilder::new(io).build();
    server.await;

    Ok(())
}

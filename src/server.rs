use futures::FutureExt;
use jsonrpc_stdio_server::jsonrpc_core::{Error as JsonRpcError, IoHandler, Value};
use serde::Serialize;
use std::error::Error;
use std::sync::Arc;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;

use crate::AppState;

use wayland_client::{Connection, DispatchError, EventQueue};

pub struct Backend {
    pub connection: Connection,
    pub event_queue: EventQueue<AppState>,
    pub app_state: AppState,
}

pub enum BackendRequestParams {
    GetInfo,
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

#[derive(Serialize)]
#[serde(untagged)]
pub enum BackendResponse {
    Info(crate::JsonInfo),
}

impl Backend {
    fn start(
        mut self,
    ) -> Result<
        (
            tokio::task::JoinHandle<Result<(), DispatchError>>,
            UnboundedSender<BackendRequest>,
        ),
        Box<dyn Error>,
    > {
        let (request_tx, mut request_rx) = unbounded_channel::<BackendRequest>();

        let handle = tokio::task::spawn(async move {
            let sleep = tokio::time::Duration::from_millis(300);
            loop {
                tokio::select! {
                    Some(request) = request_rx.recv() => {
                        match request.params {
                            BackendRequestParams::GetInfo => {
                                let _ = request.response_tx.send(BackendResponse::Info(crate::JsonInfo::from(&self.app_state)));
                            }
                        }
                    }
                    _ = tokio::time::sleep(sleep) => {
                        // core::future::poll_fn(|cx| self.event_queue.poll_dispatch_pending(cx, &mut self.app_state)).await?;
                        // self.connection.flush()?;

                        // TODO use async methods!!!
                        // this code will block the whole executor
                        self.event_queue.roundtrip(&mut self.app_state)?;
                        self.connection.flush()?;
                    }
                }
            }
        });
        Ok((handle, request_tx))
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
    ) -> Result<serde_json::Value, JsonRpcError> {
        let (response_tx, request) = BackendRequest::request(request_params);
        self.tx.send(request).map_err(|e| JsonRpcError {
            message: e.to_string(),
            ..JsonRpcError::internal_error()
        })?;
        let response = response_tx.await.map_err(|e| JsonRpcError {
            message: e.to_string(),
            ..JsonRpcError::internal_error()
        })?;
        serde_json::to_value(response).map_err(|e| JsonRpcError {
            message: e.to_string(),
            ..JsonRpcError::internal_error()
        })
    }
}

pub async fn run(mut backend: Backend) -> Result<(), Box<dyn Error>> {
    let mut io = IoHandler::new();

    let mut watch_rx = backend.app_state.enable_notify();
    let (_handle, request_tx) = backend.start()?;
    let server_handler = Arc::new(ServerHandler::new(request_tx));

    let _handle = tokio::spawn(async move {
        while let Ok(_) = watch_rx.changed().await {
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

    io.add_method("info", move |_params| {
        server_handler
            .clone()
            .handle_request(BackendRequestParams::GetInfo)
            .boxed()
    });

    io.add_method("move", |_params| {
        async move { Ok(Value::String("move method not implemented yet".to_string())) }.boxed()
    });

    io.add_method("activate", |_params| {
        async move {
            Ok(Value::String(
                "activate method not implemented yet".to_string(),
            ))
        }
        .boxed()
    });

    io.add_method("state", |_params| {
        async move {
            Ok(Value::String(
                "state method not implemented yet".to_string(),
            ))
        }
        .boxed()
    });

    let server = jsonrpc_stdio_server::ServerBuilder::new(io).build();
    server.await;

    Ok(())
}

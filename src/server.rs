use futures::FutureExt;
use jsonrpc_stdio_server::jsonrpc_core::{IoHandler, Value};
use std::error::Error;

pub fn run() -> Result<(), Box<dyn Error>> {
    let mut io = IoHandler::new();

    io.add_method("info", |_params| {
        async move { Ok(Value::String("info method not implemented yet".to_string())) }.boxed()
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

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(server);
    Ok(())
}

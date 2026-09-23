use std::time::Duration;

use eggserve_primitives::{Response, ResponseBody, StatusCode};
use eggserve_server::{service_fn, Request, RuntimeConfig, Server, ShutdownResult};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

async fn get(stream: &mut TcpStream) {
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    let mut byte = [0; 1];
    while !response.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).await.unwrap();
        response.push(byte[0]);
    }
    assert!(response.starts_with(b"HTTP/1.1 200"));
}

#[tokio::test]
async fn leaf_crate_supervision_with_unlimited_keep_alive() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let runtime = RuntimeConfig::builder()
        .connection_total_timeout(Duration::from_millis(40))
        .disable_connection_total_timeout()
        .keep_alive_idle_timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let server = Server::builder()
        .runtime(runtime)
        .from_listener(listener)
        .build()
        .unwrap();
    let handle = server
        .start_with_service(service_fn(|_request: Request| async {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Empty)
                .unwrap())
        }))
        .await
        .unwrap();
    let (control, mut completion) = handle.into_parts();

    let mut client = TcpStream::connect(address).await.unwrap();
    get(&mut client).await;
    tokio::time::sleep(Duration::from_millis(80)).await;
    get(&mut client).await;
    drop(client);

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let _ = shutdown_tx.send(());
    let shutdown = async move {
        let _ = shutdown_rx.await;
    };
    tokio::pin!(shutdown);
    let result = tokio::select! {
        result = completion.wait() => result.unwrap(),
        _ = &mut shutdown => {
            control.shutdown();
            completion.wait().await.unwrap()
        }
    };
    assert_eq!(result, ShutdownResult::Clean);
    // The external supervisor owns the signal source independently of the
    // completion value, and requests shutdown without consuming it.
}

//! Downstream-neutral caller-owned embedding (Plan 215).
//!
//! Demonstrates the intended embedding boundary without
//! application-framework behavior:
//!
//! 1. the downstream accepts a TCP connection itself over loopback;
//! 2. it applies its own pre-HTTP admission policy (loopback-only here) and
//!    rejects before handoff by dropping the stream;
//! 3. it constructs explicit, truthful [`ConnectionContext`] metadata from
//!    observed socket addresses;
//! 4. it hands the surviving stream to
//!    [`serve_http1_connection`](eggserve_server::connection::serve_http1_connection)
//!    with a canonical service and one shared [`RuntimeState`];
//! 5. it requests graceful per-connection shutdown and inspects the
//!    [`ConnectionOutcome`].
//!
//! No Synvoid, WAF, routing, or backend concepts appear here: pre-accept
//! policy is an inline predicate owning its own decision. Runs one request
//! and exits; binds loopback only.

use std::net::SocketAddr;
use std::sync::Arc;

use eggserve_primitives::{
    canonical::{Response, ResponseBody, StatusCode},
    connection_info::Scheme,
};
use eggserve_server::connection::{serve_http1_connection, ConnectionContext, ConnectionShutdown};
use eggserve_server::{config::RuntimeConfig, runtime::RuntimeState, service_fn, Request};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Downstream-owned pre-HTTP admission: loopback peers only.
fn admit_peer(remote: SocketAddr) -> bool {
    remote.ip().is_loopback()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Downstream accepts the transport itself.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let bound = listener.local_addr()?;
    println!("downstream rendezvous on {bound}");

    let (done_tx, mut done_rx) = tokio::sync::oneshot::channel::<Vec<u8>>();
    let client = tokio::spawn(async move {
        let mut stream = tokio::net::TcpStream::connect(bound).await.unwrap();
        stream
            .write_all(b"GET /hello HTTP/1.1\r\nHost: example\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut body = Vec::new();
        stream.read_to_end(&mut body).await.unwrap();
        let _ = done_tx.send(body);
    });

    let (stream, remote) = listener.accept().await?;
    let local = stream.local_addr()?;

    // 2. Caller-owned admission before handoff; rejection drops the stream
    // before EggServe ever sees it.
    if !admit_peer(remote) {
        println!("rejected non-loopback peer {remote} before handoff");
        return Ok(());
    }

    // 3. Truthful transport metadata asserted by the transport owner.
    let context = ConnectionContext::for_tcp(local, remote, None);
    assert_eq!(context.scheme, Scheme::Http);
    println!("handing off {remote} -> {local}");

    // 4. One shared runtime drives the surviving stream.
    let config = Arc::new(RuntimeConfig::default());
    let state = Arc::new(RuntimeState::new(&config));
    let shutdown = ConnectionShutdown::new();
    let service = service_fn(|req: Request| async move {
        let path = req.head().target().path().to_owned();
        let text = format!("hello from {}\n", path);
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(text.into_bytes()))
            .unwrap())
    });

    // 5. Graceful per-connection shutdown once the client is done, then
    // inspect the classified outcome.
    let driver = serve_http1_connection(stream, service, config, context, state, &shutdown);
    tokio::pin!(driver);
    let (raw, outcome) = tokio::select! {
        outcome = &mut driver => {
            client.await.unwrap();
            let raw = done_rx.await.unwrap_or_default();
            (raw, outcome)
        }
        raw = &mut done_rx => {
            let raw = raw.unwrap_or_default();
            shutdown.shutdown();
            (raw, driver.await)
        }
    };
    println!(
        "connection outcome: {outcome} (clean: {})",
        outcome.is_clean()
    );
    let text = String::from_utf8_lossy(&raw);
    assert!(text.starts_with("HTTP/1.1 200 OK"), "got: {text}");
    assert!(text.ends_with("hello from /hello\n"), "got: {text}");
    assert!(outcome.is_clean());
    println!("done");
    Ok(())
}

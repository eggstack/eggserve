//! Plan 299 core compatibility parity: declared H1 trailers reach the wire
//! through the compatibility pipeline (no second H1 implementation).

use std::time::Duration;

use bytes::Bytes;
use eggserve_core::primitives::{Response, ResponseBody, StatusCode, TrailerDeclaration, Trailers};
use eggserve_core::server::{RuntimeConfig, Server};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

async fn start<S>(service: S) -> (std::net::SocketAddr, eggserve_core::server::ServerHandle)
where
    S: eggserve_core::server::Service,
{
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = Server::builder()
        .runtime(RuntimeConfig::builder().bind(addr).build().unwrap())
        .from_listener(listener)
        .build()
        .unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    (addr, handle)
}

#[tokio::test]
async fn core_compat_declared_trailers_reach_h1_wire() {
    let svc = eggserve_core::server::service_fn(|_req: eggserve_core::server::Request| async {
        let decl = TrailerDeclaration::from_names(vec!["x-end"]).unwrap();
        let items = vec![Ok::<_, eggserve_core::primitives::ResponseStreamError>(
            Bytes::from_static(b"trailer-body"),
        )];
        let trailer_future = async move {
            let mut block = eggserve_core::primitives::HeaderBlock::new();
            block.push_str("x-end", "yes").unwrap();
            Ok(Some(Trailers::new(block).unwrap()))
        };
        let stream = eggserve_core::primitives::ResponseStream::with_declared_trailers(
            futures_util::stream::iter(items),
            decl,
            trailer_future,
        );
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Stream(stream))
            .unwrap())
    });
    let (addr, handle) = start(svc).await;
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket
        .write_all(b"GET / HTTP/1.1\r\nHost: t\r\nTE: trailers\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut wire = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), socket.read_to_end(&mut wire))
        .await
        .unwrap()
        .unwrap();
    let text = String::from_utf8_lossy(&wire).to_ascii_lowercase();
    assert!(text.contains("200 ok"), "{text}");
    assert!(
        text.contains("trailer:") && text.contains("x-end"),
        "{text}"
    );
    assert!(!text.contains("content-length"), "{text}");
    assert!(text.contains("x-end: yes"), "{text}");
    handle.shutdown();
}

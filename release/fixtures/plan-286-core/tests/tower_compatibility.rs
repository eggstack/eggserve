#![cfg(feature = "tower")]

use bytes::Bytes;
use eggserve_core::primitives::interop::HttpRequestBody;
use eggserve_core::server::{RuntimeConfig, Server, TowerToEggserve};
use http_body_util::Full;
use std::future::Ready;
use std::task::{Context, Poll};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone)]
struct Hello;

impl tower_service::Service<http::Request<HttpRequestBody>> for Hello {
    type Response = http::Response<Full<Bytes>>;
    type Error = std::convert::Infallible;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: http::Request<HttpRequestBody>) -> Self::Future {
        std::future::ready(Ok(http::Response::new(Full::new(Bytes::from_static(
            b"core-tower",
        )))))
    }
}

#[tokio::test]
async fn historical_core_tower_forwarding_runs_from_registry_artifacts() {
    let runtime = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap();
    let server = Server::builder().runtime(runtime).build().unwrap();
    let handle = server
        .start_with_service(TowerToEggserve::new(Hello))
        .await
        .unwrap();
    let mut client = tokio::net::TcpStream::connect(handle.local_addr())
        .await
        .unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert!(response.windows(b"core-tower".len()).any(|w| w == b"core-tower"));
    handle.shutdown();
    let _ = handle.wait().await;
}

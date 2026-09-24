use eggserve_core::server::{RuntimeConfig, Server, StaticService};
use std::io::Write;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn compatibility_static_service_keeps_dotfiles_denied_by_default() {
    let root = tempfile::tempdir().unwrap();
    std::fs::File::create(root.path().join("visible.txt"))
        .unwrap()
        .write_all(b"visible")
        .unwrap();
    std::fs::File::create(root.path().join(".hidden"))
        .unwrap()
        .write_all(b"secret")
        .unwrap();
    let service = StaticService::builder(root.path()).build().unwrap();
    let runtime = RuntimeConfig::builder()
        .bind("127.0.0.1:0".parse().unwrap())
        .build()
        .unwrap();
    let server = Server::builder().runtime(runtime).build().unwrap();
    let handle = server.start_with_service(service).await.unwrap();
    let mut client = tokio::net::TcpStream::connect(handle.local_addr())
        .await
        .unwrap();
    client
        .write_all(b"GET /.hidden HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert!(response.starts_with(b"HTTP/1.1 403"), "{response:?}");
    assert!(!response.windows(b"secret".len()).any(|w| w == b"secret"));
    handle.shutdown();
    let _ = handle.wait().await;
}

//! Integration tests for the HTTP client primitive.
//!
//! These tests spin up local TCP servers and exercise the client against
//! them. No network access is required.

#![cfg(feature = "client")]

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use eggserve_core::primitives::client::{
    ClientConfig, ClientError, ClientRequestBuilder, HttpClient, Method,
};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::task;

async fn start_server<F, Fut>(handler: F) -> std::net::SocketAddr
where
    F: Fn(Request<Incoming>) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Response<Full<Bytes>>> + Send + 'static,
{
    let handler = Arc::new(handler);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    task::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let handler = Arc::clone(&handler);
            task::spawn(async move {
                let io = TokioIo::new(stream);
                let service = service_fn(move |req| {
                    let handler = Arc::clone(&handler);
                    async move { Ok::<_, Infallible>(handler(req).await) }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await;
            });
        }
    });

    addr
}

async fn test_handler(req: Request<Incoming>) -> Response<Full<Bytes>> {
    let response = match req.uri().path() {
        "/hello" => Response::builder()
            .status(200)
            .header("content-type", "text/plain")
            .body(Full::new(Bytes::from("hello world")))
            .unwrap(),
        "/head-only" => Response::builder()
            .status(200)
            .header("content-type", "text/plain")
            .body(Full::new(Bytes::from("head response")))
            .unwrap(),
        "/empty" => Response::builder()
            .status(204)
            .body(Full::new(Bytes::new()))
            .unwrap(),
        "/not-found" => Response::builder()
            .status(404)
            .body(Full::new(Bytes::from("not found")))
            .unwrap(),
        "/forbidden" => Response::builder()
            .status(403)
            .body(Full::new(Bytes::from("forbidden")))
            .unwrap(),
        "/echo-body" => {
            let body = req.into_body().collect().await.unwrap().to_bytes();
            Response::builder()
                .status(200)
                .header("content-type", "application/octet-stream")
                .body(Full::new(body))
                .unwrap()
        }
        "/large" => Response::builder()
            .status(200)
            .header("content-type", "text/plain")
            .body(Full::new(Bytes::from("x".repeat(1024 * 1024))))
            .unwrap(),
        "/slow" => {
            tokio::time::sleep(Duration::from_secs(2)).await;
            Response::builder()
                .status(200)
                .body(Full::new(Bytes::from("slow response")))
                .unwrap()
        }
        "/custom-headers" => Response::builder()
            .status(200)
            .header("x-request-id", "12345")
            .header("x-custom", "custom-value")
            .body(Full::new(Bytes::from("with headers")))
            .unwrap(),
        _ => Response::builder()
            .status(404)
            .body(Full::new(Bytes::from("not found")))
            .unwrap(),
    };

    response
}

#[test]
fn get_request_returns_200() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(test_handler));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/hello", addr))
        .unwrap()
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.text().unwrap(), "hello world");
}

#[test]
fn get_returns_content_type() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(test_handler));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/hello", addr))
        .unwrap()
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.content_type(), Some("text/plain"));
    assert_eq!(resp.content_length(), Some(11));
}

#[test]
fn head_request_returns_200_empty_body() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(test_handler));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Head)
        .url(&format!("http://{}/head-only", addr))
        .unwrap()
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.status, 200);
    assert!(resp.body.is_empty());
}

#[test]
fn post_echoes_body() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(test_handler));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Post)
        .url(&format!("http://{}/echo-body", addr))
        .unwrap()
        .body(b"test body content".to_vec())
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.bytes(), b"test body content");
}

#[test]
fn post_sends_content_length() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(test_handler));

    let client = HttpClient::with_defaults();
    let body = b"hello";
    let req = ClientRequestBuilder::new(Method::Post)
        .url(&format!("http://{}/echo-body", addr))
        .unwrap()
        .body(body.to_vec())
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.bytes(), body);
}

#[test]
fn put_replaces_resource() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(test_handler));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Put)
        .url(&format!("http://{}/echo-body", addr))
        .unwrap()
        .body(b"updated".to_vec())
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.bytes(), b"updated");
}

#[test]
fn delete_returns_200() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(test_handler));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Delete)
        .url(&format!("http://{}/hello", addr))
        .unwrap()
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.status, 200);
}

#[test]
fn patch_returns_200() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(test_handler));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Patch)
        .url(&format!("http://{}/echo-body", addr))
        .unwrap()
        .body(b"patched".to_vec())
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.status, 200);
}

#[test]
fn not_found_returns_404() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(test_handler));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/nonexistent", addr))
        .unwrap()
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.status, 404);
    assert!(!resp.is_success());
}

#[test]
fn forbidden_returns_403() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(test_handler));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/forbidden", addr))
        .unwrap()
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.status, 403);
    assert!(!resp.is_success());
}

#[test]
fn empty_response_204() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(test_handler));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/empty", addr))
        .unwrap()
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.status, 204);
    assert!(resp.body.is_empty());
}

#[test]
fn custom_headers_received() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(test_handler));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/custom-headers", addr))
        .unwrap()
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.headers.get("x-request-id").unwrap(), "12345");
    assert_eq!(resp.headers.get("x-custom").unwrap(), "custom-value");
}

#[test]
fn connect_timeout_on_unreachable_host() {
    let client = HttpClient::new(ClientConfig {
        connect_timeout: Duration::from_millis(100),
        request_timeout: Duration::from_secs(5),
        max_response_body_bytes: Some(1024),
        verify_tls: true,
    });

    let req = ClientRequestBuilder::new(Method::Get)
        .url("http://192.0.2.1:1/")
        .unwrap()
        .build()
        .unwrap();

    let result = client.send(&req);
    assert!(matches!(result, Err(ClientError::Timeout(_))));
}

#[test]
fn large_response_within_limit() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(test_handler));

    let client = HttpClient::new(ClientConfig {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(10),
        max_response_body_bytes: Some(2 * 1024 * 1024),
        verify_tls: true,
    });

    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/large", addr))
        .unwrap()
        .build()
        .unwrap();

    let resp = client.send(&req).unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.bytes().len(), 1024 * 1024);
}

#[test]
fn large_response_exceeds_limit() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(test_handler));

    let client = HttpClient::new(ClientConfig {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(10),
        max_response_body_bytes: Some(100),
        verify_tls: true,
    });

    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/large", addr))
        .unwrap()
        .build()
        .unwrap();

    let result = client.send(&req);
    assert!(matches!(
        result,
        Err(ClientError::ResponseBodyTooLarge { limit: 100 })
    ));
}

#[test]
fn user_agent_header_sent() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let captured = Arc::new(std::sync::Mutex::new(String::new()));
    let captured_clone = Arc::clone(&captured);

    let handler = move |req: Request<Incoming>| {
        let captured = Arc::clone(&captured_clone);
        async move {
            let ua = req
                .headers()
                .get("user-agent")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            *captured.lock().unwrap() = ua;
            Response::builder()
                .status(200)
                .body(Full::new(Bytes::new()))
                .unwrap()
        }
    };

    let addr = rt.block_on(start_server(handler));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .build()
        .unwrap();

    client.send(&req).unwrap();
    assert_eq!(*captured.lock().unwrap(), "eggserve-client/0.1");
}

#[test]
fn custom_user_agent_header() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let captured = Arc::new(std::sync::Mutex::new(String::new()));
    let captured_clone = Arc::clone(&captured);

    let handler = move |req: Request<Incoming>| {
        let captured = Arc::clone(&captured_clone);
        async move {
            let ua = req
                .headers()
                .get("user-agent")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            *captured.lock().unwrap() = ua;
            Response::builder()
                .status(200)
                .body(Full::new(Bytes::new()))
                .unwrap()
        }
    };

    let addr = rt.block_on(start_server(handler));

    let client = HttpClient::with_defaults();
    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/test", addr))
        .unwrap()
        .header("user-agent", "custom-agent/1.0")
        .unwrap()
        .build()
        .unwrap();

    client.send(&req).unwrap();
    assert_eq!(*captured.lock().unwrap(), "custom-agent/1.0");
}

#[test]
fn request_timeout_on_slow_server() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(test_handler));

    let client = HttpClient::new(ClientConfig {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_millis(500),
        max_response_body_bytes: Some(1024),
        verify_tls: true,
    });

    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/slow", addr))
        .unwrap()
        .build()
        .unwrap();

    let result = client.send(&req);
    assert!(matches!(result, Err(ClientError::Timeout(_))));
}

#[test]
fn connection_refused() {
    let client = HttpClient::with_defaults();

    let req = ClientRequestBuilder::new(Method::Get)
        .url("http://127.0.0.1:19/")
        .unwrap()
        .build()
        .unwrap();

    let result = client.send(&req);
    assert!(matches!(result, Err(ClientError::ConnectError(_))));
}

#[test]
fn unsupported_scheme_rejected() {
    let client = HttpClient::with_defaults();

    let result = ClientRequestBuilder::new(Method::Get).url("ftp://example.com/file");
    assert!(result.is_err());
    let _ = client; // ensure client is "used"
}

#[test]
fn get_with_body_rejected() {
    let result = ClientRequestBuilder::new(Method::Get)
        .url("http://example.com/")
        .unwrap()
        .body(b"body".to_vec())
        .build();
    assert!(result.is_err());
}

#[test]
fn head_with_body_rejected() {
    let result = ClientRequestBuilder::new(Method::Head)
        .url("http://example.com/")
        .unwrap()
        .body(b"body".to_vec())
        .build();
    assert!(result.is_err());
}

#[test]
fn missing_url_rejected() {
    let result = ClientRequestBuilder::new(Method::Get).build();
    assert!(result.is_err());
}

#[test]
fn server_disconnect_mid_body() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let addr = rt.block_on(start_server(test_handler));

    let client = HttpClient::new(ClientConfig {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        max_response_body_bytes: Some(10),
        verify_tls: true,
    });

    let req = ClientRequestBuilder::new(Method::Get)
        .url(&format!("http://{}/large", addr))
        .unwrap()
        .build()
        .unwrap();

    let result = client.send(&req);
    assert!(matches!(
        result,
        Err(ClientError::ResponseBodyTooLarge { limit: 10 })
    ));
}

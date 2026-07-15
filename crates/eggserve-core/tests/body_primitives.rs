//! Integration tests for request body primitives (Plan 056).

use eggserve_core::primitives::connection_info::{ConnectionInfo, Scheme};
use eggserve_core::primitives::header_block::HeaderBlock;
use eggserve_core::primitives::incomplete_body_policy::IncompleteBodyPolicy;
use eggserve_core::primitives::method::Method;
use eggserve_core::primitives::request::Request;
use eggserve_core::primitives::request_body::{BodyState, RequestBody};
use eggserve_core::primitives::request_body_error::RequestBodyError;
use eggserve_core::primitives::request_body_policy::RequestBodyPolicy;
use eggserve_core::primitives::request_head::RequestHead;
use eggserve_core::primitives::request_target::RequestTarget;
use eggserve_core::primitives::version::HttpVersion;
use futures_util::StreamExt;
use std::net::SocketAddr;
use std::time::Duration;

fn test_connection() -> ConnectionInfo {
    ConnectionInfo {
        local_addr: "127.0.0.1:8000".parse::<SocketAddr>().unwrap(),
        remote_addr: "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        scheme: Scheme::Http,
        tls: None,
    }
}

fn test_head(path: &str) -> RequestHead {
    RequestHead::new(
        Method::get(),
        RequestTarget::parse(path).unwrap(),
        HttpVersion::Http11,
        HeaderBlock::new(),
    )
}

fn test_request(path: &str) -> Request {
    Request::new(test_head(path), RequestBody::empty(), test_connection())
}

// ---------------------------------------------------------------------------
// RequestBodyPolicy tests
// ---------------------------------------------------------------------------

#[test]
fn policy_default_is_reject() {
    assert_eq!(RequestBodyPolicy::default(), RequestBodyPolicy::Reject);
}

#[test]
fn policy_reject_helpers() {
    let p = RequestBodyPolicy::Reject;
    assert!(p.is_reject());
    assert!(!p.allows_buffer());
    assert!(!p.allows_stream());
    assert_eq!(p.max_bytes(), None);
}

#[test]
fn policy_buffer_helpers() {
    let p = RequestBodyPolicy::Buffer { max_bytes: 1024 };
    assert!(!p.is_reject());
    assert!(p.allows_buffer());
    assert!(!p.allows_stream());
    assert_eq!(p.max_bytes(), Some(1024));
}

#[test]
fn policy_stream_helpers() {
    let p = RequestBodyPolicy::Stream { max_bytes: 4096 };
    assert!(!p.is_reject());
    assert!(!p.allows_buffer());
    assert!(p.allows_stream());
    assert_eq!(p.max_bytes(), Some(4096));
}

#[test]
fn policy_equality() {
    assert_eq!(
        RequestBodyPolicy::Buffer { max_bytes: 100 },
        RequestBodyPolicy::Buffer { max_bytes: 100 }
    );
    assert_ne!(
        RequestBodyPolicy::Buffer { max_bytes: 100 },
        RequestBodyPolicy::Buffer { max_bytes: 200 }
    );
    assert_ne!(
        RequestBodyPolicy::Buffer { max_bytes: 100 },
        RequestBodyPolicy::Stream { max_bytes: 100 }
    );
}

// ---------------------------------------------------------------------------
// RequestBody tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn empty_body_read_all() {
    let body = RequestBody::empty();
    assert_eq!(body.declared_length(), None);
    assert_eq!(body.bytes_received(), 0);
    assert_eq!(body.state(), BodyState::Unread);
    let data = body.read_all().await.unwrap();
    assert!(data.is_empty());
}

#[tokio::test]
async fn fixed_body_read_all() {
    let body = RequestBody::from_bytes(b"hello world".to_vec(), u64::MAX);
    assert_eq!(body.declared_length(), Some(11));
    assert_eq!(body.bytes_received(), 0);
    let data = body.read_all().await.unwrap();
    assert_eq!(&data[..], b"hello world");
}

#[tokio::test]
async fn fixed_body_chunked_read() {
    let mut body = RequestBody::from_bytes(b"abcde".to_vec(), u64::MAX);
    let chunk1 = body.next_chunk().await.unwrap().unwrap();
    assert!(!chunk1.is_empty());
    let mut all = chunk1.to_vec();
    while let Ok(Some(chunk)) = body.next_chunk().await {
        all.extend_from_slice(&chunk);
    }
    assert_eq!(&all[..], b"abcde");
}

#[tokio::test]
async fn body_limit_enforced_read_all() {
    let body = RequestBody::from_bytes(b"hello".to_vec(), 3);
    let err = body.read_all().await.unwrap_err();
    assert!(err.is_limit_exceeded());
    match err {
        RequestBodyError::LimitExceeded { limit, received } => {
            assert_eq!(limit, 3);
            assert_eq!(received, 5);
        }
        _ => panic!("expected LimitExceeded"),
    }
}

#[tokio::test]
async fn body_limit_enforced_streaming() {
    let mut body = RequestBody::from_bytes(b"hello".to_vec(), 3);
    // First chunk reads 5 bytes (the whole body since it's small)
    let result = body.next_chunk().await;
    // The body is 5 bytes but limit is 3, so it should fail
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.is_limit_exceeded());
}

#[tokio::test]
async fn zero_length_body() {
    let body = RequestBody::from_bytes(Vec::new(), u64::MAX);
    assert_eq!(body.declared_length(), Some(0));
    let data = body.read_all().await.unwrap();
    assert!(data.is_empty());
}

#[tokio::test]
async fn body_state_transitions() {
    // Use a body larger than 8192 (the internal chunk size) to ensure streaming
    let large_body = vec![0u8; 16384];
    let mut body = RequestBody::from_bytes(large_body, u64::MAX);
    assert_eq!(body.state(), BodyState::Unread);

    // First chunk transitions to Streaming
    let chunk = body.next_chunk().await.unwrap();
    assert!(chunk.is_some());
    assert_eq!(body.state(), BodyState::Streaming);

    // Exhaust the body
    while body.next_chunk().await.unwrap().is_some() {}
    assert_eq!(body.state(), BodyState::Complete);
}

#[tokio::test]
async fn stream_trait_implementation() {
    let body = RequestBody::from_bytes(b"stream test".to_vec(), u64::MAX);
    let mut stream = body;
    let mut all = Vec::new();
    while let Some(chunk) = stream.next().await {
        all.extend_from_slice(&chunk.unwrap());
    }
    assert_eq!(&all[..], b"stream test");
}

#[tokio::test]
async fn stream_trait_limit_enforcement() {
    let body = RequestBody::from_bytes(b"12345".to_vec(), 3);
    let mut stream = body;
    let mut received = 0;
    while let Some(result) = stream.next().await {
        match result {
            Ok(chunk) => received += chunk.len(),
            Err(RequestBodyError::LimitExceeded { .. }) => break,
            Err(e) => panic!("unexpected error: {e}"),
        }
    }
    assert!(received <= 3);
}

#[test]
fn body_debug_format() {
    let body = RequestBody::empty();
    let dbg = format!("{:?}", body);
    assert!(dbg.contains("RequestBody"));
    assert!(dbg.contains("Unread"));
}

#[test]
fn body_max_bytes_accessor() {
    let body = RequestBody::from_bytes(b"x".to_vec(), 1024);
    assert_eq!(body.max_bytes(), 1024);
}

// ---------------------------------------------------------------------------
// RequestBodyError tests
// ---------------------------------------------------------------------------

#[test]
fn error_display_all_variants() {
    let errors: Vec<RequestBodyError> = vec![
        RequestBodyError::RejectedByPolicy,
        RequestBodyError::DeclaredLengthTooLarge {
            declared: 1000,
            limit: 100,
        },
        RequestBodyError::LimitExceeded {
            limit: 100,
            received: 200,
        },
        RequestBodyError::ReadTimeout,
        RequestBodyError::PrematureEof {
            received: 5,
            expected: Some(10),
        },
        RequestBodyError::LengthMismatch {
            declared: 10,
            actual: 8,
        },
        RequestBodyError::InvalidChunkFraming("bad hex".into()),
        RequestBodyError::Cancelled,
        RequestBodyError::Disconnected,
        RequestBodyError::AlreadyConsumed,
        RequestBodyError::MixedConsumptionMode,
        RequestBodyError::Transport("reset".into()),
    ];
    for err in &errors {
        let s = err.to_string();
        assert!(!s.is_empty(), "empty display for {err:?}");
    }
}

#[test]
fn error_classifications() {
    assert!(RequestBodyError::RejectedByPolicy.is_policy_rejection());
    assert!(RequestBodyError::LimitExceeded {
        limit: 100,
        received: 200
    }
    .is_limit_exceeded());
    assert!(RequestBodyError::DeclaredLengthTooLarge {
        declared: 1000,
        limit: 100
    }
    .is_limit_exceeded());
    assert!(RequestBodyError::ReadTimeout.is_timeout());
    assert!(RequestBodyError::Disconnected.is_disconnect());
    assert!(RequestBodyError::PrematureEof {
        received: 0,
        expected: Some(10)
    }
    .is_disconnect());
    assert!(RequestBodyError::AlreadyConsumed.is_consumption_state());
    assert!(RequestBodyError::MixedConsumptionMode.is_consumption_state());
}

#[test]
fn error_status_codes() {
    assert_eq!(RequestBodyError::RejectedByPolicy.to_status_code(), 400);
    assert_eq!(
        RequestBodyError::LimitExceeded {
            limit: 100,
            received: 200
        }
        .to_status_code(),
        413
    );
    assert_eq!(
        RequestBodyError::DeclaredLengthTooLarge {
            declared: 1000,
            limit: 100
        }
        .to_status_code(),
        413
    );
    assert_eq!(RequestBodyError::ReadTimeout.to_status_code(), 408);
    assert_eq!(
        RequestBodyError::PrematureEof {
            received: 0,
            expected: Some(10)
        }
        .to_status_code(),
        400
    );
    assert_eq!(
        RequestBodyError::Transport("err".into()).to_status_code(),
        502
    );
    assert_eq!(RequestBodyError::AlreadyConsumed.to_status_code(), 500);
}

#[test]
fn error_is_std_error() {
    let err: &dyn std::error::Error = &RequestBodyError::RejectedByPolicy;
    assert!(!err.to_string().is_empty());
}

// ---------------------------------------------------------------------------
// IncompleteBodyPolicy tests
// ---------------------------------------------------------------------------

#[test]
fn incomplete_default_is_close() {
    assert_eq!(IncompleteBodyPolicy::default(), IncompleteBodyPolicy::Close);
}

#[test]
fn incomplete_drain_helpers() {
    let p = IncompleteBodyPolicy::Drain {
        max_bytes: 1024,
        timeout: Duration::from_secs(5),
    };
    assert!(p.is_drain());
    assert!(!p.is_close());
}

#[test]
fn incomplete_close_helpers() {
    let p = IncompleteBodyPolicy::Close;
    assert!(!p.is_drain());
    assert!(p.is_close());
}

// ---------------------------------------------------------------------------
// Request envelope tests
// ---------------------------------------------------------------------------

#[test]
fn request_construction_and_accessors() {
    let req = test_request("/foo/bar");
    assert_eq!(req.head().method().as_str(), "GET");
    assert_eq!(req.head().target().path(), "/foo/bar");
    assert_eq!(req.connection().scheme, Scheme::Http);
}

#[test]
fn request_into_parts() {
    let req = test_request("/test");
    let (head, body, conn) = req.into_parts();
    assert_eq!(head.method().as_str(), "GET");
    assert_eq!(conn.scheme, Scheme::Http);
    // body is empty — verify state
    assert_eq!(body.state(), BodyState::Unread);
}

#[test]
fn request_into_body() {
    let req = test_request("/test");
    let body = req.into_body();
    assert_eq!(body.state(), BodyState::Unread);
}

#[test]
fn request_into_head_and_body() {
    let req = test_request("/path");
    let (head, body) = req.into_head_and_body();
    assert_eq!(head.target().path(), "/path");
    assert_eq!(body.state(), BodyState::Unread);
}

#[test]
fn request_with_body() {
    let body = RequestBody::from_bytes(b"payload".to_vec(), u64::MAX);
    let req = Request::new(test_head("/upload"), body, test_connection());
    assert_eq!(req.head().target().path(), "/upload");
    assert_eq!(req.body().declared_length(), Some(7));
}

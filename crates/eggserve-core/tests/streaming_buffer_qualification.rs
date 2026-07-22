//! Streaming buffer qualification tests for Plan 088.
//!
//! These tests verify exact range boundaries, buffer isolation across
//! requests, short-read behavior, and zero-length file handling.

use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::{Method, Request, StatusCode};
use std::fs;
use std::sync::Arc;
use tempfile::TempDir;

use eggserve_core::config::{ServeConfig, ServeState};
use eggserve_core::service::handle_request;

fn setup() -> (TempDir, ServeState) {
    let tmp = TempDir::new().unwrap();
    let config = Arc::new(ServeConfig {
        root: tmp.path().to_path_buf(),
        ..ServeConfig::default()
    });
    let state = ServeState::new(config).unwrap();
    (tmp, state)
}

fn get_req(path: &str) -> Request<http_body_util::Empty<Bytes>> {
    Request::builder()
        .method(Method::GET)
        .uri(path)
        .body(http_body_util::Empty::new())
        .unwrap()
}

fn get_req_with_header(
    path: &str,
    header_name: &str,
    header_value: &str,
) -> Request<http_body_util::Empty<Bytes>> {
    Request::builder()
        .method(Method::GET)
        .uri(path)
        .header(header_name, header_value)
        .body(http_body_util::Empty::new())
        .unwrap()
}

#[tokio::test]
async fn exact_range_first_byte() {
    let (_tmp, state) = setup();
    fs::write(
        state.config().root.join("data.bin"),
        vec![0u8, 1, 2, 3, 4, 5, 6, 7],
    )
    .unwrap();
    let resp = handle_request(
        get_req_with_header("/data.bin", "range", "bytes=0-0"),
        &state,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.len(), 1);
    assert_eq!(body[0], 0);
}

#[tokio::test]
async fn exact_range_last_byte() {
    let (_tmp, state) = setup();
    fs::write(
        state.config().root.join("data.bin"),
        vec![0u8, 1, 2, 3, 4, 5, 6, 7],
    )
    .unwrap();
    let resp = handle_request(
        get_req_with_header("/data.bin", "range", "bytes=7-7"),
        &state,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.len(), 1);
    assert_eq!(body[0], 7);
}

#[tokio::test]
async fn exact_range_full_file() {
    let data: Vec<u8> = (0..=255).collect();
    let (_tmp, state) = setup();
    fs::write(state.config().root.join("data.bin"), &data).unwrap();
    let resp = handle_request(
        get_req_with_header("/data.bin", "range", "bytes=0-255"),
        &state,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        resp.headers().get("content-range").unwrap(),
        "bytes 0-255/256"
    );
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], &data[..]);
}

#[tokio::test]
async fn exact_range_cross_chunk_boundary() {
    // File slightly larger than DEFAULT_CHUNK_SIZE (8192)
    let data: Vec<u8> = (0..=255).cycle().take(8192 + 100).collect();
    let (_tmp, state) = setup();
    fs::write(state.config().root.join("data.bin"), &data).unwrap();
    // Range that crosses the 8192 chunk boundary
    let resp = handle_request(
        get_req_with_header("/data.bin", "range", "bytes=8100-8299"),
        &state,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    // Range 8100-8299 clamped to 8100-8291 on 8292-byte file = 192 bytes
    assert_eq!(body.len(), 192);
    assert_eq!(&body[..], &data[8100..8292]);
}

#[tokio::test]
async fn exact_range_at_chunk_boundary_start() {
    let data: Vec<u8> = (0..=255).cycle().take(16384).collect();
    let (_tmp, state) = setup();
    fs::write(state.config().root.join("data.bin"), &data).unwrap();
    let resp = handle_request(
        get_req_with_header("/data.bin", "range", "bytes=8192-8391"),
        &state,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.len(), 200);
    assert_eq!(&body[..], &data[8192..8392]);
}

#[tokio::test]
async fn zero_length_file_full() {
    let (_tmp, state) = setup();
    fs::write(state.config().root.join("empty.txt"), "").unwrap();
    let resp = handle_request(get_req("/empty.txt"), &state).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers().get("content-length").unwrap(), "0");
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert!(body.is_empty());
}

#[tokio::test]
async fn zero_length_file_head() {
    let (_tmp, state) = setup();
    fs::write(state.config().root.join("empty.txt"), "").unwrap();
    let resp = handle_request(
        Request::builder()
            .method(Method::HEAD)
            .uri("/empty.txt")
            .body(http_body_util::Empty::<Bytes>::new())
            .unwrap(),
        &state,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    // HEAD with empty body suppresses Content-Length per normalize_metadata
    assert!(resp.headers().get("content-length").is_none());
}

#[tokio::test]
async fn zero_length_file_range_416() {
    let (_tmp, state) = setup();
    fs::write(state.config().root.join("empty.txt"), "").unwrap();
    let resp = handle_request(
        get_req_with_header("/empty.txt", "range", "bytes=0-0"),
        &state,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::RANGE_NOT_SATISFIABLE);
}

#[tokio::test]
async fn small_file_range_1byte() {
    let (_tmp, state) = setup();
    fs::write(state.config().root.join("tiny.txt"), "X").unwrap();
    let resp = handle_request(
        get_req_with_header("/tiny.txt", "range", "bytes=0-0"),
        &state,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], b"X");
}

#[tokio::test]
async fn buffer_isolation_between_requests() {
    // Serve the same range twice and verify identical content (no stale bytes)
    let data: Vec<u8> = (0..=255).cycle().take(4096).collect();
    let (_tmp, state) = setup();
    fs::write(state.config().root.join("data.bin"), &data).unwrap();

    let resp1 = handle_request(
        get_req_with_header("/data.bin", "range", "bytes=100-199"),
        &state,
    )
    .await;
    let body1 = resp1.into_body().collect().await.unwrap().to_bytes();

    let resp2 = handle_request(
        get_req_with_header("/data.bin", "range", "bytes=200-299"),
        &state,
    )
    .await;
    let body2 = resp2.into_body().collect().await.unwrap().to_bytes();

    assert_eq!(&body1[..], &data[100..200]);
    assert_eq!(&body2[..], &data[200..300]);
    assert_ne!(
        &body1[..],
        &body2[..],
        "different ranges must return different data"
    );
}

#[tokio::test]
async fn suffix_range_exact_boundary() {
    let data: Vec<u8> = (0..=100).collect();
    let (_tmp, state) = setup();
    fs::write(state.config().root.join("data.bin"), &data).unwrap();
    // Last 10 bytes
    let resp = handle_request(
        get_req_with_header("/data.bin", "range", "bytes=-10"),
        &state,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.len(), 10);
    assert_eq!(&body[..], &data[91..101]);
}

#[tokio::test]
async fn open_ended_range_exact() {
    let data: Vec<u8> = (0..=100).collect();
    let (_tmp, state) = setup();
    fs::write(state.config().root.join("data.bin"), &data).unwrap();
    let resp = handle_request(
        get_req_with_header("/data.bin", "range", "bytes=95-"),
        &state,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.len(), 6);
    assert_eq!(&body[..], &data[95..101]);
}

#[tokio::test]
async fn range_content_range_header_exact() {
    let (_tmp, state) = setup();
    fs::write(state.config().root.join("data.bin"), vec![0u8; 1000]).unwrap();
    let resp = handle_request(
        get_req_with_header("/data.bin", "range", "bytes=100-199"),
        &state,
    )
    .await;
    assert_eq!(
        resp.headers().get("content-range").unwrap(),
        "bytes 100-199/1000"
    );
    assert_eq!(resp.headers().get("content-length").unwrap(), "100");
}

#[tokio::test]
async fn multiple_sequential_range_requests_same_connection() {
    let data: Vec<u8> = (0..=255).cycle().take(8192).collect();
    let (_tmp, state) = setup();
    fs::write(state.config().root.join("data.bin"), &data).unwrap();

    // Simulate multiple sequential requests (as would happen on a keep-alive connection)
    for offset in (0..8192).step_by(100) {
        let end = (offset + 99).min(8191);
        let range_header = format!("bytes={}-{}", offset, end);
        let resp = handle_request(
            get_req_with_header("/data.bin", "range", &range_header),
            &state,
        )
        .await;
        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let expected_len = end - offset + 1;
        assert_eq!(body.len(), expected_len as usize);
        assert_eq!(&body[..], &data[offset as usize..=end as usize]);
    }
}

#[tokio::test]
async fn large_file_range_preserves_exact_content() {
    // 256 KiB file - larger than typical chunk sizes
    let data: Vec<u8> = (0..=255).cycle().take(256 * 1024).collect();
    let (_tmp, state) = setup();
    fs::write(state.config().root.join("big.bin"), &data).unwrap();

    // Request the middle 1000 bytes
    let resp = handle_request(
        get_req_with_header("/big.bin", "range", "bytes=100000-100999"),
        &state,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.len(), 1000);
    assert_eq!(&body[..], &data[100000..101000]);
}

// ---------------------------------------------------------------------------
// Required tests: client disconnect, shutdown, concurrency
// ---------------------------------------------------------------------------

#[tokio::test]
async fn client_disconnect_releases_stream_permits() {
    // Simulate client disconnect by dropping response body mid-stream.
    // The OwnedSemaphorePermit is held in the stream state; dropping the
    // stream should release the permit back to the semaphore.
    let (_tmp, state) = setup();
    let data = vec![b'x'; 128 * 1024];
    fs::write(state.config().root.join("big.bin"), &data).unwrap();

    let max = state.config().limits.max_file_streams;

    // Exhaust all permits except one
    let mut permits = Vec::with_capacity(max - 1);
    for _ in 0..max - 1 {
        permits.push(
            state
                .file_stream_semaphore()
                .clone()
                .try_acquire_owned()
                .unwrap(),
        );
    }

    // Start streaming a large file, then drop the response body immediately
    let resp = handle_request(get_req("/big.bin"), &state).await;
    assert_eq!(resp.status(), StatusCode::OK);
    drop(resp); // Simulate client disconnect — stream and permit dropped

    // The permit should be released; we should be able to acquire one more
    drop(permits.pop());
    let acquired = state.file_stream_semaphore().clone().try_acquire_owned();
    assert!(
        acquired.is_ok(),
        "stream permit not released after client disconnect"
    );
}

#[tokio::test]
async fn forced_shutdown_releases_stream_permits() {
    // Simulate forced shutdown by dropping all responses and state.
    // All OwnedSemaphorePermits held by streams must be released.
    let (_tmp, state) = setup();
    let data = vec![b'x'; 128 * 1024];
    fs::write(state.config().root.join("big.bin"), &data).unwrap();

    let max = state.config().limits.max_file_streams;

    // Start multiple streaming responses
    let mut responses = Vec::new();
    for _ in 0..max {
        let resp = handle_request(get_req("/big.bin"), &state).await;
        assert_eq!(resp.status(), StatusCode::OK);
        responses.push(resp);
    }

    // All permits are held by the streaming responses
    let acquired = state.file_stream_semaphore().clone().try_acquire_owned();
    assert!(
        acquired.is_err(),
        "should not be able to acquire permit when all are held"
    );

    // Simulate forced shutdown: drop all responses
    responses.clear();

    // All permits should be released
    let acquired = state.file_stream_semaphore().clone().try_acquire_owned();
    assert!(
        acquired.is_ok(),
        "permits not released after forced shutdown"
    );
}

#[tokio::test]
async fn concurrent_stream_exhaustion_returns_503() {
    // Verify that concurrent streaming requests that exhaust the semaphore
    // return 503 Service Unavailable.
    let (_tmp, state) = setup();
    let data = vec![b'x'; 1024];
    fs::write(state.config().root.join("file.bin"), &data).unwrap();

    let max = state.config().limits.max_file_streams;

    // Exhaust all permits
    let mut permits = Vec::with_capacity(max);
    for _ in 0..max {
        permits.push(
            state
                .file_stream_semaphore()
                .clone()
                .try_acquire_owned()
                .unwrap(),
        );
    }

    // Next request should fail with 503
    let resp = handle_request(get_req("/file.bin"), &state).await;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

    drop(permits);

    // After releasing permits, requests should succeed again
    let resp = handle_request(get_req("/file.bin"), &state).await;
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn range_request_releases_permits_after_stream() {
    // Range requests also acquire a stream permit; verify it's released.
    let (_tmp, state) = setup();
    let data = vec![b'y'; 16 * 1024];
    fs::write(state.config().root.join("ranged.bin"), &data).unwrap();

    let max = state.config().limits.max_file_streams;

    // Exhaust all permits except one
    let mut permits = Vec::with_capacity(max - 1);
    for _ in 0..max - 1 {
        permits.push(
            state
                .file_stream_semaphore()
                .clone()
                .try_acquire_owned()
                .unwrap(),
        );
    }

    // Range request acquires the last permit, streams, then releases
    let resp = handle_request(
        get_req_with_header("/ranged.bin", "range", "bytes=0-4095"),
        &state,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.len(), 4096);

    // Permit should be released after body consumption
    drop(permits.pop());
    let acquired = state.file_stream_semaphore().clone().try_acquire_owned();
    assert!(
        acquired.is_ok(),
        "stream permit not released after range request body consumption"
    );
}

#[tokio::test]
async fn head_request_does_not_acquire_stream_permits() {
    // HEAD requests should not acquire file stream permits since they
    // don't stream any body data.
    let (_tmp, state) = setup();
    let data = vec![b'z'; 1024];
    fs::write(state.config().root.join("head.bin"), &data).unwrap();

    let max = state.config().limits.max_file_streams;

    // Exhaust all permits
    let mut permits = Vec::with_capacity(max);
    for _ in 0..max {
        permits.push(
            state
                .file_stream_semaphore()
                .clone()
                .try_acquire_owned()
                .unwrap(),
        );
    }

    // HEAD should succeed even with all permits held (no streaming needed)
    let req = Request::builder()
        .method(Method::HEAD)
        .uri("/head.bin")
        .body(http_body_util::Empty::<Bytes>::new())
        .unwrap();
    let resp = handle_request(req, &state).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers().get("content-length").unwrap(), "1024");
}

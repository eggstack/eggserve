//! Streaming buffer qualification tests for Plan 088.
//!
//! These tests verify exact range boundaries, buffer isolation across
//! requests, short-read behavior, and zero-length file handling.

use std::fs;
use tempfile::TempDir;

use eggserve_core::primitives::connection_info::{ConnectionInfo, Scheme};
use eggserve_core::primitives::header_block::HeaderBlock;
use eggserve_core::primitives::method::Method;
use eggserve_core::primitives::request::Request;
use eggserve_core::primitives::request_body::RequestBody;
use eggserve_core::primitives::request_head::RequestHead;
use eggserve_core::primitives::request_target::RequestTarget;
use eggserve_core::primitives::version::HttpVersion;
use eggserve_core::server::service::Service;
use eggserve_core::server::StaticService;
use std::net::SocketAddr;

#[allow(dead_code)]
fn extract_body_bytes_from_ref(
    body: &eggserve_core::primitives::canonical::ResponseBody,
) -> Vec<u8> {
    use eggserve_core::primitives::canonical::ResponseBody;
    match body {
        ResponseBody::Bytes(b) => b.clone(),
        ResponseBody::Empty | ResponseBody::EmptyWithLength(_) => vec![],
        ResponseBody::File(_) => vec![],
    }
}

// FIXME(extract_body_bytes): four duplicate helpers across integration tests; consolidated helper should replace these in a follow-up.
async fn extract_body_bytes(resp: &eggserve_core::primitives::canonical::Response) -> Vec<u8> {
    use eggserve_core::primitives::body::BodySource;
    use eggserve_core::primitives::canonical::ResponseBody;
    use std::io::{Read, Seek, SeekFrom};
    match resp.body() {
        Some(ResponseBody::Bytes(b)) => b.clone(),
        Some(ResponseBody::Empty) | Some(ResponseBody::EmptyWithLength(_)) => vec![],
        Some(ResponseBody::File(source)) => match source {
            BodySource::FileFull { file, len, .. } => {
                let mut buf = vec![0u8; *len as usize];
                let mut f = file.try_clone().expect("clone file handle");
                f.read_exact(&mut buf).expect("read full file");
                buf
            }
            BodySource::FileRange { file, range, .. } => {
                let len = (range.end_inclusive - range.start + 1) as usize;
                let mut buf = vec![0u8; len];
                let mut f = file.try_clone().expect("clone file handle");
                f.seek(SeekFrom::Start(range.start))
                    .expect("seek to range start");
                f.read_exact(&mut buf).expect("read range");
                buf
            }
            BodySource::Empty => vec![],
            BodySource::Bytes(b) => b.clone(),
        },
        None => vec![],
    }
}

fn setup() -> (TempDir, StaticService) {
    let tmp = TempDir::new().unwrap();
    let svc = StaticService::builder(tmp.path()).build().unwrap();
    (tmp, svc)
}

fn test_connection() -> ConnectionInfo {
    ConnectionInfo {
        local_addr: "127.0.0.1:8000".parse::<SocketAddr>().unwrap(),
        remote_addr: "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        scheme: Scheme::Http,
        tls: None,
    }
}

fn head_req(path: &str) -> Request {
    let target = RequestTarget::parse(path).unwrap();
    let head = RequestHead::new(
        Method::head(),
        target,
        HttpVersion::Http11,
        HeaderBlock::new(),
    );
    Request::new(head, RequestBody::empty(), test_connection())
}

fn get_req(path: &str) -> Request {
    let target = RequestTarget::parse(path).unwrap();
    let head = RequestHead::new(
        Method::get(),
        target,
        HttpVersion::Http11,
        HeaderBlock::new(),
    );
    Request::new(head, RequestBody::empty(), test_connection())
}

fn get_req_with_header(path: &str, header_name: &str, header_value: &str) -> Request {
    let target = RequestTarget::parse(path).unwrap();
    let mut headers = HeaderBlock::new();
    headers.push_str(header_name, header_value).unwrap();
    let head = RequestHead::new(Method::get(), target, HttpVersion::Http11, headers);
    Request::new(head, RequestBody::empty(), test_connection())
}

#[tokio::test]
async fn exact_range_first_byte() {
    let (_tmp, svc) = setup();
    fs::write(_tmp.path().join("data.bin"), vec![0u8, 1, 2, 3, 4, 5, 6, 7]).unwrap();
    let resp = svc
        .call(get_req_with_header("/data.bin", "range", "bytes=0-0"))
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 206);
    let body = extract_body_bytes(&resp).await;
    assert_eq!(body.len(), 1);
    assert_eq!(body[0], 0);
}

#[tokio::test]
async fn exact_range_last_byte() {
    let (_tmp, svc) = setup();
    fs::write(_tmp.path().join("data.bin"), vec![0u8, 1, 2, 3, 4, 5, 6, 7]).unwrap();
    let resp = svc
        .call(get_req_with_header("/data.bin", "range", "bytes=7-7"))
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 206);
    let body = extract_body_bytes(&resp).await;
    assert_eq!(body.len(), 1);
    assert_eq!(body[0], 7);
}

#[tokio::test]
async fn exact_range_full_file() {
    let data: Vec<u8> = (0..=255).collect();
    let (_tmp, svc) = setup();
    fs::write(_tmp.path().join("data.bin"), &data).unwrap();
    let resp = svc
        .call(get_req_with_header("/data.bin", "range", "bytes=0-255"))
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 206);
    assert_eq!(
        resp.headers().get_first("content-range").unwrap().as_str(),
        "bytes 0-255/256"
    );
    let body = extract_body_bytes(&resp).await;
    assert_eq!(&body[..], &data[..]);
}

#[tokio::test]
async fn exact_range_cross_chunk_boundary() {
    // File slightly larger than DEFAULT_CHUNK_SIZE (8192)
    let data: Vec<u8> = (0..=255).cycle().take(8192 + 100).collect();
    let (_tmp, svc) = setup();
    fs::write(_tmp.path().join("data.bin"), &data).unwrap();
    // Range that crosses the 8192 chunk boundary
    let resp = svc
        .call(get_req_with_header("/data.bin", "range", "bytes=8100-8299"))
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 206);
    let body = extract_body_bytes(&resp).await;
    // Range 8100-8299 clamped to 8100-8291 on 8292-byte file = 192 bytes
    assert_eq!(body.len(), 192);
    assert_eq!(&body[..], &data[8100..8292]);
}

#[tokio::test]
async fn exact_range_at_chunk_boundary_start() {
    let data: Vec<u8> = (0..=255).cycle().take(16384).collect();
    let (_tmp, svc) = setup();
    fs::write(_tmp.path().join("data.bin"), &data).unwrap();
    let resp = svc
        .call(get_req_with_header("/data.bin", "range", "bytes=8192-8391"))
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 206);
    let body = extract_body_bytes(&resp).await;
    assert_eq!(body.len(), 200);
    assert_eq!(&body[..], &data[8192..8392]);
}

#[tokio::test]
async fn zero_length_file_full() {
    let (_tmp, svc) = setup();
    fs::write(_tmp.path().join("empty.txt"), "").unwrap();
    let resp = svc.call(get_req("/empty.txt")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(
        resp.headers().get_first("content-length").unwrap().as_str(),
        "0"
    );
    let body = extract_body_bytes(&resp).await;
    assert!(body.is_empty());
}

#[tokio::test]
async fn zero_length_file_head() {
    let (_tmp, svc) = setup();
    fs::write(_tmp.path().join("empty.txt"), "").unwrap();
    let resp = svc.call(head_req("/empty.txt")).await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    // HEAD with empty body suppresses Content-Length per normalize_metadata
    assert!(resp.headers().get_first("content-length").is_none());
}

#[tokio::test]
async fn zero_length_file_range_416() {
    let (_tmp, svc) = setup();
    fs::write(_tmp.path().join("empty.txt"), "").unwrap();
    let resp = svc
        .call(get_req_with_header("/empty.txt", "range", "bytes=0-0"))
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 416);
}

#[tokio::test]
async fn small_file_range_1byte() {
    let (_tmp, svc) = setup();
    fs::write(_tmp.path().join("tiny.txt"), "X").unwrap();
    let resp = svc
        .call(get_req_with_header("/tiny.txt", "range", "bytes=0-0"))
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 206);
    let body = extract_body_bytes(&resp).await;
    assert_eq!(&body[..], b"X");
}

#[tokio::test]
async fn buffer_isolation_between_requests() {
    // Serve the same range twice and verify identical content (no stale bytes)
    let data: Vec<u8> = (0..=255).cycle().take(4096).collect();
    let (_tmp, svc) = setup();
    fs::write(_tmp.path().join("data.bin"), &data).unwrap();

    let resp1 = svc
        .call(get_req_with_header("/data.bin", "range", "bytes=100-199"))
        .await
        .unwrap();
    let body1 = extract_body_bytes(&resp1).await;

    let resp2 = svc
        .call(get_req_with_header("/data.bin", "range", "bytes=200-299"))
        .await
        .unwrap();
    let body2 = extract_body_bytes(&resp2).await;

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
    let (_tmp, svc) = setup();
    fs::write(_tmp.path().join("data.bin"), &data).unwrap();
    // Last 10 bytes
    let resp = svc
        .call(get_req_with_header("/data.bin", "range", "bytes=-10"))
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 206);
    let body = extract_body_bytes(&resp).await;
    assert_eq!(body.len(), 10);
    assert_eq!(&body[..], &data[91..101]);
}

#[tokio::test]
async fn open_ended_range_exact() {
    let data: Vec<u8> = (0..=100).collect();
    let (_tmp, svc) = setup();
    fs::write(_tmp.path().join("data.bin"), &data).unwrap();
    let resp = svc
        .call(get_req_with_header("/data.bin", "range", "bytes=95-"))
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 206);
    let body = extract_body_bytes(&resp).await;
    assert_eq!(body.len(), 6);
    assert_eq!(&body[..], &data[95..101]);
}

#[tokio::test]
async fn range_content_range_header_exact() {
    let (_tmp, svc) = setup();
    fs::write(_tmp.path().join("data.bin"), vec![0u8; 1000]).unwrap();
    let resp = svc
        .call(get_req_with_header("/data.bin", "range", "bytes=100-199"))
        .await
        .unwrap();
    assert_eq!(
        resp.headers().get_first("content-range").unwrap().as_str(),
        "bytes 100-199/1000"
    );
    assert_eq!(
        resp.headers().get_first("content-length").unwrap().as_str(),
        "100"
    );
}

#[tokio::test]
async fn multiple_sequential_range_requests_same_connection() {
    let data: Vec<u8> = (0..=255).cycle().take(8192).collect();
    let (_tmp, svc) = setup();
    fs::write(_tmp.path().join("data.bin"), &data).unwrap();

    // Simulate multiple sequential requests (as would happen on a keep-alive connection)
    for offset in (0..8192).step_by(100) {
        let end = (offset + 99).min(8191);
        let range_header = format!("bytes={}-{}", offset, end);
        let resp = svc
            .call(get_req_with_header("/data.bin", "range", &range_header))
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 206);
        let body = extract_body_bytes(&resp).await;
        let expected_len = end - offset + 1;
        assert_eq!(body.len(), expected_len as usize);
        assert_eq!(&body[..], &data[offset as usize..=end as usize]);
    }
}

#[tokio::test]
async fn large_file_range_preserves_exact_content() {
    // 256 KiB file - larger than typical chunk sizes
    let data: Vec<u8> = (0..=255).cycle().take(256 * 1024).collect();
    let (_tmp, svc) = setup();
    fs::write(_tmp.path().join("big.bin"), &data).unwrap();

    // Request the middle 1000 bytes
    let resp = svc
        .call(get_req_with_header(
            "/big.bin",
            "range",
            "bytes=100000-100999",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 206);
    let body = extract_body_bytes(&resp).await;
    assert_eq!(body.len(), 1000);
    assert_eq!(&body[..], &data[100000..101000]);
}

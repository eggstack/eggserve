//! Ignored Plan 227 in-process body/adapter measurements.
//!
//! This is deliberately an integration test so it exercises the public
//! canonical adapter and static authority without adding a production
//! benchmark dependency. It records frame counts and elapsed time; absolute
//! timings are evidence for the local machine, not CI gates.

use std::sync::Arc;
use std::time::Instant;

use bytes::Bytes;
use eggserve_core::layers::static_files::SecureRoot;
use eggserve_core::primitives::canonical::{
    to_hyper_response, Response, ResponseBody, ResponseStream, StatusCode,
};
use eggserve_core::primitives::http::ReadOnlyMethod;
use eggserve_core::primitives::StaticPolicy;
use futures_util::stream;
use http_body_util::BodyExt;
use tempfile::TempDir;

const FILE_SIZES: [usize; 4] = [1024, 128 * 1024, 1024 * 1024, 16 * 1024 * 1024];
const CHUNK_SIZES: [usize; 6] = [
    8 * 1024,
    16 * 1024,
    32 * 1024,
    64 * 1024,
    128 * 1024,
    256 * 1024,
];

async fn consume<B>(mut body: B) -> (usize, usize)
where
    B: http_body::Body<Data = Bytes, Error = std::io::Error> + Unpin,
{
    let mut bytes = 0;
    let mut frames = 0;
    while let Some(frame) = body.frame().await {
        let frame = frame.expect("benchmark body must be valid");
        if let Ok(data) = frame.into_data() {
            bytes += data.len();
            frames += 1;
        }
    }
    (bytes, frames)
}

fn response(body: ResponseBody) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .body(body)
        .expect("benchmark response is valid")
}

fn file_response(root: &SecureRoot, size: usize) -> Response {
    let resource = root
        .resolve_uri("/payload.bin")
        .expect("benchmark path is valid")
        .into_file()
        .expect("benchmark resource is a file");
    let body = if size == *FILE_SIZES.last().unwrap() {
        let plan = resource.plan_response(ReadOnlyMethod::Get, None, None, None, None, None, None);
        resource.into_body(&plan).expect("benchmark body is valid")
    } else {
        resource
            .into_range_body(0, size as u64 - 1)
            .expect("benchmark range is valid")
    };
    Response::builder()
        .status(StatusCode::OK)
        .body(ResponseBody::File(body))
        .expect("benchmark response is valid")
}

fn stream_response(size: usize, chunk: usize) -> Response {
    let body = stream::unfold(size, move |remaining| async move {
        if remaining == 0 {
            None
        } else {
            let len = remaining.min(chunk);
            Some((Ok(Bytes::from(vec![b'x'; len])), remaining - len))
        }
    });
    response(ResponseBody::Stream(ResponseStream::with_known_length(
        body,
        size as u64,
    )))
}

#[tokio::test]
#[ignore = "manual Plan 227 evidence; absolute timing is not a CI gate"]
async fn plan227_body_and_adapter_benchmark() -> Result<(), Box<dyn std::error::Error>> {
    let temp = TempDir::new()?;
    let file_bytes = vec![b'f'; *FILE_SIZES.last().unwrap()];
    std::fs::write(temp.path().join("payload.bin"), &file_bytes)?;
    let root = SecureRoot::new(temp.path(), StaticPolicy::safe_default())?;
    let semaphore = Arc::new(tokio::sync::Semaphore::new(1024));

    println!("{{\"plan\":\"227\",\"kind\":\"file_adapter\",\"records\":[");
    let mut first_record = true;
    for &size in &FILE_SIZES {
        for &chunk in &CHUNK_SIZES {
            let began = Instant::now();
            let body = file_response(&root, size);
            let response = eggserve_core::primitives::canonical::adapters::to_hyper_response_with_file_stream_semaphore_and_chunk_size(
                body,
                &semaphore,
                chunk,
                None,
            )?;
            let (bytes, frames) = consume(response.into_body()).await;
            assert_eq!(bytes, size);
            if !first_record {
                println!(",");
            }
            first_record = false;
            print!(
                "{{\"size\":{size},\"chunk\":{chunk},\"bytes\":{bytes},\"frames\":{frames},\"elapsed_ms\":{:.3}}}",
                began.elapsed().as_secs_f64() * 1000.0
            );
        }
    }
    println!("]}}");

    println!("{{\"plan\":\"227\",\"kind\":\"response_adapter\",\"records\":[");
    let mut first_record = true;
    for &(label, size) in &[
        ("bytes_1m", 1024 * 1024),
        ("known_stream_1m", 1024 * 1024),
        ("known_stream_16m", 16 * 1024 * 1024),
    ] {
        let began = Instant::now();
        let response = if label == "bytes_1m" {
            to_hyper_response(response(ResponseBody::Bytes(vec![b'x'; size])))?
        } else {
            to_hyper_response(stream_response(size, 8 * 1024))?
        };
        let (bytes, frames) = consume(response.into_body()).await;
        assert_eq!(bytes, size);
        if !first_record {
            println!(",");
        }
        first_record = false;
        print!(
            "{{\"workload\":\"{label}\",\"bytes\":{bytes},\"frames\":{frames},\"elapsed_ms\":{:.3}}}",
            began.elapsed().as_secs_f64() * 1000.0
        );
    }
    println!("]}}");

    let began = Instant::now();
    let mut planned = 0usize;
    for _ in 0..1000 {
        let resource = root
            .resolve_uri("/payload.bin")
            .expect("benchmark path is valid")
            .into_file()
            .expect("benchmark resource is a file");
        let _ = resource.plan_response(ReadOnlyMethod::Get, None, None, None, None, None, None);
        planned += 1;
    }
    println!(
        "{{\"plan\":\"227\",\"kind\":\"static_planning\",\"requests\":{planned},\"elapsed_ms\":{:.3}}}",
        began.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

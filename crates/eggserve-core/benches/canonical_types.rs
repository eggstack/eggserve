use criterion::{black_box, criterion_group, criterion_main, Criterion};
use eggserve_core::primitives::canonical::{
    normalize_response, NormalizeRequest, Response, ResponseBody, StatusCode,
};
use eggserve_core::primitives::header_block::{HeaderBlock, HeaderName, HeaderValue};
use eggserve_core::primitives::method::Method;
use eggserve_core::primitives::request_head::RequestHead;
use eggserve_core::primitives::request_target::RequestTarget;
use eggserve_core::primitives::version::HttpVersion;

fn bench_method_construction(c: &mut Criterion) {
    c.bench_function("method_get", |b| {
        b.iter(|| Method::new(black_box("GET")).unwrap())
    });
    c.bench_function("method_extension", |b| {
        b.iter(|| Method::new(black_box("PURGE")).unwrap())
    });
}

fn bench_header_block(c: &mut Criterion) {
    c.bench_function("header_block_push_10", |b| {
        b.iter(|| {
            let mut hb = HeaderBlock::new();
            for i in 0..10 {
                let name = HeaderName::new(format!("x-header-{}", i)).unwrap();
                let value = HeaderValue::new("value").unwrap();
                hb.push(name, value);
            }
            black_box(&hb);
        })
    });
    c.bench_function("header_block_get_first", |b| {
        let mut hb = HeaderBlock::new();
        hb.push_str("content-type", "text/html").unwrap();
        hb.push_str("accept", "application/json").unwrap();
        hb.push_str("x-custom", "value").unwrap();
        b.iter(|| black_box(hb.get_first(black_box("content-type"))))
    });
}

fn bench_request_head_construction(c: &mut Criterion) {
    c.bench_function("request_head_construction", |b| {
        b.iter(|| {
            let mut headers = HeaderBlock::new();
            headers.push_str("host", "example.com").unwrap();
            headers.push_str("accept", "text/html").unwrap();
            RequestHead::new(
                Method::get(),
                RequestTarget::parse(black_box("/index.html")).unwrap(),
                HttpVersion::Http11,
                headers,
            )
        })
    });
}

fn bench_response_normalization(c: &mut Criterion) {
    c.bench_function("normalize_200_with_body", |b| {
        b.iter(|| {
            let resp = Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/plain")
                .unwrap()
                .body(ResponseBody::Bytes(b"hello world".to_vec()))
                .unwrap();
            let req = NormalizeRequest::new(false);
            black_box(normalize_response(black_box(resp), &req).unwrap());
        })
    });
    c.bench_function("normalize_head_suppresses", |b| {
        b.iter(|| {
            let resp = Response::builder()
                .status(StatusCode::OK)
                .body(ResponseBody::Bytes(b"hello world".to_vec()))
                .unwrap();
            let req = NormalizeRequest::new(true);
            black_box(normalize_response(black_box(resp), &req).unwrap());
        })
    });
    c.bench_function("normalize_204_strips_body", |b| {
        b.iter(|| {
            let resp = Response::builder()
                .status(StatusCode::NO_CONTENT)
                .body(ResponseBody::Bytes(b"unexpected".to_vec()))
                .unwrap();
            let req = NormalizeRequest::new(false);
            black_box(normalize_response(black_box(resp), &req).unwrap());
        })
    });
}

fn bench_status_code(c: &mut Criterion) {
    c.bench_function("status_code_new_valid", |b| {
        b.iter(|| StatusCode::new(black_box(200)).unwrap())
    });
    c.bench_function("status_code_classification", |b| {
        let sc = StatusCode::new(200).unwrap();
        b.iter(|| {
            black_box(sc.is_informational());
            black_box(sc.is_success());
            black_box(sc.is_redirection());
            black_box(sc.is_client_error());
            black_box(sc.is_server_error());
            black_box(sc.permits_payload_body());
        })
    });
}

criterion_group!(
    benches,
    bench_method_construction,
    bench_header_block,
    bench_request_head_construction,
    bench_response_normalization,
    bench_status_code,
);
criterion_main!(benches);

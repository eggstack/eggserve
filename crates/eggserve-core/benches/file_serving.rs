use criterion::{black_box, criterion_group, criterion_main, Criterion};
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

fn make_svc(dir: &TempDir) -> StaticService {
    StaticService::builder(dir.path()).build().unwrap()
}

fn test_connection() -> ConnectionInfo {
    ConnectionInfo {
        local_addr: "127.0.0.1:8000".parse::<SocketAddr>().unwrap(),
        remote_addr: "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        scheme: Scheme::Http,
        tls: None,
    }
}

fn make_file(dir: &TempDir, name: &str, size: usize) {
    let data = vec![b'x'; size];
    fs::write(dir.path().join(name), &data).unwrap();
}

fn make_req(method: Method, path: &str) -> Request {
    let target = RequestTarget::parse(path).unwrap();
    let head = RequestHead::new(method, target, HttpVersion::Http11, HeaderBlock::new());
    Request::new(head, RequestBody::empty(), test_connection())
}

fn get_req(path: &str) -> Request {
    make_req(Method::get(), path)
}

fn head_req(path: &str) -> Request {
    make_req(Method::head(), path)
}

fn get_req_header(path: &str, hdr: &str, val: &str) -> Request {
    let target = RequestTarget::parse(path).unwrap();
    let mut headers = HeaderBlock::new();
    headers.push_str(hdr, val).unwrap();
    let head = RequestHead::new(Method::get(), target, HttpVersion::Http11, headers);
    Request::new(head, RequestBody::empty(), test_connection())
}

// ---------------------------------------------------------------------------
// Track A — File size matrix: GET full file
// ---------------------------------------------------------------------------

fn bench_get_file_sizes(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let runtime_state = eggserve_core::server::RuntimeState::new_for_testing(32);
    let mut group = c.benchmark_group("get_file_sizes");

    for &(label, size) in &[
        ("empty", 0),
        ("1k", 1024),
        ("16k", 16 * 1024),
        ("128k", 128 * 1024),
        ("1m", 1024 * 1024),
    ] {
        let tmp = TempDir::new().unwrap();
        make_file(&tmp, "file.bin", size);
        let svc = make_svc(&tmp);
        group.bench_function(label, |b| {
            b.iter(|| {
                let resp = rt.block_on(svc.call(get_req("/file.bin")));
                assert_eq!(resp.status().as_u16(), 200);
                black_box(resp);
            })
        });
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// Track A — HEAD requests
// ---------------------------------------------------------------------------

fn bench_head_file_sizes(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let runtime_state = eggserve_core::server::RuntimeState::new_for_testing(32);
    let mut group = c.benchmark_group("head_file_sizes");

    for &(label, size) in &[
        ("empty", 0),
        ("1k", 1024),
        ("16k", 16 * 1024),
        ("128k", 128 * 1024),
        ("1m", 1024 * 1024),
    ] {
        let tmp = TempDir::new().unwrap();
        make_file(&tmp, "file.bin", size);
        let svc = make_svc(&tmp);
        group.bench_function(label, |b| {
            b.iter(|| {
                let resp = rt.block_on(svc.call(head_req("/file.bin")));
                assert_eq!(resp.status().as_u16(), 200);
                black_box(resp);
            })
        });
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// Track A — Range requests
// ---------------------------------------------------------------------------

fn bench_range_requests(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let runtime_state = eggserve_core::server::RuntimeState::new_for_testing(32);
    let mut group = c.benchmark_group("range_requests");

    // 16 KiB file — chunk-crossing range
    let tmp16 = TempDir::new().unwrap();
    make_file(&tmp16, "file.bin", 16 * 1024);
    let state16 = make_state(&tmp16);

    group.bench_function("16k_first_byte", |b| {
        b.iter(|| {
            let resp = rt.block_on(svc.call(
                get_req_header("/file.bin", "range", "bytes=0-0"),
                black_box(&state16),
                &runtime_state,
            ));
            assert_eq!(resp.status().as_u16(), 206);
            black_box(resp);
        })
    });

    group.bench_function("16k_chunk_cross", |b| {
        b.iter(|| {
            let resp = rt.block_on(svc.call(
                get_req_header("/file.bin", "range", "bytes=4000-5000"),
                black_box(&state16),
                &runtime_state,
            ));
            assert_eq!(resp.status().as_u16(), 206);
            black_box(resp);
        })
    });

    group.bench_function("16k_full", |b| {
        b.iter(|| {
            let resp = rt.block_on(svc.call(
                get_req_header("/file.bin", "range", "bytes=0-16383"),
                black_box(&state16),
                &runtime_state,
            ));
            assert_eq!(resp.status().as_u16(), 206);
            black_box(resp);
        })
    });

    // 1 MiB file — large range
    let tmp1m = TempDir::new().unwrap();
    make_file(&tmp1m, "file.bin", 1024 * 1024);
    let state1m = make_state(&tmp1m);

    group.bench_function("1m_first_8k", |b| {
        b.iter(|| {
            let resp = rt.block_on(svc.call(
                get_req_header("/file.bin", "range", "bytes=0-8191"),
                black_box(&state1m),
                &runtime_state,
            ));
            assert_eq!(resp.status().as_u16(), 206);
            black_box(resp);
        })
    });

    group.bench_function("1m_suffix", |b| {
        b.iter(|| {
            let resp = rt.block_on(svc.call(
                get_req_header("/file.bin", "range", "bytes=-8192"),
                black_box(&state1m),
                &runtime_state,
            ));
            assert_eq!(resp.status().as_u16(), 206);
            black_box(resp);
        })
    });

    group.bench_function("1m_last_8k", |b| {
        b.iter(|| {
            let resp = rt.block_on(svc.call(
                get_req_header("/file.bin", "range", "bytes=1040384-1048575"),
                black_box(&state1m),
                &runtime_state,
            ));
            assert_eq!(resp.status().as_u16(), 206);
            black_box(resp);
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Track A — Conditional requests (304)
// ---------------------------------------------------------------------------

fn bench_conditional_requests(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let runtime_state = eggserve_core::server::RuntimeState::new_for_testing(32);

    let tmp = TempDir::new().unwrap();
    make_file(&tmp, "file.bin", 16 * 1024);
    let svc = make_svc(&tmp);

    // Get ETag from a real response
    let resp = rt.block_on(svc.call(get_req("/file.bin")));
    let etag = resp
        .headers()
        .get("etag")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    c.bench_function("conditional_304_if_none_match", |b| {
        b.iter(|| {
            let resp = rt.block_on(svc.call(
                get_req_header("/file.bin", "if-none-match", &etag),
                black_box(&state),
                &runtime_state,
            ));
            assert_eq!(resp.status().as_u16(), 304);
            black_box(resp);
        })
    });

    c.bench_function("conditional_get_if_modified_since", |b| {
        let lm = resp
            .headers()
            .get("last-modified")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        b.iter(|| {
            let resp = rt.block_on(svc.call(
                get_req_header("/file.bin", "if-modified-since", &lm),
                black_box(&state),
                &runtime_state,
            ));
            assert_eq!(resp.status().as_u16(), 304);
            black_box(resp);
        })
    });
}

// ---------------------------------------------------------------------------
// Track A — Error paths (404, 403, 405)
// ---------------------------------------------------------------------------

fn bench_error_paths(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let runtime_state = eggserve_core::server::RuntimeState::new_for_testing(32);

    let tmp = TempDir::new().unwrap();
    make_file(&tmp, "file.bin", 1024);
    let svc = make_svc(&tmp);

    c.bench_function("not_found_404", |b| {
        b.iter(|| {
            let resp = rt.block_on(svc.call(get_req("/nope.bin")));
            assert_eq!(resp.status().as_u16(), 404);
            black_box(resp);
        })
    });

    c.bench_function("forbidden_403_dotfile", |b| {
        fs::write(tmp.path().join(".env"), "secret").unwrap();
        b.iter(|| {
            let resp = rt.block_on(svc.call(get_req("/.env")));
            assert_eq!(resp.status().as_u16(), 403);
            black_box(resp);
        })
    });

    c.bench_function("method_not_allowed_405", |b| {
        b.iter(|| {
            let req = Request::builder()
                .method(Method::post())
                .uri("/file.bin")
                .body(RequestBody::empty())
                .unwrap();
            let resp = rt.block_on(svc.call(req)).unwrap();
            assert_eq!(resp.status().as_u16(), 405);
            black_box(resp);
        })
    });
}

// ---------------------------------------------------------------------------
// Track A — Directory index
// ---------------------------------------------------------------------------

fn bench_directory_index(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let runtime_state = eggserve_core::server::RuntimeState::new_for_testing(32);

    let tmp = TempDir::new().unwrap();
    fs::create_dir(tmp.path().join("subdir")).unwrap();
    fs::write(
        tmp.path().join("subdir").join("index.html"),
        "<html>hi</html>",
    )
    .unwrap();
    let svc = make_svc(&tmp);

    c.bench_function("directory_index_serves_index_html", |b| {
        b.iter(|| {
            let resp = rt.block_on(svc.call(get_req("/subdir")));
            assert_eq!(resp.status().as_u16(), 200);
            black_box(resp);
        })
    });
}

// ---------------------------------------------------------------------------
// Track A — Keep-alive sequences (sequential requests on same state)
// ---------------------------------------------------------------------------

fn bench_keepalive_sequential(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let runtime_state = eggserve_core::server::RuntimeState::new_for_testing(32);

    let tmp = TempDir::new().unwrap();
    make_file(&tmp, "a.txt", 1024);
    make_file(&tmp, "b.txt", 2048);
    make_file(&tmp, "c.txt", 4096);
    let svc = make_svc(&tmp);

    c.bench_function("sequential_3_requests", |b| {
        b.iter(|| {
            let r1 = rt.block_on(svc.call(get_req("/a.txt")));
            assert_eq!(r1.status(), 200);
            let r2 = rt.block_on(svc.call(get_req("/b.txt")));
            assert_eq!(r2.status(), 200);
            let r3 = rt.block_on(svc.call(get_req("/c.txt")));
            assert_eq!(r3.status(), 200);
            black_box((r1, r2, r3));
        })
    });
}

// ---------------------------------------------------------------------------
// Track A — Body consumption (verify streaming bodies are correct)
// ---------------------------------------------------------------------------

fn bench_body_consumption(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let runtime_state = eggserve_core::server::RuntimeState::new_for_testing(32);
    let mut group = c.benchmark_group("body_consumption");

    for &(label, size) in &[("1k", 1024), ("16k", 16 * 1024), ("128k", 128 * 1024)] {
        let tmp = TempDir::new().unwrap();
        make_file(&tmp, "file.bin", size);
        let svc = make_svc(&tmp);
        group.bench_function(label, |b| {
            b.iter(|| {
                let resp = rt.block_on(svc.call(get_req("/file.bin")));
                assert_eq!(resp.status().as_u16(), 200);
                let collected = rt.block_on(extract_body_bytes(&resp).collect()).unwrap();
                black_box(collected);
            })
        });
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// Track D — Chunk size comparison (indirect: measure different file sizes
// that stress different chunk-count regimes with DEFAULT_CHUNK_SIZE=8192)
// ---------------------------------------------------------------------------

fn bench_chunk_count_regimes(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let runtime_state = eggserve_core::server::RuntimeState::new_for_testing(32);
    let mut group = c.benchmark_group("chunk_count_regimes");

    // Exactly 1 chunk (8192 bytes)
    let tmp1 = TempDir::new().unwrap();
    make_file(&tmp1, "file.bin", 8192);
    let state1 = make_state(&tmp1);
    group.bench_function("exact_1_chunk", |b| {
        b.iter(|| {
            let resp =
                rt.block_on(svc.call(get_req("/file.bin"), black_box(&state1), &runtime_state));
            let collected = rt.block_on(extract_body_bytes(&resp).collect()).unwrap();
            black_box(collected);
        })
    });

    // Exactly 2 chunks (16384 bytes)
    let tmp2 = TempDir::new().unwrap();
    make_file(&tmp2, "file.bin", 16384);
    let state2 = make_state(&tmp2);
    group.bench_function("exact_2_chunks", |b| {
        b.iter(|| {
            let resp =
                rt.block_on(svc.call(get_req("/file.bin"), black_box(&state2), &runtime_state));
            let collected = rt.block_on(extract_body_bytes(&resp).collect()).unwrap();
            black_box(collected);
        })
    });

    // 129 chunks + 1 byte (129 * 8192 + 1 = 1056769)
    let tmp3 = TempDir::new().unwrap();
    make_file(&tmp3, "file.bin", 129 * 8192 + 1);
    let state3 = make_state(&tmp3);
    group.bench_function("129_chunks_plus_1", |b| {
        b.iter(|| {
            let resp =
                rt.block_on(svc.call(get_req("/file.bin"), black_box(&state3), &runtime_state));
            let collected = rt.block_on(extract_body_bytes(&resp).collect()).unwrap();
            black_box(collected);
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Track J — Directory listing bounds
// ---------------------------------------------------------------------------

fn bench_directory_listing(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let runtime_state = eggserve_core::server::RuntimeState::new_for_testing(32);
    let mut group = c.benchmark_group("directory_listing");

    for &count in &[0usize, 10, 100, 1000] {
        let tmp = TempDir::new().unwrap();
        let config = Arc::new(ServeConfig {
            root: tmp.path().to_path_buf(),
            static_policy: eggserve_core::policy::StaticPolicy {
                directory_listing: eggserve_core::policy::DirectoryListingPolicy::Enabled,
                ..eggserve_core::policy::StaticPolicy::safe_default()
            },
            ..ServeConfig::default()
        });
        let state = ServeState::new(config).unwrap();

        for i in 0..count {
            fs::write(
                tmp.path().join(format!("file_{:04}.txt", i)),
                format!("content {}", i),
            )
            .unwrap();
        }

        group.bench_with_input(
            criterion::BenchmarkId::from_parameter(count),
            &count,
            |b, _count| {
                b.iter(|| {
                    let resp = rt.block_on(svc.call(get_req("/")));
                    assert_eq!(resp.status().as_u16(), 200);
                    black_box(resp);
                })
            },
        );
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// Track A — HEAD with body suppression verification
// ---------------------------------------------------------------------------

fn bench_head_vs_get(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let runtime_state = eggserve_core::server::RuntimeState::new_for_testing(32);
    let mut group = c.benchmark_group("head_vs_get");

    let tmp = TempDir::new().unwrap();
    make_file(&tmp, "file.bin", 128 * 1024);
    let svc = make_svc(&tmp);

    group.bench_function("get_128k", |b| {
        b.iter(|| {
            let resp = rt.block_on(svc.call(get_req("/file.bin")));
            assert_eq!(resp.status().as_u16(), 200);
            black_box(resp);
        })
    });

    group.bench_function("head_128k", |b| {
        b.iter(|| {
            let resp = rt.block_on(svc.call(head_req("/file.bin")));
            assert_eq!(resp.status().as_u16(), 200);
            black_box(resp);
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_get_file_sizes,
    bench_head_file_sizes,
    bench_range_requests,
    bench_conditional_requests,
    bench_error_paths,
    bench_directory_index,
    bench_keepalive_sequential,
    bench_body_consumption,
    bench_chunk_count_regimes,
    bench_directory_listing,
    bench_head_vs_get,
);
criterion_main!(benches);

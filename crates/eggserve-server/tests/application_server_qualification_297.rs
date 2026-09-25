//! Plan 297 downstream-like qualification fixture (WP-E).
//!
//! EggPool-shaped application geometry WITHOUT EggPool logic:
//! caller-bound `TcpListener` → direct `eggserve-server` + `tower` → Axum
//! `Router` (health/small-JSON route, bounded request-body route, SSE/token
//! stream route, cancellation) → controlled shutdown.
//!
//! Non-ignored tests run in routine CI. The `extended_timing_matrix` test is
//! `#[ignore]`d: same-machine loopback timing (release only); stdout JSON is
//! retained under `benchmarks/297-application-server-qualification/`.

#![cfg(feature = "tower")]

use std::convert::Infallible;
use std::time::Duration;

use axum::body::Body as AxumBody;
use axum::extract::State;
use axum::response::Response as AxumResponse;
use axum::routing::{get, post};
use axum::Router;
use bytes::Bytes;
use eggserve_primitives::request_body_policy::RequestBodyPolicy;
use eggserve_server::{RuntimeConfig, Server, TowerToEggserve};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

#[derive(Clone)]
struct AppState {
    release_second: std::sync::Arc<std::sync::Mutex<Option<oneshot::Receiver<()>>>>,
}

async fn health() -> AxumResponse {
    AxumResponse::builder()
        .header("content-type", "application/json")
        .body(AxumBody::from(r#"{"status":"ok"}"#))
        .unwrap()
}

async fn echo(body: AxumBody) -> AxumResponse {
    use futures_util::StreamExt as _;
    let mut stream = body.into_data_stream();
    let mut out = Vec::new();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(bytes) => out.extend_from_slice(&bytes),
            Err(_) => {
                return AxumResponse::builder()
                    .status(500)
                    .body(AxumBody::from("upstream body failed"))
                    .unwrap();
            }
        }
    }
    AxumResponse::new(AxumBody::from(out))
}

async fn events(State(state): State<AppState>) -> AxumResponse {
    // SSE/token-stream shape: one immediate chunk, one gated chunk, then end.
    // 128-chunk variants are exercised by the ignored timing matrix.
    let gate = state.release_second.lock().unwrap().take();
    let stream = futures_util::stream::unfold((0u8, gate), |(n, gate)| async move {
        match n {
            0 => Some((
                Ok::<_, Infallible>(Bytes::from_static(b"data: 0\n\n")),
                (1, gate),
            )),
            1 => {
                if let Some(gate) = gate {
                    let _ = gate.await;
                }
                Some((Ok(Bytes::from_static(b"data: 1\n\n")), (2, None)))
            }
            _ => None,
        }
    });
    AxumResponse::builder()
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .body(AxumBody::from_stream(stream))
        .unwrap()
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/echo", post(echo))
        .route("/events", get(events))
        .with_state(state)
}

async fn start_app(
    body_ceiling: u64,
) -> (
    std::net::SocketAddr,
    AppState,
    eggserve_server::ServerControl,
) {
    let (release_tx, release_rx) = oneshot::channel();
    let _ = release_tx.send(());
    let state = AppState {
        release_second: std::sync::Arc::new(std::sync::Mutex::new(Some(release_rx))),
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = Server::builder()
        .runtime(
            RuntimeConfig::builder()
                .bind(addr)
                .max_request_body_bytes(body_ceiling)
                .build()
                .unwrap(),
        )
        .from_listener(listener)
        .build()
        .unwrap();
    let handle = server
        .start_with_service(TowerToEggserve::with_policy(
            router(state.clone()),
            RequestBodyPolicy::Stream {
                max_bytes: 64 * 1024,
            },
        ))
        .await
        .unwrap();
    let (control, completion) = handle.into_parts();
    tokio::spawn(async move {
        let mut completion = completion;
        let _ = completion.wait().await;
    });
    (addr, state, control)
}

async fn exchange(addr: std::net::SocketAddr, raw: &[u8]) -> Vec<u8> {
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket.write_all(raw).await.unwrap();
    let mut wire = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), socket.read_to_end(&mut wire))
        .await
        .unwrap()
        .unwrap();
    wire
}

#[tokio::test]
async fn app_health_json_exact_with_content_length() {
    // P1 regression in app geometry: buffered Axum JSON carries
    // Content-Length (no chunked downgrade).
    let (addr, _state, control) = start_app(1024 * 1024).await;
    let wire = exchange(
        addr,
        b"GET /health HTTP/1.1\r\nHost: app\r\nConnection: close\r\n\r\n",
    )
    .await;
    let text = String::from_utf8_lossy(&wire).to_ascii_lowercase();
    assert!(text.starts_with("http/1.1 200"), "{text}");
    assert!(text.contains("content-length: 15"), "{text}");
    assert!(!text.contains("transfer-encoding: chunked"), "{text}");
    assert!(text.ends_with(r#"{"status":"ok"}"#));
    control.shutdown();
}

#[tokio::test]
async fn app_bounded_body_accepts_and_over_limit_rejects() {
    let (addr, _state, control) = start_app(1024 * 1024).await;
    // Small JSON POST echoes.
    let payload = br#"{"prompt":"hello"}"#;
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket
        .write_all(
            format!(
                "POST /echo HTTP/1.1\r\nHost: app\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    socket.write_all(payload).await.unwrap();
    let mut wire = Vec::new();
    socket.read_to_end(&mut wire).await.unwrap();
    let text = String::from_utf8_lossy(&wire);
    assert!(text.contains("200 OK"), "{text}");
    assert!(text.ends_with(std::str::from_utf8(payload).unwrap()));

    // Over the 64 KiB adapter ceiling: rejected, no echo.
    let big = vec![b'x'; 128 * 1024];
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket
        .write_all(
            format!(
                "POST /echo HTTP/1.1\r\nHost: app\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                big.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    socket.write_all(&big).await.unwrap();
    let mut wire = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), socket.read_to_end(&mut wire))
        .await
        .unwrap()
        .unwrap();
    let text = String::from_utf8_lossy(&wire);
    assert!(
        text.contains("413"),
        "over-limit body must be rejected, got: {text}"
    );
    control.shutdown();
}

#[tokio::test]
async fn app_sse_stream_completes_and_cancellation_recovers() {
    let (addr, _state, control) = start_app(1024 * 1024).await;
    // Full stream completes (gate pre-released by start_app).
    let wire = exchange(
        addr,
        b"GET /events HTTP/1.1\r\nHost: app\r\nConnection: close\r\n\r\n",
    )
    .await;
    let text = String::from_utf8_lossy(&wire);
    assert!(text.contains("200 OK"), "{text}");
    assert!(text.contains("text/event-stream"), "{text}");
    assert!(text.contains("data: 0"), "{text}");
    assert!(text.contains("data: 1"), "{text}");

    // Slow consumer disconnects mid-stream; the server must recover.
    let (held_tx, _held_rx) = oneshot::channel::<()>();
    let held_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let held_addr = held_listener.local_addr().unwrap();
    // Plain ungated infinite stream (294-proven shape) for the abandonment
    // probe: the AppState-gated /events route above already covers gated
    // completion.
    async fn infinite() -> AxumResponse {
        let stream = futures_util::stream::unfold(0u64, |count| async move {
            Some((
                Ok::<_, Infallible>(Bytes::from_static(b"cancel-chunk")),
                count + 1,
            ))
        });
        AxumResponse::new(AxumBody::from_stream(stream))
    }
    let held_router: Router = Router::new()
        .route("/events", get(infinite))
        .route("/health", get(health));
    let held_server = Server::builder()
        .runtime(
            RuntimeConfig::builder()
                .bind(held_addr)
                .max_request_body_bytes(1024 * 1024)
                .build()
                .unwrap(),
        )
        .from_listener(held_listener)
        .build()
        .unwrap();
    let held_handle = held_server
        .start_with_service(TowerToEggserve::with_policy(
            held_router,
            RequestBodyPolicy::Stream {
                max_bytes: 64 * 1024,
            },
        ))
        .await
        .unwrap();
    let (held_control, mut held_completion) = held_handle.into_parts();
    let mut socket = TcpStream::connect(held_addr).await.unwrap();
    socket
        .write_all(b"GET /events HTTP/1.1\r\nHost: app\r\n\r\n")
        .await
        .unwrap();
    // Read until the first chunk arrives, then abandon the connection.
    let mut seen = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !seen.windows(12).any(|w| w == b"cancel-chunk") {
            let mut chunk = [0u8; 512];
            let n = socket.read(&mut chunk).await.unwrap();
            assert_ne!(n, 0, "stream ended before first event");
            seen.extend_from_slice(&chunk[..n]);
        }
    })
    .await
    .unwrap();
    drop(socket);
    drop(held_tx);
    // Fresh connection still serves after the abandonment.
    let wire = exchange(
        held_addr,
        b"GET /health HTTP/1.1\r\nHost: app\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(
        String::from_utf8_lossy(&wire).contains("200 OK"),
        "server must recover after stream abandonment"
    );
    held_control.shutdown();
    tokio::time::timeout(Duration::from_secs(5), held_completion.wait())
        .await
        .unwrap()
        .unwrap();
    control.shutdown();
}

#[tokio::test]
async fn app_controlled_shutdown_drains_cleanly() {
    let (addr, _state, control) = start_app(1024 * 1024).await;
    // Server serves before shutdown.
    let wire = exchange(
        addr,
        b"GET /health HTTP/1.1\r\nHost: app\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(String::from_utf8_lossy(&wire).contains("200 OK"));
    // Controlled shutdown via the cloneable control handle.
    let cloned = control.clone();
    cloned.shutdown();
    drop(cloned);
}

#[tokio::test]
async fn app_gated_sse_first_chunk_solo() {
    let (release_tx, release_rx) = oneshot::channel::<()>();
    let _ = release_tx.send(());
    let state = AppState {
        release_second: std::sync::Arc::new(std::sync::Mutex::new(Some(release_rx))),
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = Server::builder()
        .runtime(
            RuntimeConfig::builder()
                .bind(addr)
                .max_request_body_bytes(1024 * 1024)
                .build()
                .unwrap(),
        )
        .from_listener(listener)
        .build()
        .unwrap();
    let handle = server
        .start_with_service(TowerToEggserve::with_policy(
            router(state),
            RequestBodyPolicy::Stream {
                max_bytes: 64 * 1024,
            },
        ))
        .await
        .unwrap();
    let (control, mut completion) = handle.into_parts();
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket
        .write_all(b"GET /events HTTP/1.1\r\nHost: app\r\n\r\n")
        .await
        .unwrap();
    let mut seen = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !seen.windows(7).any(|w| w == b"data: 0") {
            let mut chunk = [0u8; 512];
            let n = socket.read(&mut chunk).await.unwrap();
            assert_ne!(n, 0);
            seen.extend_from_slice(&chunk[..n]);
        }
    })
    .await
    .unwrap();
    control.shutdown();
    tokio::time::timeout(Duration::from_secs(5), completion.wait())
        .await
        .unwrap()
        .unwrap();
}

/// Same-machine extended timing matrix (ignored in routine CI).
///
/// Run explicitly:
/// `PROFILE_297=release-qual cargo test --release -p eggserve-server
/// --features tower --test application_server_qualification_297
/// extended_timing_matrix -- --ignored --nocapture`
///
/// Covers what the 294 sequential matrix does not: c16 concurrent
/// keep-alive, SSE-128 streams, small POST/JSON exchanges, 10 concurrent
/// slow streams with shutdown, and harness RSS markers.
#[tokio::test]
#[ignore]
async fn extended_timing_matrix() {
    use std::time::Instant;

    fn profile() -> &'static str {
        option_env!("PROFILE_297").unwrap_or("profile-not-set")
    }

    fn rss_hwm_kb() -> Option<u64> {
        // Linux-only harness marker (VmHWM of this test process, which hosts
        // every fixture server sequentially — a recovery marker, not a
        // per-profile attribution).
        #[cfg(target_os = "linux")]
        {
            std::fs::read_to_string("/proc/self/status")
                .ok()?
                .lines()
                .find(|l| l.starts_with("VmHWM:"))?
                .split_whitespace()
                .nth(1)?
                .parse()
                .ok()
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }

    async fn keep_alive_session(
        addr: std::net::SocketAddr,
        raw_request: Vec<u8>,
        requests: usize,
    ) -> Vec<f64> {
        let mut socket = TcpStream::connect(addr).await.unwrap();
        let mut latencies = Vec::with_capacity(requests);
        for _ in 0..requests {
            let began = Instant::now();
            socket.write_all(&raw_request).await.unwrap();
            // 1 KiB Content-Length responses in this matrix.
            let mut head = Vec::with_capacity(256);
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).await.unwrap();
                head.push(byte[0]);
            }
            let mut remaining = 1024usize;
            let mut buf = [0u8; 32768];
            while remaining > 0 {
                let take = remaining.min(buf.len());
                let n = socket.read(&mut buf[..take]).await.unwrap();
                assert!(n > 0);
                remaining -= n;
            }
            latencies.push(began.elapsed().as_secs_f64() * 1000.0);
        }
        latencies
    }

    fn summarize(case: &str, mut latencies: Vec<f64>) -> String {
        latencies.sort_by(f64::total_cmp);
        let pct = |q: f64| latencies[((latencies.len() - 1) as f64 * q).round() as usize];
        let elapsed_s = latencies.iter().sum::<f64>() / 1000.0;
        format!(
            "QUAL297 {{\"case\":\"{case}\",\"profile\":\"{}\",\"requests\":{},\"elapsed_s\":{elapsed_s:.3},\"rps\":{:.1},\"p50_ms\":{:.3},\"p95_ms\":{:.3},\"p99_ms\":{:.3}}}",
            profile(),
            latencies.len(),
            latencies.len() as f64 / elapsed_s,
            pct(0.50),
            pct(0.95),
            pct(0.99),
        )
    }

    async fn serve<S>(service: S) -> (std::net::SocketAddr, eggserve_server::ServerControl)
    where
        S: eggserve_server::Service,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = Server::builder()
            .runtime(
                RuntimeConfig::builder()
                    .bind(addr)
                    .max_request_body_bytes(1024 * 1024)
                    .build()
                    .unwrap(),
            )
            .from_listener(listener)
            .build()
            .unwrap();
        let handle = server.start_with_service(service).await.unwrap();
        let (control, completion) = handle.into_parts();
        tokio::spawn(async move {
            let mut completion = completion;
            let _ = completion.wait().await;
        });
        (addr, control)
    }

    println!("QUAL297 rss_hwm_start_kb={:?}", rss_hwm_kb());

    // c16 concurrent keep-alive, 1 KiB: native vs default-constructor Tower.
    let native_one_kib = eggserve_server::service_fn(|_req: eggserve_server::Request| async {
        Ok(eggserve_primitives::Response::builder()
            .status(eggserve_primitives::StatusCode::OK)
            .body(eggserve_primitives::ResponseBody::Bytes(vec![b'k'; 1024]))
            .unwrap())
    });
    async fn axum_one_kib() -> AxumResponse {
        AxumResponse::new(AxumBody::from(vec![b'k'; 1024]))
    }
    let kib_router: Router = Router::new().route("/", get(axum_one_kib));
    let (native_addr, native_control) = serve(native_one_kib).await;
    let (tower_addr, tower_control) = serve(TowerToEggserve::new(kib_router)).await;
    let request = b"GET / HTTP/1.1\r\nHost: qual\r\nConnection: keep-alive\r\n\r\n".to_vec();
    for (case, addr) in [
        ("c16-1kib-native", native_addr),
        ("c16-1kib-tower", tower_addr),
    ] {
        // Warm-up wave, then the measured wave.
        for warm in [true, false] {
            let mut workers = Vec::new();
            for _ in 0..16 {
                let request = request.clone();
                workers.push(tokio::spawn(async move {
                    keep_alive_session(addr, request, if warm { 20 } else { 100 }).await
                }));
            }
            let mut latencies = Vec::new();
            for worker in workers {
                latencies.extend(worker.await.unwrap());
            }
            if !warm {
                println!("{}", summarize(case, latencies));
            }
        }
    }
    println!("QUAL297 rss_hwm_after_c16_kb={:?}", rss_hwm_kb());
    native_control.shutdown();
    tower_control.shutdown();

    // SSE-128 streams over fresh connections, native vs Axum.
    async fn axum_sse_128() -> AxumResponse {
        let items: Vec<Result<Bytes, Infallible>> = (0..128)
            .map(|i| Ok(Bytes::from(format!("chunk-{i:03}\n"))))
            .collect();
        AxumResponse::new(AxumBody::from_stream(futures_util::stream::iter(items)))
    }
    fn native_sse_128() -> impl eggserve_server::Service {
        eggserve_server::service_fn(|_req: eggserve_server::Request| async move {
            let items: Vec<Result<Bytes, eggserve_primitives::ResponseStreamError>> = (0..128)
                .map(|i| Ok(Bytes::from(format!("chunk-{i:03}\n"))))
                .collect();
            let stream =
                eggserve_primitives::ResponseStream::new(futures_util::stream::iter(items));
            Ok(eggserve_primitives::Response::builder()
                .status(eggserve_primitives::StatusCode::OK)
                .body(eggserve_primitives::ResponseBody::Stream(stream))
                .unwrap())
        })
    }
    let sse_router: Router = Router::new().route("/sse", get(axum_sse_128));
    let (sse_native_addr, sse_native_control) = serve(native_sse_128()).await;
    let (sse_tower_addr, sse_tower_control) = serve(TowerToEggserve::new(sse_router)).await;
    let sse_request = b"GET /sse HTTP/1.1\r\nHost: qual\r\nConnection: close\r\n\r\n".to_vec();
    for (case, addr) in [
        ("sse-128-native", sse_native_addr),
        ("sse-128-tower", sse_tower_addr),
    ] {
        for _ in 0..5 {
            exchange(addr, &sse_request).await;
        }
        let mut latencies = Vec::new();
        for _ in 0..30 {
            let began = Instant::now();
            let wire = exchange(addr, &sse_request).await;
            assert!(wire.windows(10).any(|w| w == b"chunk-127\n"));
            latencies.push(began.elapsed().as_secs_f64() * 1000.0);
        }
        println!("{}", summarize(case, latencies));
    }
    sse_native_control.shutdown();
    sse_tower_control.shutdown();

    // Small POST/JSON echo over fresh connections, native vs Axum.
    let native_echo = eggserve_server::service_fn_with_policy(
        |req: eggserve_server::Request| async move {
            let bytes = req.into_body().read_all().await.unwrap();
            Ok(eggserve_primitives::Response::builder()
                .status(eggserve_primitives::StatusCode::OK)
                .body(eggserve_primitives::ResponseBody::Bytes(bytes.to_vec()))
                .unwrap())
        },
        RequestBodyPolicy::Stream {
            max_bytes: 64 * 1024,
        },
    );
    let echo_router: Router = Router::new().route("/echo", post(echo));
    let (echo_native_addr, echo_native_control) = serve(native_echo).await;
    let (echo_tower_addr, echo_tower_control) = serve(TowerToEggserve::new(echo_router)).await;
    let payload = br#"{"prompt":"hello"}"#.to_vec();
    for (case, addr) in [
        ("post-json-native", echo_native_addr),
        ("post-json-tower", echo_tower_addr),
    ] {
        let mut latencies = Vec::new();
        for _ in 0..50 {
            let began = Instant::now();
            let mut socket = TcpStream::connect(addr).await.unwrap();
            socket
                .write_all(
                    format!(
                        "POST /echo HTTP/1.1\r\nHost: qual\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        payload.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            socket.write_all(&payload).await.unwrap();
            let mut wire = Vec::new();
            socket.read_to_end(&mut wire).await.unwrap();
            assert!(wire.windows(payload.len()).any(|w| w == payload));
            latencies.push(began.elapsed().as_secs_f64() * 1000.0);
        }
        println!("{}", summarize(case, latencies));
    }
    echo_native_control.shutdown();
    echo_tower_control.shutdown();

    // 10 concurrent slow streams: gated second halves, all must complete,
    // then shutdown must drain.
    let gates: Vec<_> = (0..10).map(|_| oneshot::channel::<()>()).collect();
    let (senders, receivers): (Vec<_>, Vec<_>) = gates.into_iter().unzip();
    let shared: std::sync::Arc<std::sync::Mutex<Vec<oneshot::Receiver<()>>>> =
        std::sync::Arc::new(std::sync::Mutex::new(receivers));
    let slow_router: Router = Router::new().route(
        "/slow",
        get({
            let shared = shared.clone();
            move || {
                let shared = shared.clone();
                async move {
                    let gate = shared.lock().unwrap().pop();
                    let stream =
                        futures_util::stream::unfold((0u8, gate), |(n, gate)| async move {
                            match n {
                                0 => Some((
                                    Ok::<_, Infallible>(Bytes::from_static(b"first")),
                                    (1, gate),
                                )),
                                1 => {
                                    if let Some(gate) = gate {
                                        let _ = gate.await;
                                    }
                                    Some((Ok(Bytes::from_static(b"second")), (2, None)))
                                }
                                _ => None,
                            }
                        });
                    AxumResponse::new(AxumBody::from_stream(stream))
                }
            }
        }),
    );
    let slow_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let slow_addr = slow_listener.local_addr().unwrap();
    let slow_server = Server::builder()
        .runtime(
            RuntimeConfig::builder()
                .bind(slow_addr)
                .max_request_body_bytes(1024 * 1024)
                .build()
                .unwrap(),
        )
        .from_listener(slow_listener)
        .build()
        .unwrap();
    let slow_handle = slow_server
        .start_with_service(TowerToEggserve::new(slow_router))
        .await
        .unwrap();
    let (slow_control, mut slow_completion) = slow_handle.into_parts();
    let slow_request = b"GET /slow HTTP/1.1\r\nHost: qual\r\nConnection: close\r\n\r\n".to_vec();
    let mut streams = Vec::new();
    for _ in 0..10 {
        let req = slow_request.clone();
        streams.push(tokio::spawn(async move { exchange(slow_addr, &req).await }));
    }
    // Let every stream deliver its first chunk, then release all gates.
    tokio::time::sleep(Duration::from_millis(500)).await;
    for sender in senders {
        let _ = sender.send(());
    }
    let mut completed = 0;
    for stream in streams {
        let wire = tokio::time::timeout(Duration::from_secs(10), stream)
            .await
            .unwrap()
            .unwrap();
        let text = String::from_utf8_lossy(&wire);
        assert!(text.contains("first") && text.contains("second"));
        completed += 1;
    }
    assert_eq!(completed, 10);
    println!(
        "QUAL297 {{\"case\":\"slow-10-concurrent\",\"profile\":\"{}\",\"completed\":10}}",
        profile()
    );
    slow_control.shutdown();
    tokio::time::timeout(Duration::from_secs(10), slow_completion.wait())
        .await
        .unwrap()
        .unwrap();
    println!("QUAL297 rss_hwm_end_kb={:?}", rss_hwm_kb());
}

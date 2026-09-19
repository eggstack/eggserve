//! Small dependency-free HTTP/1 keep-alive client for Plan 227 evidence.
//!
//! Compile with `rustc -O benchmarks/227-current-head/native_client.rs`.
//! The client intentionally uses only the standard library so the result is
//! not coupled to a load-testing framework or to CPython's HTTP loop.

use std::env;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::{Duration, Instant};

fn parse_args() -> Result<(String, u16, String, usize, usize, usize), String> {
    let args: Vec<_> = env::args().collect();
    if args.len() != 7 {
        return Err(format!(
            "usage: {} HOST PORT PATH EXPECTED_BYTES CONCURRENCY REQUESTS_PER_WORKER",
            args[0]
        ));
    }
    Ok((
        args[1].clone(),
        args[2].parse().map_err(|_| "invalid PORT".to_owned())?,
        args[3].clone(),
        args[4]
            .parse()
            .map_err(|_| "invalid EXPECTED_BYTES".to_owned())?,
        args[5].parse().map_err(|_| "invalid CONCURRENCY".to_owned())?,
        args[6]
            .parse()
            .map_err(|_| "invalid REQUESTS_PER_WORKER".to_owned())?,
    ))
}

fn read_response(stream: &mut TcpStream, expected_bytes: usize) -> io::Result<()> {
    let mut head = Vec::with_capacity(512);
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte)?;
        head.push(byte[0]);
        if head.len() > 64 * 1024 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "headers too large"));
        }
    }
    let text = String::from_utf8_lossy(&head);
    if !text.starts_with("HTTP/1.1 200 ") {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "unexpected status"));
    }
    let content_length = text
        .lines()
        .find_map(|line| {
            line.strip_prefix("Content-Length:")
                .or_else(|| line.strip_prefix("content-length:"))
                .and_then(|value| value.trim().parse::<usize>().ok())
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing content length"))?;
    if content_length != expected_bytes {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "wrong content length"));
    }
    let mut remaining = content_length;
    let mut buffer = [0u8; 32 * 1024];
    while remaining > 0 {
        let read_len = remaining.min(buffer.len());
        let read = stream.read(&mut buffer[..read_len])?;
        if read == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "short body"));
        }
        remaining -= read;
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (host, port, path, expected_bytes, concurrency, requests_per_worker) = parse_args()?;
    if concurrency == 0 || requests_per_worker == 0 {
        return Err("CONCURRENCY and REQUESTS_PER_WORKER must be nonzero".into());
    }
    let start = Arc::new(Barrier::new(concurrency + 1));
    let latencies = Arc::new(Mutex::new(Vec::with_capacity(
        concurrency * requests_per_worker,
    )));
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    let mut workers = Vec::with_capacity(concurrency);
    for _ in 0..concurrency {
        let start = Arc::clone(&start);
        let latencies = Arc::clone(&latencies);
        let errors = Arc::clone(&errors);
        let host = host.clone();
        let path = path.clone();
        workers.push(thread::spawn(move || {
            let result = (|| -> io::Result<()> {
                let mut stream = TcpStream::connect((host.as_str(), port))?;
                stream.set_read_timeout(Some(Duration::from_secs(30)))?;
                stream.set_write_timeout(Some(Duration::from_secs(30)))?;
                start.wait();
                let request = format!(
                    "GET {path} HTTP/1.1\r\nHost: benchmark\r\nConnection: keep-alive\r\n\r\n"
                );
                for _ in 0..requests_per_worker {
                    let began = Instant::now();
                    stream.write_all(request.as_bytes())?;
                    read_response(&mut stream, expected_bytes)?;
                    latencies.lock().unwrap().push(began.elapsed().as_secs_f64() * 1000.0);
                }
                Ok(())
            })();
            if let Err(error) = result {
                errors.lock().unwrap().push(error.to_string());
            }
        }));
    }
    let began = Instant::now();
    start.wait();
    for worker in workers {
        worker.join().map_err(|_| "worker panicked")?;
    }
    let elapsed = began.elapsed().as_secs_f64();
    let latencies = latencies.lock().unwrap();
    let errors = errors.lock().unwrap();
    let mut ordered = latencies.clone();
    ordered.sort_by(f64::total_cmp);
    let percentile = |q: f64| -> f64 {
        if ordered.is_empty() {
            return 0.0;
        }
        let index = ((ordered.len() - 1) as f64 * q).round() as usize;
        ordered[index]
    };
    let requests = latencies.len();
    println!(
        "{{\"requests\":{requests},\"expected_requests\":{},\"elapsed_s\":{elapsed:.6},\"rps\":{:.3},\"bytes_per_s\":{:.3},\"p50_latency_ms\":{:.3},\"p95_latency_ms\":{:.3},\"p99_latency_ms\":{:.3},\"error_count\":{},\"errors\":{:?},\"client\":\"rust-std-tcp\"}}",
        concurrency * requests_per_worker,
        requests as f64 / elapsed,
        requests as f64 * expected_bytes as f64 / elapsed,
        percentile(0.50),
        percentile(0.95),
        percentile(0.99),
        errors.len(),
        &*errors,
    );
    if !errors.is_empty() {
        return Err("native client recorded errors".into());
    }
    Ok(())
}

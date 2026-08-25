//! CLI validation parity tests (Plan 080, Track I).
//!
//! Tests that the CLI binary produces actionable error messages for
//! invalid configuration values and exits with non-zero status.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

fn eggserve_bin() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_eggserve"));
    // Use a temporary directory to avoid serving the workspace
    cmd.arg("--directory");
    cmd.arg(std::env::temp_dir());
    cmd
}

#[test]
fn zero_max_connections_exits_with_error() {
    let output = eggserve_bin()
        .arg("--max-connections")
        .arg("0")
        .output()
        .expect("failed to execute binary");
    assert!(!output.status.success(), "expected non-zero exit code");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--max-connections"),
        "stderr should mention --max-connections: {}",
        stderr
    );
}

#[test]
fn zero_max_file_streams_exits_with_error() {
    let output = eggserve_bin()
        .arg("--max-file-streams")
        .arg("0")
        .output()
        .expect("failed to execute binary");
    assert!(!output.status.success(), "expected non-zero exit code");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--max-file-streams"),
        "stderr should mention --max-file-streams: {}",
        stderr
    );
}

#[test]
fn zero_handler_timeout_exits_with_error() {
    let output = eggserve_bin()
        .arg("--handler-timeout")
        .arg("0")
        .output()
        .expect("failed to execute binary");
    assert!(!output.status.success(), "expected non-zero exit code");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("handler-timeout") || stderr.contains("handler_timeout"),
        "stderr should mention handler timeout: {}",
        stderr
    );
}

#[test]
fn zero_body_read_timeout_exits_with_error() {
    let output = eggserve_bin()
        .arg("--body-read-timeout")
        .arg("0")
        .output()
        .expect("failed to execute binary");
    assert!(!output.status.success(), "expected non-zero exit code");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("body-read-timeout") || stderr.contains("body_read_timeout"),
        "stderr should mention body read timeout: {}",
        stderr
    );
}

#[test]
fn invalid_timeout_value_exits_with_error() {
    let output = eggserve_bin()
        .arg("--handler-timeout")
        .arg("not_a_number")
        .output()
        .expect("failed to execute binary");
    assert!(!output.status.success(), "expected non-zero exit code");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid"),
        "stderr should mention invalid: {}",
        stderr
    );
}

#[test]
fn help_flag_shows_usage() {
    let output = Command::new(env!("CARGO_BIN_EXE_eggserve"))
        .arg("--help")
        .output()
        .expect("failed to execute binary");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.stderr.is_empty(), "help should be written to stdout");
    assert!(
        stdout.contains("--max-connections"),
        "help should mention --max-connections: {}",
        stdout
    );
    assert!(
        stdout.contains("--handler-timeout"),
        "help should mention --handler-timeout: {}",
        stdout
    );
    assert!(
        stdout.contains("--body-read-timeout"),
        "help should mention --body-read-timeout: {}",
        stdout
    );
}

#[test]
fn header_timeout_cannot_exceed_connection_timeout() {
    let output = eggserve_bin()
        .arg("--header-timeout")
        .arg("2")
        .arg("--connection-total-timeout")
        .arg("1")
        .output()
        .expect("failed to execute binary");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("header_read_timeout"));
}

#[test]
fn version_flag_shows_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_eggserve"))
        .arg("--version")
        .output()
        .expect("failed to execute binary");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("eggserve"),
        "output should mention eggserve: {}",
        combined
    );
}

// ---------------------------------------------------------------------------
// Plan 134–137 positional CLI regression tests
// ---------------------------------------------------------------------------

#[test]
fn bind_host_only_with_numeric_port_and_numeric_dir() {
    // --bind HOST (no port) leaves port slot open; first numeric positional
    // is port, second is directory name.
    let mut child = Command::new(env!("CARGO_BIN_EXE_eggserve"))
        .arg("--bind")
        .arg("127.0.0.1")
        .arg("12345")
        .arg("1234")
        .arg("--version")
        .spawn()
        .expect("failed to spawn binary");
    std::thread::sleep(std::time::Duration::from_millis(200));
    let _ = child.kill();
    let output = child.wait_with_output().expect("failed to wait on child");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !combined.contains("error"),
        "expected clean arg parse, got: {}",
        combined
    );
}

#[test]
fn positional_port_then_numeric_directory() {
    // First numeric positional is port, second is directory name.
    let mut child = Command::new(env!("CARGO_BIN_EXE_eggserve"))
        .arg("12346")
        .arg("5678")
        .arg("--version")
        .spawn()
        .expect("failed to spawn binary");
    std::thread::sleep(std::time::Duration::from_millis(200));
    let _ = child.kill();
    let output = child.wait_with_output().expect("failed to wait on child");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !combined.contains("error"),
        "expected clean arg parse, got: {}",
        combined
    );
}

#[test]
fn bind_host_only_with_port_flag_and_numeric_dir() {
    // --bind HOST (no port) + --port sets port via flag; numeric positional
    // then becomes directory.
    let mut child = Command::new(env!("CARGO_BIN_EXE_eggserve"))
        .arg("--bind")
        .arg("127.0.0.1")
        .arg("--port")
        .arg("12347")
        .arg("1234")
        .arg("--version")
        .spawn()
        .expect("failed to spawn binary");
    std::thread::sleep(std::time::Duration::from_millis(200));
    let _ = child.kill();
    let output = child.wait_with_output().expect("failed to wait on child");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !combined.contains("error"),
        "expected clean arg parse, got: {}",
        combined
    );
}

#[test]
fn addr_and_port_conflict() {
    // --addr sets both host and port; --port after --addr is rejected.
    let output = Command::new(env!("CARGO_BIN_EXE_eggserve"))
        .arg("--addr")
        .arg("127.0.0.1:80")
        .arg("--port")
        .arg("443")
        .output()
        .expect("failed to execute binary");
    assert!(!output.status.success(), "expected non-zero exit code");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--port"),
        "stderr should mention --port: {}",
        stderr
    );
    assert!(
        stderr.contains("--addr"),
        "stderr should mention --addr: {}",
        stderr
    );
}

// ---------------------------------------------------------------------------
// IPv6 bracketed bind end-to-end
// ---------------------------------------------------------------------------

#[test]
fn ipv6_bracketed_bind_serves_end_to_end() {
    // Skip on hosts without IPv6 loopback.
    if std::net::TcpListener::bind("[::1]:0").is_err() {
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("hello.txt"), "hello ipv6").unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_eggserve"))
        .arg("--bind")
        .arg("[::1]:0")
        .arg("--directory")
        .arg(tmp.path())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn binary");

    let stderr = child.stderr.take().expect("stderr was piped");
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // The startup log must name the resolved local address, including the
    // OS-assigned port for a port-0 bind.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut listening = None;
    while std::time::Instant::now() < deadline {
        match rx.recv_timeout(std::time::Duration::from_millis(200)) {
            Ok(line) => {
                let marker = "Listening: ";
                if let Some(idx) = line.find(marker) {
                    // Strip the scheme (http:// or https://); no certificate
                    // is configured here, so the endpoint serves plain HTTP.
                    let url = line[idx + marker.len()..].trim();
                    let addr = url.split("://").nth(1).unwrap_or(url);
                    listening = Some(addr.split_whitespace().next().unwrap_or("").to_string());
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let request_result = listening.as_ref().map(|addr| {
        std::net::TcpStream::connect(addr).and_then(|mut stream| {
            stream.write_all(
                b"GET /hello.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            )?;
            let mut response = String::new();
            stream.read_to_string(&mut response)?;
            Ok(response)
        })
    });

    let _ = child.kill();
    let _ = child.wait();

    let addr = listening.expect("server did not report its listening address");
    assert!(
        addr.starts_with('['),
        "expected bracketed IPv6 listen addr, got: {}",
        addr
    );
    let response = request_result
        .expect("request never ran")
        .expect("request failed");
    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "unexpected status line: {}",
        response
    );
    assert!(
        response.ends_with("hello ipv6"),
        "missing body: {}",
        response
    );
}

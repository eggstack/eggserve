//! CLI validation parity tests (Plan 080, Track I).
//!
//! Tests that the CLI binary produces actionable error messages for
//! invalid configuration values and exits with non-zero status.

use std::process::Command;

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
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--max-connections"),
        "help should mention --max-connections: {}",
        stderr
    );
    assert!(
        stderr.contains("--handler-timeout"),
        "help should mention --handler-timeout: {}",
        stderr
    );
    assert!(
        stderr.contains("--body-read-timeout"),
        "help should mention --body-read-timeout: {}",
        stderr
    );
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

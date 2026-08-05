//! Logging mode tests (Plan 103, Track A).
//!
//! Verifies that the CLI binary produces the correct stderr output for each
//! logging mode: default (text), JSON, quiet, and none.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::Duration;

fn eggserve_bin() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_eggserve"));
    cmd.arg("--directory");
    cmd.arg(std::env::temp_dir());
    cmd
}

/// Start the binary with the given args, wait for up to `timeout` for output,
/// then kill and return the collected stderr lines.
fn capture_stderr(args: &[&str], timeout: Duration) -> Vec<String> {
    let mut cmd = eggserve_bin();
    for arg in args {
        cmd.arg(arg);
    }
    cmd.arg("--addr")
        .arg("127.0.0.1:0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("failed to spawn binary");
    let stderr = child.stderr.take().unwrap();
    let reader = BufReader::new(stderr);

    // Spawn a thread to read lines with a timeout
    let handle = std::thread::spawn(move || {
        let mut lines = Vec::new();
        for line in reader.lines() {
            match line {
                Ok(l) => lines.push(l),
                Err(_) => break,
            }
        }
        lines
    });

    // Wait for the timeout, then kill
    std::thread::sleep(timeout);
    let _ = child.kill();
    let _ = child.wait();

    handle.join().unwrap_or_default()
}

/// Start the binary with the given args, wait briefly, then kill and return
/// whether the process exited with a non-zero status and the stderr output.
fn run_and_capture(args: &[&str]) -> (bool, Vec<String>) {
    let mut cmd = eggserve_bin();
    for arg in args {
        cmd.arg(arg);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("failed to spawn binary");
    let stderr = child.stderr.take().unwrap();
    let reader = BufReader::new(stderr);

    let handle = std::thread::spawn(move || {
        let mut lines = Vec::new();
        for line in reader.lines() {
            match line {
                Ok(l) => lines.push(l),
                Err(_) => break,
            }
        }
        lines
    });

    // Wait for process to exit (should exit quickly for arg validation errors)
    let status = child.wait().expect("failed to wait for child");
    let stderr_lines = handle.join().unwrap_or_default();

    (!status.success(), stderr_lines)
}

// ---------------------------------------------------------------------------
// Test 1: Default mode emits a listener/startup event
// ---------------------------------------------------------------------------

#[test]
fn default_mode_emits_startup_event() {
    let stderr = capture_stderr(&[], Duration::from_secs(2));
    let combined = stderr.join("\n");
    assert!(
        combined.contains("process_starting"),
        "default mode should emit process_starting: {}",
        combined
    );
    assert!(
        combined.contains("listener_ready"),
        "default mode should emit listener_ready: {}",
        combined
    );
}

// ---------------------------------------------------------------------------
// Test 2: JSON mode emits valid JSON Lines at least for startup
// ---------------------------------------------------------------------------

#[test]
fn json_mode_emits_valid_json() {
    let stderr = capture_stderr(&["--log-format", "json"], Duration::from_secs(2));
    for line in &stderr {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Validate basic JSON structure: starts with '{', ends with '}'
        assert!(
            line.starts_with('{') && line.ends_with('}'),
            "JSON mode should emit valid JSON (must start with '{{' and end with '}}'): {}",
            line
        );
        // Check for required fields
        assert!(
            line.contains("\"schema_version\""),
            "JSON should contain schema_version: {}",
            line
        );
        assert!(
            line.contains("\"severity\""),
            "JSON should contain severity: {}",
            line
        );
        assert!(
            line.contains("\"event\""),
            "JSON should contain event: {}",
            line
        );
    }
    let combined = stderr.join("\n");
    assert!(
        combined.contains("process_starting"),
        "JSON mode should emit process_starting: {}",
        combined
    );
}

// ---------------------------------------------------------------------------
// Test 3: Quiet mode omits the normal listener/startup message
// ---------------------------------------------------------------------------

#[test]
fn quiet_mode_omits_startup_message() {
    let stderr = capture_stderr(&["--quiet"], Duration::from_secs(2));
    let combined = stderr.join("\n");
    assert!(
        !combined.contains("process_starting"),
        "quiet mode should not emit process_starting: {}",
        combined
    );
    assert!(
        !combined.contains("listener_ready"),
        "quiet mode should not emit listener_ready: {}",
        combined
    );
}

// ---------------------------------------------------------------------------
// Test 4: Quiet mode still emits a forced startup error
// ---------------------------------------------------------------------------

#[test]
fn quiet_mode_still_emits_startup_error() {
    let (non_zero, stderr) = run_and_capture(&["--quiet", "--max-connections", "0"]);
    let combined = stderr.join("\n");
    assert!(non_zero, "invalid config should exit non-zero");
    assert!(
        combined.contains("error") || combined.contains("must be"),
        "quiet mode should still emit startup errors: {}",
        combined
    );
}

// ---------------------------------------------------------------------------
// Test 5: None mode emits no routine output during start/stop
// ---------------------------------------------------------------------------

#[test]
fn none_mode_emits_no_routine_output() {
    let stderr = capture_stderr(&["--log-format", "none"], Duration::from_secs(2));
    assert!(
        stderr.is_empty(),
        "none mode should emit no output: {:?}",
        stderr
    );
}

// ---------------------------------------------------------------------------
// Test 6: None mode still reports an invalid CLI invocation before startup
// ---------------------------------------------------------------------------

#[test]
fn none_mode_reports_invalid_invocation() {
    let (non_zero, stderr) = run_and_capture(&["--log-format", "none", "--max-connections", "0"]);
    let combined = stderr.join("\n");
    assert!(
        non_zero,
        "invalid invocation should exit with non-zero status"
    );
    assert!(
        combined.contains("error") || combined.contains("must be"),
        "none mode should still report invalid CLI invocation: {}",
        combined
    );
}

// ---------------------------------------------------------------------------
// Test 7: --quiet --log-format json does not emit informational JSON records
// ---------------------------------------------------------------------------

#[test]
fn quiet_json_mode_suppresses_info_records() {
    let stderr = capture_stderr(&["--quiet", "--log-format", "json"], Duration::from_secs(2));
    assert!(
        stderr.is_empty(),
        "quiet+json mode should not emit informational records: {:?}",
        stderr
    );
}

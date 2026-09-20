#!/usr/bin/env python3
"""Plan 233 environment capture (benchmark-only, stdlib only).

Writes ``raw/environment.json`` with the exact machine/toolchain/lockfile/
command identity shared by all Plan 233 trial files. Per-regime source
identity (SHA, temporary-diff hash, chunk bytes) lives inside each raw trial
file; this file binds them to one host and one command set.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import subprocess
import sys
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


def command_output(command: list[str]) -> str | None:
    try:
        return subprocess.check_output(command, text=True, cwd=str(ROOT)).strip()
    except (OSError, subprocess.CalledProcessError):
        return None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path,
                        default=Path(__file__).with_name("raw") / "environment.json")
    parser.add_argument("--base-sha", default=None)
    parser.add_argument("--diff-sha256-64k", default=None)
    parser.add_argument("--trials", type=int, default=3)
    args = parser.parse_args()

    cpu = "unknown"
    cpuinfo = Path("/proc/cpuinfo")
    if cpuinfo.exists():
        for line in cpuinfo.read_text().splitlines():
            if line.lower().startswith("model name"):
                cpu = line.split(":", 1)[1].strip()
                break
    memory_gb = None
    if Path("/proc/meminfo").exists():
        for line in Path("/proc/meminfo").read_text().splitlines():
            if line.startswith("MemTotal:"):
                memory_gb = round(int(line.split()[1]) / 1024 / 1024, 2)
                break
    python_lock = ROOT / "crates/eggserve-python/Cargo.lock"
    environment = {
        "schema_version": 1,
        "plan": "233",
        "source_sha": command_output(["git", "rev-parse", "HEAD"]),
        "base_sha_for_64k_override": args.base_sha,
        "temporary_64k_diff_sha256": args.diff_sha256_64k,
        "cargo_lock_sha256": hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest(),
        "python_crate_lock_sha256": (
            hashlib.sha256(python_lock.read_bytes()).hexdigest()
            if python_lock.exists() else None
        ),
        "rust_toolchain": command_output(["rustc", "--version"]),
        "os": platform.platform(),
        "kernel": command_output(["uname", "-r"]),
        "arch": platform.machine(),
        "cpu": cpu,
        "logical_cpus": os.cpu_count(),
        "memory_gb": memory_gb,
        "python": sys.version.split()[0],
        "release_profile": "release",
        "enabled_features": ["tls"],
        "build_command": "cargo build --release --locked -p eggserve-bin --features tls",
        "runtime_limits": {
            "max_connections": 512, "max_file_streams": 512,
            "max_in_flight_requests": 512, "max_buf_size": 65536,
            "max_headers": 100, "max_header_bytes": 32768,
            "max_request_target_bytes": 8192,
        },
        "chunk_regimes": {"64k": 65536, "128k": 131072},
        "final_default_chunk_bytes": 131072,
        "benchmark_commands": {
            "native_128k": "python3 benchmarks/233-evidence-polish/harness.py --chunk-label 128k --chunk-bytes 131072",
            "native_64k": "python3 benchmarks/233-evidence-polish/harness.py --chunk-label 64k --chunk-bytes 65536 --source-diff-sha256 <diff> (temporary source-only default override, restored before commit)",
            "tls": "python3 benchmarks/233-evidence-polish/harness.py --tls-only --chunk-label 128k --chunk-bytes 131072 --tls-cert /tmp/<ephemeral>/cert.pem --tls-key /tmp/<ephemeral>/key.pem",
        },
        "warmup_policy": "one excluded run per case",
        "trial_count": args.trials,
        "tls_certificate_command": (
            "openssl req -x509 -newkey rsa:2048 -nodes -days 1 "
            "-subj /CN=localhost -keyout /tmp/<ephemeral>/key.pem "
            "-out /tmp/<ephemeral>/cert.pem (ephemeral; keys never committed)"
        ),
        "clients": {
            "native": "native_client.rs dependency-free Rust std::net keep-alive client",
            "ranges_tls": "CPython stdlib http.client + threading",
        },
        "absolute_timing_ci_gate": False,
        "captured_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(environment, indent=2) + "\n")
    print(json.dumps({"output": str(args.output)}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

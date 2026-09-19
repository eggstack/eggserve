#!/usr/bin/env python3
"""Plan 227 native-client loopback harness.

The orchestration uses the standard library, but all measured HTTP requests
are issued by the dependency-free Rust client beside this file. The output is
compact trial evidence, not a CI timing gate.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import socket
import statistics
import subprocess
import tempfile
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CLIENT = Path(__file__).with_name("native_client")


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def command_output(command: list[str]) -> str | None:
    try:
        return subprocess.check_output(command, text=True).strip()
    except (OSError, subprocess.CalledProcessError):
        return None


def environment() -> dict:
    cpu = "unknown"
    cpuinfo = Path("/proc/cpuinfo")
    if cpuinfo.exists():
        for line in cpuinfo.read_text().splitlines():
            if line.lower().startswith("model name"):
                cpu = line.split(":", 1)[1].strip()
                break
    return {
        "os": platform.platform(),
        "arch": platform.machine(),
        "cpu": cpu,
        "logical_cpus": os.cpu_count(),
        "rustc": command_output(["rustc", "--version"]),
        "client": "native_client.rs rust-std-tcp",
    }


def sample(pid: int) -> dict:
    result: dict[str, int | float] = {}
    status = Path(f"/proc/{pid}/status")
    if status.exists():
        for line in status.read_text().splitlines():
            if line.startswith("VmRSS:"):
                result["rss_kb"] = int(line.split()[1])
            elif line.startswith("VmHWM:"):
                result["peak_rss_kb"] = int(line.split()[1])
            elif line.startswith("Threads:"):
                result["threads"] = int(line.split()[1])
    try:
        result["fds"] = len(list(Path(f"/proc/{pid}/fd").iterdir()))
    except OSError:
        pass
    return result


def wait_ready(port: int, process: subprocess.Popen) -> None:
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError("server exited before native client became ready")
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.5):
                return
        except OSError:
            time.sleep(0.05)
    raise RuntimeError("server did not become ready")


def run_case(cli: Path, root: Path, path: str, size: int, concurrency: int, trials: int) -> dict:
    records = []
    for trial in range(1, trials + 1):
        port = free_port()
        command = [
            str(cli),
            "--directory",
            str(root),
            "--bind",
            f"127.0.0.1:{port}",
            "--log-format",
            "none",
            "--max-connections",
            "512",
            "--max-file-streams",
            "512",
            "--max-in-flight-requests",
            "512",
            "--max-buf-size",
            "65536",
            "--max-headers",
            "100",
            "--max-header-bytes",
            "32768",
            "--max-request-target-bytes",
            "8192",
        ]
        process = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            wait_ready(port, process)
            requests_per_worker = max(1, {1024: 3000, 128 * 1024: 600, 1024 * 1024: 120, 16 * 1024 * 1024: 8}.get(size, 64) // concurrency)
            subprocess.run(
                [str(CLIENT), "127.0.0.1", str(port), path, str(size), str(concurrency), str(requests_per_worker)],
                check=True,
                text=True,
                capture_output=True,
            )
            before = sample(process.pid)
            native = subprocess.run(
                [str(CLIENT), "127.0.0.1", str(port), path, str(size), str(concurrency), str(requests_per_worker)],
                check=True,
                text=True,
                capture_output=True,
            )
            after = sample(process.pid)
            record = json.loads(native.stdout)
            record.update({"trial": trial, "path": path, "response_size": size, "concurrency": concurrency, "rss_before": before.get("rss_kb"), "rss_after": after.get("rss_kb"), "peak_rss_kb": after.get("peak_rss_kb"), "fds_after": after.get("fds"), "threads_after": after.get("threads")})
            records.append(record)
        finally:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=10)
    return {"path": path, "response_size": size, "concurrency": concurrency, "trials": records, "median_rps": statistics.median(record["rps"] for record in records), "rps_spread": [min(record["rps"] for record in records), max(record["rps"] for record in records)], "total_errors": sum(record["error_count"] for record in records)}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cli", type=Path, default=ROOT / "target/release/eggserve")
    parser.add_argument("--trials", type=int, default=3)
    parser.add_argument("--output", type=Path, default=Path(__file__).with_name("native-results.json"))
    args = parser.parse_args()
    if args.trials < 3:
        parser.error("Plan 227 requires at least three measured trials")
    if not args.cli.exists():
        parser.error("build target/release/eggserve first")
    subprocess.run(["rustc", "-O", str(Path(__file__).with_name("native_client.rs")), "-o", str(CLIENT)], check=True)
    root = Path(tempfile.mkdtemp(prefix="eggserve-227-native-"))
    try:
        for size, name in ((1024, "f1k.bin"), (128 * 1024, "f128k.bin"), (1024 * 1024, "f1m.bin")):
            (root / name).write_bytes(bytes([size // 1024 % 251]) * size)
        cases = [
            run_case(args.cli, root, f"/{name}", size, concurrency, args.trials)
            for size, name in ((1024, "f1k.bin"), (128 * 1024, "f128k.bin"), (1024 * 1024, "f1m.bin"))
            for concurrency in (1, 16, 64)
        ]
        result = {
            "schema_version": 1,
            "plan": "227",
            "workload": "native static HTTP/1 keep-alive",
            "source_sha": command_output(["git", "rev-parse", "HEAD"]),
            "cargo_lock_sha256": hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest(),
            "captured_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "environment": environment(),
            "build": {"command": "cargo build --release --locked -p eggserve-bin", "profile": "release", "features": []},
            "runtime_limits": {"max_connections": 512, "max_file_streams": 512, "max_in_flight_requests": 512, "max_buf_size": 65536, "max_headers": 100, "max_header_bytes": 32768, "max_request_target_bytes": 8192},
            "method": {"trials": args.trials, "warmup": "one excluded native-client trial per case", "connection_reuse": "one keep-alive TCP connection per worker", "absolute_timing_ci_gate": False},
            "workloads": cases,
            "notes": ["The HTTP client is dependency-free Rust std::net code; Python only orchestrates server processes and JSON capture.", "Results are same-machine evidence and are not a universal performance claim.", "No TLS or CPython substitution is included in this native-client capture."],
        }
        args.output.write_text(json.dumps(result, indent=2) + "\n")
        print(json.dumps({"output": str(args.output), "source_sha": result["source_sha"], "cases": len(cases)}, indent=2))
    finally:
        for path in (CLIENT,):
            try:
                path.unlink()
            except FileNotFoundError:
                pass
        for path in root.iterdir():
            path.unlink()
        root.rmdir()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Plan 241 native H1 evidence harness.

This is benchmark-only code.  It records correctness together with timing for
the custom-service and static response-shape tracks, and captures the
caller-owned H1 smoke result.  It intentionally has no timing gate.
"""

from __future__ import annotations

import argparse
import hashlib
import http.client
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
CLIENT_SOURCE = ROOT / "benchmarks/227-current-head/native_client.rs"


def command_output(command: list[str], cwd: Path = ROOT) -> str | None:
    try:
        return subprocess.check_output(command, text=True, cwd=cwd).strip()
    except (OSError, subprocess.CalledProcessError):
        return None


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def sample(pid: int) -> dict[str, int | float]:
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


class Process:
    def __init__(self, command: list[str], *, bind_flag: bool = True):
        self.port = free_port()
        bind = ["--bind", f"127.0.0.1:{self.port}"] if bind_flag else [f"127.0.0.1:{self.port}"]
        self.command = command + bind
        self.process = subprocess.Popen(
            self.command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
        )

    def ready(self) -> None:
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError(f"server exited: {self.command}")
            try:
                with socket.create_connection(("127.0.0.1", self.port), timeout=0.5):
                    return
            except OSError:
                time.sleep(0.05)
        raise RuntimeError(f"server did not become ready: {self.command}")

    def close(self) -> None:
        if self.process.poll() is None:
            self.process.terminate()
        try:
            self.process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=10)


def request(
    conn: http.client.HTTPConnection,
    method: str,
    path: str,
    headers: dict[str, str] | None = None,
) -> dict:
    began = time.perf_counter()
    try:
        conn.request(method, path, headers={"Connection": "keep-alive", **(headers or {})})
        response = conn.getresponse()
        body = response.read()
        elapsed_ms = (time.perf_counter() - began) * 1000
        return {
            "status": response.status,
            "body_bytes": len(body),
            "body_sha256": hashlib.sha256(body).hexdigest(),
            "content_length": response.getheader("Content-Length"),
            "etag": response.getheader("ETag"),
            "content_range": response.getheader("Content-Range"),
            "elapsed_ms": elapsed_ms,
            "error": None,
        }
    except Exception as exc:  # benchmark errors are retained as data
        return {
            "status": None,
            "body_bytes": None,
            "body_sha256": None,
            "content_length": None,
            "etag": None,
            "content_range": None,
            "elapsed_ms": None,
            "error": f"{type(exc).__name__}: {exc}",
        }


def static_command(cli: Path, root: Path) -> list[str]:
    return [
        str(cli),
        "--directory", str(root),
        "--log-format", "none",
        "--max-connections", "512",
        "--max-file-streams", "512",
        "--max-in-flight-requests", "512",
        "--max-buf-size", "65536",
        "--max-headers", "100",
        "--max-header-bytes", "32768",
        "--max-request-target-bytes", "8192",
    ]


def write_static_root() -> tuple[Path, dict[str, bytes]]:
    root = Path(tempfile.mkdtemp(prefix="eggserve-241-static-"))
    data = {
        "one.bin": b"o" * 1024,
        "deep.bin": b"d" * 2048,
        "encoded.bin": b"e" * 3072,
        "index.html": b"root resource\n",
    }
    (root / "nested").mkdir()
    (root / "one.bin").write_bytes(data["one.bin"])
    (root / "nested" / "deep.bin").write_bytes(data["deep.bin"])
    (root / "encoded.bin").write_bytes(data["encoded.bin"])
    (root / "index.html").write_bytes(data["index.html"])
    return root, data


def run_static(cli: Path, trials: int) -> dict:
    root, data = write_static_root()
    process = Process(static_command(cli, root))
    cases = [
        ("head-known-length", "HEAD", "/one.bin", None, 200, 0, data["one.bin"]),
        ("one-component-short", "GET", "/one.bin", None, 200, 1024, data["one.bin"]),
        ("nested-path", "GET", "/nested/deep.bin", None, 200, 2048, data["deep.bin"]),
        ("query-bearing-target", "GET", "/one.bin?fixed=1", None, 200, 1024, data["one.bin"]),
        ("percent-encoded-safe-path", "GET", "/encoded%2Ebin", None, 200, 3072, data["encoded.bin"]),
        ("root-directory-resource", "GET", "/", None, 200, len(data["index.html"]), data["index.html"]),
        ("satisfiable-range", "GET", "/one.bin", {"Range": "bytes=10-109"}, 206, 100, data["one.bin"][10:110]),
    ]
    try:
        process.ready()
        records = []
        for name, method, path, headers, status, body_bytes, expected in cases:
            conn = http.client.HTTPConnection("127.0.0.1", process.port, timeout=30)
            warmup = request(conn, method, path, headers)
            rows = []
            for trial in range(1, trials + 1):
                measured = request(conn, method, path, headers)
                expected_hash = hashlib.sha256(expected).hexdigest()
                measured.update({
                    "trial": trial,
                    "expected_status": status,
                    "expected_body_bytes": body_bytes,
                    "correct": (
                        measured["status"] == status
                        and measured["body_bytes"] == body_bytes
                        and (method == "HEAD" or measured["body_sha256"] == expected_hash)
                    ),
                })
                rows.append(measured)
            conn.close()
            records.append({
                "name": name,
                "method": method,
                "path": path,
                "headers": headers or {},
                "warmup": warmup,
                "trials": rows,
                "median_latency_ms": statistics.median(
                    row["elapsed_ms"] for row in rows if row["elapsed_ms"] is not None
                ),
                "errors": sum(row["error"] is not None for row in rows),
                "all_correct": all(row["correct"] for row in rows),
            })
        # Conditional 304 depends on the representation's actual validator.
        conn = http.client.HTTPConnection("127.0.0.1", process.port, timeout=30)
        initial = request(conn, "GET", "/one.bin")
        etag = initial["etag"]
        rows = []
        warmup = request(conn, "GET", "/one.bin", {"If-None-Match": etag or ""})
        for trial in range(1, trials + 1):
            measured = request(conn, "GET", "/one.bin", {"If-None-Match": etag or ""})
            measured.update({
                "trial": trial,
                "expected_status": 304,
                "expected_body_bytes": 0,
                "correct": measured["status"] == 304 and measured["body_bytes"] == 0,
            })
            rows.append(measured)
        conn.close()
        records.append({
            "name": "conditional-304",
            "method": "GET",
            "path": "/one.bin",
            "headers": {"If-None-Match": etag},
            "warmup": warmup,
            "trials": rows,
            "median_latency_ms": statistics.median(row["elapsed_ms"] for row in rows),
            "errors": sum(row["error"] is not None for row in rows),
            "all_correct": all(row["correct"] for row in rows),
        })
        return {"cases": records, "server_resources": process.__dict__.get("_resources", {})}
    finally:
        process.close()
        for path in root.iterdir():
            if path.is_dir():
                for child in path.iterdir():
                    child.unlink()
                path.rmdir()
            else:
                path.unlink()
        root.rmdir()


def run_custom(custom: Path, trials: int) -> dict:
    client_dir = Path(tempfile.mkdtemp(prefix="eggserve-241-client-"))
    client = client_dir / "native_client"
    subprocess.run(["rustc", "-O", str(CLIENT_SOURCE), "-o", str(client)], check=True)
    process = Process([str(custom)], bind_flag=False)
    cases = []
    try:
        process.ready()
        for concurrency in (1, 16, 64):
            requests_per_worker = max(1, 3000 // concurrency)
            command = [
                str(client), "127.0.0.1", str(process.port), "/bytes/1024",
                "1024", str(concurrency), str(requests_per_worker),
            ]
            subprocess.run(command, check=True, capture_output=True, text=True)
            rows = []
            for trial in range(1, trials + 1):
                before = sample(process.process.pid)
                completed = subprocess.run(command, capture_output=True, text=True)
                after = sample(process.process.pid)
                row = {"trial": trial, "returncode": completed.returncode, **after}
                if completed.returncode == 0:
                    row.update(json.loads(completed.stdout))
                else:
                    row.update({"error": completed.stderr[-500:]})
                row["resources_before"] = before
                row["resources_after"] = after
                rows.append(row)
            cases.append({
                "concurrency": concurrency,
                "requests_per_worker": requests_per_worker,
                "trials": rows,
                "total_errors": sum(row.get("error_count", 1) for row in rows),
                "all_correct": all(row.get("returncode") == 0 for row in rows),
            })
        return {"cases": cases}
    finally:
        process.close()
        client.unlink(missing_ok=True)
        client_dir.rmdir()


def run_caller_owned(binary: Path) -> dict:
    completed = subprocess.run([str(binary)], capture_output=True, text=True, timeout=30)
    return {
        "returncode": completed.returncode,
        "stdout": completed.stdout,
        "stderr": completed.stderr,
        "correct": completed.returncode == 0 and "HTTP/1.1 200" in completed.stdout,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--custom", type=Path, required=True)
    parser.add_argument("--caller-owned", type=Path, required=True)
    parser.add_argument("--sha", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--trials", type=int, default=3)
    args = parser.parse_args()
    if args.trials < 3:
        parser.error("Plan 241 requires at least three measured trials")

    root_lock = ROOT / "Cargo.lock"
    python_lock = ROOT / "crates/eggserve-python/Cargo.lock"
    result = {
        "schema_version": 1,
        "plan": "241",
        "source_sha": args.sha,
        "binary": str(args.cli),
        "cargo_lock_sha256": hashlib.sha256(root_lock.read_bytes()).hexdigest(),
        "python_crate_lock_sha256": hashlib.sha256(python_lock.read_bytes()).hexdigest(),
        "compiler": command_output(["rustc", "--version"]),
        "environment": {
            "os": platform.platform(),
            "arch": platform.machine(),
            "cpu": next((line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines() if line.lower().startswith("model name")), "unknown"),
            "logical_cpus": os.cpu_count(),
            "memory_kb": next((int(line.split()[1]) for line in Path("/proc/meminfo").read_text().splitlines() if line.startswith("MemTotal:")), None),
            "python": command_output(["python3.11", "--version"]),
        },
        "build": {"command": "cargo +1.89 build --release --locked -p eggserve-bin", "profile": "release", "features": []},
        "runtime_limits": {"max_connections": 512, "max_file_streams": 512, "max_in_flight_requests": 512, "max_buf_size": 65536, "max_headers": 100, "max_header_bytes": 32768, "max_request_target_bytes": 8192},
        "method": {"trials": args.trials, "warmup": "one excluded request/client run per case", "absolute_timing_ci_gate": False},
        "workloads": {
            "custom_h1_1k": run_custom(args.custom, args.trials),
            "static_response_shapes": run_static(args.cli, args.trials),
            "caller_owned_h1": run_caller_owned(args.caller_owned),
        },
        "captured_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"output": str(args.output), "source_sha": args.sha}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

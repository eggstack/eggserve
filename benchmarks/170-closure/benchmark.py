#!/usr/bin/env python3
"""Plan 170 performance-closure harness.

This is a standard-library harness for same-machine evidence, not a CI test or
benchmark leaderboard. It drives the release CLI and streaming example,
current CPython ``http.server``, and an installed wheel. The Rust caller-owned
microbenchmark is a separately invoked ignored Cargo test.

Build the release binaries first. For Python workloads, pass ``--python`` for
an interpreter in a venv containing the installed wheel. TLS workloads require
``--tls-cert`` and ``--tls-key``.
"""

from __future__ import annotations

import argparse
import hashlib
import http.client
import json
import os
import platform
import random
import shutil
import socket
import statistics
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
OUT = Path(__file__).resolve().parent
DEFAULT_TRIALS = 3
STATIC_SIZES = (1024, 128 * 1024, 1024 * 1024)
STATIC_CONCURRENCY = (1, 16, 64, 256)


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def quantile(values: list[float], q: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    index = (len(ordered) - 1) * q
    lower = int(index)
    upper = min(lower + 1, len(ordered) - 1)
    return ordered[lower] + (ordered[upper] - ordered[lower]) * (index - lower)


def system_metadata() -> dict:
    cpu = "unknown"
    if Path("/proc/cpuinfo").exists():
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.lower().startswith("model name"):
                cpu = line.split(":", 1)[1].strip()
                break
    memory_gb = None
    if Path("/proc/meminfo").exists():
        for line in Path("/proc/meminfo").read_text().splitlines():
            if line.startswith("MemTotal:"):
                memory_gb = round(int(line.split()[1]) / 1024 / 1024, 2)
                break
    return {
        "os": platform.platform(),
        "arch": platform.machine(),
        "cpu": cpu,
        "logical_cpus": os.cpu_count(),
        "memory_gb": memory_gb,
        "rustc": command_output(["rustc", "--version"]),
        "python": sys.version,
        "python_executable": sys.executable,
        "client": "CPython stdlib http.client + threading",
    }


def command_output(command: list[str]) -> str | None:
    try:
        return subprocess.check_output(command, text=True).strip()
    except (OSError, subprocess.CalledProcessError):
        return None


def proc_sample(pid: int) -> dict:
    result: dict[str, int | float | None] = {}
    status = Path(f"/proc/{pid}/status")
    if status.exists():
        try:
            for line in status.read_text().splitlines():
                if line.startswith("VmRSS:"):
                    result["rss_kb"] = int(line.split()[1])
                elif line.startswith("VmHWM:"):
                    result["peak_rss_kb"] = int(line.split()[1])
                elif line.startswith("Threads:"):
                    result["threads"] = int(line.split()[1])
        except OSError:
            pass
    fds = Path(f"/proc/{pid}/fd")
    try:
        result["fds"] = len(list(fds.iterdir()))
    except OSError:
        pass
    stat = Path(f"/proc/{pid}/stat")
    try:
        fields = stat.read_text().split()
        ticks = os.sysconf(os.sysconf_names["SC_CLK_TCK"])
        result["cpu_time_s"] = (int(fields[13]) + int(fields[14])) / ticks
    except (OSError, ValueError, IndexError):
        pass
    return result


class ServerProcess:
    def __init__(self, command: list[str], env: dict[str, str] | None = None, bind_flag: bool = True):
        self.port = free_port()
        bind = ["--bind", f"127.0.0.1:{self.port}"] if bind_flag else [f"127.0.0.1:{self.port}"]
        self.command = [*command, *bind]
        merged = os.environ.copy()
        if env:
            merged.update(env)
        self.process = subprocess.Popen(
            self.command,
            env=merged,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

    def wait_ready(self, tls: bool = False) -> None:
        context = None
        if tls:
            import ssl

            context = ssl._create_unverified_context()
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError(f"server exited during startup: {self.command}")
            try:
                conn = make_connection(self.port, tls, context)
                conn.request("GET", "/", headers={"Connection": "close"})
                response = conn.getresponse()
                response.read()
                conn.close()
                return
            except (OSError, http.client.HTTPException):
                time.sleep(0.05)
        raise RuntimeError(f"server did not become ready: {self.command}")

    def sample(self) -> dict:
        return proc_sample(self.process.pid)

    def close(self) -> None:
        if self.process.poll() is None:
            self.process.terminate()
        try:
            self.process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=10)


def make_connection(port: int, tls: bool, context=None):
    if tls:
        if context is None:
            import ssl

            context = ssl._create_unverified_context()
        return http.client.HTTPSConnection("127.0.0.1", port, timeout=30, context=context)
    return http.client.HTTPConnection("127.0.0.1", port, timeout=30)


def requests_for(size: int, concurrency: int) -> int:
    target = {1024: 3000, 128 * 1024: 600, 1024 * 1024: 120}.get(size, 64)
    return max(1, (target + concurrency - 1) // concurrency)


def load_trial(port: int, path: str, expected_size: int, concurrency: int, tls=False) -> dict:
    count = requests_for(expected_size, concurrency)
    start_event = threading.Event()
    errors: list[str] = []
    latencies: list[float] = []
    lock = threading.Lock()

    def worker() -> None:
        conn = None
        try:
            conn = make_connection(port, tls)
            start_event.wait()
            for _ in range(count):
                begin = time.perf_counter()
                conn.request("GET", path, headers={"Connection": "keep-alive"})
                response = conn.getresponse()
                body = response.read()
                elapsed = (time.perf_counter() - begin) * 1000
                with lock:
                    latencies.append(elapsed)
                if response.status != 200 or len(body) != expected_size:
                    with lock:
                        errors.append(f"status={response.status},bytes={len(body)}")
        except Exception as exc:  # noqa: BLE001 - benchmark errors are data
            with lock:
                errors.append(f"{type(exc).__name__}: {exc}")
        finally:
            if conn is not None:
                conn.close()

    workers = [threading.Thread(target=worker) for _ in range(concurrency)]
    for worker_thread in workers:
        worker_thread.start()
    started = time.perf_counter()
    start_event.set()
    for worker_thread in workers:
        worker_thread.join()
    elapsed = time.perf_counter() - started
    requests = len(latencies)
    return {
        "requests": requests,
        "requested_per_worker": count,
        "elapsed_s": elapsed,
        "rps": requests / elapsed if elapsed else 0,
        "bytes_per_s": requests * expected_size / elapsed if elapsed else 0,
        "p50_latency_ms": quantile(latencies, 0.50),
        "p95_latency_ms": quantile(latencies, 0.95),
        "p99_latency_ms": quantile(latencies, 0.99),
        "errors": errors[:10],
        "error_count": len(errors),
    }


def benchmark_process(process: ServerProcess, cases: list[tuple[str, int, int]], trials: int, tls=False) -> list[dict]:
    process.wait_ready(tls=tls)
    # Warm-up is deliberately excluded from measured trials.
    for path, size, concurrency in cases:
        load_trial(process.port, path, size, min(concurrency, 16), tls=tls)
    records = []
    for path, size, concurrency in cases:
        trial_records = []
        for trial in range(1, trials + 1):
            before = process.sample()
            measurement = load_trial(process.port, path, size, concurrency, tls=tls)
            after = process.sample()
            measurement.update({
                "trial": trial,
                "path": path,
                "response_size": size,
                "concurrency": concurrency,
                "connection_reuse": "one keep-alive connection per worker",
                "rss_before": before.get("rss_kb"),
                "rss_after": after.get("rss_kb"),
                "peak_rss_kb": after.get("peak_rss_kb"),
                "fds_after": after.get("fds"),
                "threads_after": after.get("threads"),
                "cpu_time_delta_s": (
                    after.get("cpu_time_s", 0) - before.get("cpu_time_s", 0)
                ),
            })
            trial_records.append(measurement)
        records.append({
            "path": path,
            "response_size": size,
            "concurrency": concurrency,
            "trials": trial_records,
            "median_rps": statistics.median(t["rps"] for t in trial_records),
            "rps_spread": [min(t["rps"] for t in trial_records), max(t["rps"] for t in trial_records)],
            "total_errors": sum(t["error_count"] for t in trial_records),
        })
    return records


def static_command(cli: Path, root: Path, tls_cert: Path | None, tls_key: Path | None) -> list[str]:
    command = [str(cli), "--directory", str(root), "--log-format", "none",
               "--max-connections", "512", "--max-file-streams", "512",
               "--max-in-flight-requests", "512", "--max-buf-size", "65536",
               "--max-headers", "100", "--max-header-bytes", "32768",
               "--max-request-target-bytes", "8192"]
    if tls_cert:
        command += ["--tls-cert", str(tls_cert), "--tls-key", str(tls_key)]
    return command


def run_static(cli: Path, trials: int, tls_cert: Path | None, tls_key: Path | None) -> tuple[list[dict], Path]:
    root = Path(tempfile.mkdtemp(prefix="eggserve-170-static-"))
    try:
        for size, name in ((1024, "f1k.bin"), (128 * 1024, "f128k.bin"), (1024 * 1024, "f1m.bin")):
            (root / name).write_bytes(bytes([size // 1024 % 251]) * size)
        cases = [(f"/{name}", size, concurrency)
                 for size, name in ((1024, "f1k.bin"), (128 * 1024, "f128k.bin"), (1024 * 1024, "f1m.bin"))
                 for concurrency in STATIC_CONCURRENCY]
        process = ServerProcess(static_command(cli, root, tls_cert, tls_key))
        try:
            return benchmark_process(process, cases, trials, tls=bool(tls_cert)), root
        finally:
            process.close()
    except Exception:
        shutil.rmtree(root, ignore_errors=True)
        raise


def run_custom(streaming: Path, trials: int) -> list[dict]:
    cases = [
        (path, size, concurrency)
        for path, size in (
            ("/bytes/1024", 1024),
            ("/known/1048576", 1024 * 1024),
            ("/stream/1048576", 1024 * 1024),
            ("/stream/16777216", 16 * 1024 * 1024),
        )
        for concurrency in (16, 64)
    ]
    process = ServerProcess([str(streaming)], {
        "EGGSERVE_BENCH_MAX_CONNECTIONS": "128",
        "EGGSERVE_BENCH_MAX_IN_FLIGHT": "128",
    }, bind_flag=False)
    try:
        return benchmark_process(process, cases, trials)
    finally:
        process.close()


def run_slow_reader(streaming: Path) -> dict:
    process = ServerProcess([str(streaming)], {
        "EGGSERVE_BENCH_MAX_CONNECTIONS": "128",
        "EGGSERVE_BENCH_MAX_IN_FLIGHT": "128",
    }, bind_flag=False)
    process.wait_ready()
    errors: list[str] = []
    lock = threading.Lock()

    def one() -> None:
        try:
            conn = http.client.HTTPConnection("127.0.0.1", process.port, timeout=30)
            conn.request("GET", "/stream/16777216", headers={"Connection": "close"})
            response = conn.getresponse()
            response.read(1)
            time.sleep(0.25)
            body = response.read()
            conn.close()
            if response.status != 200 or len(body) + 1 != 16 * 1024 * 1024:
                with lock: errors.append(f"status={response.status},bytes={len(body) + 1}")
        except Exception as exc:  # noqa: BLE001 - benchmark errors are data
            with lock: errors.append(f"{type(exc).__name__}: {exc}")

    workers = [threading.Thread(target=one) for _ in range(4)]
    before = process.sample()
    for worker in workers: worker.start()
    for worker in workers: worker.join()
    after = process.sample()
    recovery = load_trial(process.port, "/bytes/1024", 1024, 16)
    process.close()
    return {"slow_readers": 4, "response_size": 16 * 1024 * 1024,
            "pause_s": 0.25, "errors": errors, "error_count": len(errors),
            "rss_before": before.get("rss_kb"), "rss_after": after.get("rss_kb"),
            "peak_rss_kb": after.get("peak_rss_kb"), "fds_after": after.get("fds"),
            "recovery": {"requests": recovery["requests"], "errors": recovery["error_count"]}}


def run_python_lowlevel(python: Path, trials: int) -> list[dict]:
    script = r'''
import signal, sys, time
from eggserve import lowlevel
port = int(sys.argv[1])
callbacks = int(sys.argv[2])
def stream(size):
    for offset in range(0, size, 8192):
        yield b"x" * min(8192, size - offset)
def handler(request):
    if request.path == "/bytes/1024":
        return lowlevel.Response.bytes(200, b"x" * 1024)
    if request.path == "/stream/1048576":
        return lowlevel.Response.stream(200, stream(1048576), content_length=1048576)
    return lowlevel.Response.text(404, "not found")
server = lowlevel.Server(lowlevel.RuntimeConfig(
    bind="127.0.0.1", port=port, max_connections=128,
    max_python_callbacks=callbacks, max_in_flight_requests=128), handler)
server.start(); server.wait_ready(); print("READY", flush=True)
signal.pause()
'''
    all_records = []
    with tempfile.NamedTemporaryFile("w", suffix=".py", delete=False) as file:
        file.write(script)
        script_path = file.name
    try:
        for callbacks in (8, 16):
            port = free_port()
            process = subprocess.Popen([str(python), script_path, str(port), str(callbacks)],
                                       stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
            wrapper = ServerProcess.__new__(ServerProcess)
            wrapper.port = port
            wrapper.command = [str(python), script_path]
            wrapper.process = process
            try:
                cases = [("/bytes/1024", 1024, callbacks), ("/stream/1048576", 1024 * 1024, callbacks)]
                for record in benchmark_process(wrapper, cases, trials):
                    record["max_python_callbacks"] = callbacks
                    all_records.append(record)
            finally:
                if process.poll() is None:
                    process.send_signal(getattr(__import__("signal"), "SIGTERM"))
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill(); process.wait(timeout=10)
        return all_records
    finally:
        os.unlink(script_path)


def run_python_saturation(python: Path) -> dict:
    script = r'''
import signal, sys, time
from eggserve import lowlevel
port = int(sys.argv[1])
def handler(request):
    time.sleep(0.15)
    return lowlevel.Response.text(200, "ok")
server = lowlevel.Server(lowlevel.RuntimeConfig(
    bind="127.0.0.1", port=port, max_connections=128,
    max_python_callbacks=8, max_in_flight_requests=8), handler)
server.start(); server.wait_ready(); print("READY", flush=True); signal.pause()
'''
    with tempfile.NamedTemporaryFile("w", suffix=".py", delete=False) as file:
        file.write(script); script_path = file.name
    port = free_port()
    process = subprocess.Popen([str(python), script_path, str(port)], stdout=subprocess.PIPE,
                               stderr=subprocess.DEVNULL, text=True)
    try:
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError("Python saturation server exited")
            if process.stdout and process.stdout.readline().strip() == "READY":
                break
        statuses: list[int] = []
        start = threading.Event()
        def one() -> None:
            start.wait()
            try:
                conn = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
                conn.request("GET", "/")
                response = conn.getresponse(); response.read(); statuses.append(response.status); conn.close()
            except Exception:
                statuses.append(0)
        workers = [threading.Thread(target=one) for _ in range(64)]
        for worker in workers: worker.start()
        start.set()
        for worker in workers: worker.join()
        recovery = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        recovery.request("GET", "/"); recovery_response = recovery.getresponse(); recovery_response.read(); recovery.close()
        return {"concurrency": 64, "max_in_flight_requests": 8,
                "statuses": {str(status): statuses.count(status) for status in sorted(set(statuses))},
                "recovery_status": recovery_response.status,
                "rejected_or_failed": sum(status != 200 for status in statuses)}
    finally:
        if process.poll() is None: process.terminate()
        try: process.wait(timeout=10)
        except subprocess.TimeoutExpired: process.kill(); process.wait(timeout=10)
        os.unlink(script_path)


def run_cpython_baseline(python: Path, root: Path, trials: int) -> list[dict]:
    cases = [(f"/{name}", size, concurrency)
             for size, name in ((1024, "f1k.bin"), (1024 * 1024, "f1m.bin"))
             for concurrency in (1, 16, 64)]
    process = ServerProcess.__new__(ServerProcess)
    process.port = free_port()
    process.command = [str(python), "-m", "http.server", str(process.port),
                       "--directory", str(root), "--bind", "127.0.0.1"]
    process.process = subprocess.Popen(process.command, stdout=subprocess.DEVNULL,
                                       stderr=subprocess.DEVNULL)
    try:
        return benchmark_process(process, cases, trials)
    finally:
        process.close()


def run_tls_handshake_churn(cli: Path, root: Path, cert: Path, key: Path, trials: int) -> list[dict]:
    process = ServerProcess(static_command(cli, root, cert, key))
    process.wait_ready(tls=True)
    records = []
    try:
        for trial in range(1, trials + 1):
            requests = 48
            start = time.perf_counter()
            errors: list[str] = []
            lock = threading.Lock()

            def one() -> None:
                try:
                    conn = make_connection(process.port, True)
                    conn.request("GET", "/f1k.bin", headers={"Connection": "close"})
                    response = conn.getresponse(); body = response.read(); conn.close()
                    if response.status != 200 or len(body) != 1024:
                        with lock: errors.append(f"status={response.status},bytes={len(body)}")
                except Exception as exc:  # noqa: BLE001 - benchmark errors are data
                    with lock: errors.append(f"{type(exc).__name__}: {exc}")

            workers = [threading.Thread(target=one) for _ in range(requests)]
            for worker in workers: worker.start()
            for worker in workers: worker.join()
            elapsed = time.perf_counter() - start
            records.append({"trial": trial, "new_connections": requests,
                            "elapsed_s": elapsed, "handshakes_per_s": requests / elapsed,
                            "errors": errors[:10], "error_count": len(errors),
                            "server_resources": process.sample()})
    finally:
        process.close()
    return records


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cli", type=Path, default=ROOT / "target/release/eggserve")
    parser.add_argument("--streaming", type=Path, default=ROOT / "target/release/examples/streaming_service")
    parser.add_argument("--python", type=Path, help="Python interpreter with the installed wheel")
    parser.add_argument("--tls-cert", type=Path)
    parser.add_argument("--tls-key", type=Path)
    parser.add_argument("--trials", type=int, default=DEFAULT_TRIALS)
    parser.add_argument("--output", type=Path, default=OUT / "results.json")
    parser.add_argument("--skip-tls", action="store_true")
    args = parser.parse_args()
    if args.trials < 3:
        parser.error("Plan 170 requires at least three measured trials")
    if not args.cli.exists() or not args.streaming.exists():
        parser.error("build the release CLI and streaming_service example first")
    if bool(args.tls_cert) != bool(args.tls_key) and not args.skip_tls:
        parser.error("--tls-cert and --tls-key must be supplied together")

    lock_hash = hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest()
    result = {
        "schema_version": 1,
        "plan": "170",
        "source_sha": command_output(["git", "rev-parse", "HEAD"]),
        "cargo_lock_sha256": lock_hash,
        "captured_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "environment": system_metadata(),
        "profiles": {
            "native_static": {"profile": "release", "features": [], "build_command": "cargo build --release --locked -p eggserve-bin", "runtime_limits": {"max_connections": 512, "max_file_streams": 512, "max_in_flight_requests": 512, "max_buf_size": 65536, "max_headers": 100, "max_header_bytes": 32768, "max_request_target_bytes": 8192}},
            "native_custom": {"profile": "release", "features": [], "build_command": "cargo build --release --locked --example streaming_service -p eggserve-core", "runtime_limits": {"max_connections": 128, "max_in_flight_requests": 128}},
            "python_lowlevel": {"build_command": "python3.14 -m maturin build --profile dist --interpreter python3.14", "installed_wheel": str(args.python) if args.python else None, "python_version": command_output([str(args.python), "--version"]) if args.python else None, "runtime_limits": {"max_connections": 128, "max_python_callbacks": 8, "max_in_flight_requests": 128}},
        },
        "method": {"trials": args.trials, "warmup": "one excluded trial at min(concurrency, 16) workers", "absolute_timing_ci_gate": False},
        "workloads": {},
    }
    static, root = run_static(args.cli, args.trials, None, None)
    result["workloads"]["native_static_http1"] = static
    result["workloads"]["native_custom_service"] = run_custom(args.streaming, args.trials)
    result["workloads"]["slow_reader_resource_recovery"] = run_slow_reader(args.streaming)
    if args.python:
        result["workloads"]["python_lowlevel"] = run_python_lowlevel(args.python, args.trials)
        result["workloads"]["admission_saturation"] = run_python_saturation(args.python)
        result["workloads"]["cpython_http_server_substitution"] = run_cpython_baseline(args.python, root, args.trials)
    else:
        result["workloads"]["python_lowlevel"] = {"status": "not-run", "reason": "pass --python for an installed wheel interpreter"}
        result["workloads"]["admission_saturation"] = {"status": "not-run"}
        result["workloads"]["cpython_http_server_substitution"] = {"status": "not-run"}
    if args.tls_cert and not args.skip_tls:
        tls, tls_root = run_static(args.cli, args.trials, args.tls_cert, args.tls_key)
        result["workloads"]["native_tls_established_keepalive"] = tls
        result["workloads"]["native_tls_handshake_churn"] = run_tls_handshake_churn(
            args.cli, tls_root, args.tls_cert, args.tls_key, args.trials
        )
        shutil.rmtree(tls_root, ignore_errors=True)
    else:
        result["workloads"]["native_tls_established_keepalive"] = {"status": "not-run", "reason": "supply certificate/key"}
        result["workloads"]["native_tls_handshake_churn"] = {"status": "not-run", "reason": "supply certificate/key"}
    result["notes"] = [
        "Numbers are same-machine evidence, not cross-machine or CI gates.",
        "Native static and CPython workloads use the same temporary files and client patterns; ratios are migration context only.",
        "Caller-owned duplex results are produced by the ignored Rust benchmark test documented in benchmarks/170-closure/README.md.",
        "Arm64 was not available in this capture; portability remains correctness-qualified, not performance-qualified.",
    ]
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"output": str(args.output), "source_sha": result["source_sha"], "workload_families": list(result["workloads"])}, indent=2))
    shutil.rmtree(root, ignore_errors=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

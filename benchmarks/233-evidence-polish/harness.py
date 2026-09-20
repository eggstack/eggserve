#!/usr/bin/env python3
"""Plan 233 evidence-polish harness (benchmark-only, stdlib only).

Re-runs the Plan 232 64 KiB vs 128 KiB native matrix, exact range probes,
and representative TLS cases with compact per-trial JSON retention.

Nothing here is a CI timing gate and nothing touches production code. The
chunk regime under measurement comes from the release binary under test; this
script only labels it (``--chunk-bytes``) and records the source identity
(SHA, lockfile hashes, temporary-diff hash for the 64 KiB override).

Outputs (under ``--output-dir``)::

    raw/native-<64k|128k>-trials.json
    raw/ranges-<64k|128k>-trials.json
    raw/tls-established-trials.json   (only with --tls-cert/--tls-key)
    raw/tls-handshake-trials.json     (only with --tls-cert/--tls-key)

Environment identity is written separately by ``environment.py`` so the raw
trial files stay deterministic (no timestamps inside trial rows).
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
import sys
import tempfile
import threading
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
OUT = Path(__file__).resolve().parent
NATIVE_CLIENT_SRC = ROOT / "benchmarks/227-current-head/native_client.rs"

NATIVE_CASES = [
    (1024, "f1k.bin", (1, 16, 64)),
    (128 * 1024, "f128k.bin", (1, 16, 64)),
    (1024 * 1024, "f1m.bin", (1, 16, 64)),
    (16 * 1024 * 1024, "f16m.bin", (16, 64)),
]
NATIVE_REQUEST_BUDGET = {1024: 3000, 128 * 1024: 600, 1024 * 1024: 120, 16 * 1024 * 1024: 8}
RANGE_FILE_SIZE = 4 * 1024 * 1024
RANGE_CASES = [(64 * 1024, (1, 16, 64)), (512 * 1024, (1, 16, 64))]
RANGE_REQUEST_BUDGET = {64 * 1024: 400, 512 * 1024: 120}
TLS_CASES = [(1024, "f1k.bin", (1, 16, 64)), (1024 * 1024, "f1m.bin", (1, 16, 64))]
HANDSHAKE_CONNECTIONS = 48


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def command_output(command: list[str]) -> str | None:
    try:
        return subprocess.check_output(command, text=True, cwd=str(ROOT)).strip()
    except (OSError, subprocess.CalledProcessError):
        return None


def proc_sample(pid: int) -> dict:
    result: dict[str, int | float | None] = {}
    try:
        for line in Path(f"/proc/{pid}/status").read_text().splitlines():
            if line.startswith("VmRSS:"):
                result["rss_kb"] = int(line.split()[1])
            elif line.startswith("VmHWM:"):
                result["peak_rss_kb"] = int(line.split()[1])
            elif line.startswith("Threads:"):
                result["threads"] = int(line.split()[1])
    except OSError:
        pass
    try:
        result["fds"] = len(list(Path(f"/proc/{pid}/fd").iterdir()))
    except OSError:
        pass
    try:
        fields = Path(f"/proc/{pid}/stat").read_text().split()
        ticks = os.sysconf(os.sysconf_names["SC_CLK_TCK"])
        result["cpu_time_s"] = (int(fields[13]) + int(fields[14])) / ticks
    except (OSError, ValueError, IndexError):
        pass
    return result


class ServerProcess:
    def __init__(self, command: list[str]):
        self.port = free_port()
        self.command = [*command, "--bind", f"127.0.0.1:{self.port}"]
        self.process = subprocess.Popen(
            self.command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
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
                if tls:
                    conn = http.client.HTTPSConnection(
                        "127.0.0.1", self.port, timeout=5, context=context
                    )
                else:
                    conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=5)
                conn.request("GET", "/", headers={"Connection": "close"})
                conn.getresponse().read()
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


def base_command(cli: Path, root: Path, tls_cert: Path | None, tls_key: Path | None) -> list[str]:
    command = [
        str(cli), "--directory", str(root), "--log-format", "none",
        "--max-connections", "512", "--max-file-streams", "512",
        "--max-in-flight-requests", "512", "--max-buf-size", "65536",
        "--max-headers", "100", "--max-header-bytes", "32768",
        "--max-request-target-bytes", "8192",
    ]
    if tls_cert:
        command += ["--tls-cert", str(tls_cert), "--tls-key", str(tls_key)]
    return command


def write_static_files(root: Path) -> dict[str, bytes]:
    contents: dict[str, bytes] = {}
    for size, name in (
        (1024, "f1k.bin"), (128 * 1024, "f128k.bin"),
        (1024 * 1024, "f1m.bin"), (16 * 1024 * 1024, "f16m.bin"),
    ):
        data = bytes([(size // 1024 + i // 65536) % 251 for i in range(size)])
        (root / name).write_bytes(data)
        contents[name] = data
    range_data = bytes([(i // 1024) % 251 for i in range(RANGE_FILE_SIZE)])
    (root / "frange.bin").write_bytes(range_data)
    contents["frange.bin"] = range_data
    return contents


def run_native_case(client: Path, server: ServerProcess, path: str, size: int,
                    concurrency: int, trials: int) -> dict:
    per_worker = max(1, NATIVE_REQUEST_BUDGET[size] // concurrency)
    # One excluded warm-up per case.
    subprocess.run(
        [str(client), "127.0.0.1", str(server.port), path, str(size),
         str(concurrency), str(per_worker)],
        check=True, text=True, capture_output=True,
    )
    trial_rows = []
    for trial in range(1, trials + 1):
        before = server.sample()
        completed = subprocess.run(
            [str(client), "127.0.0.1", str(server.port), path, str(size),
             str(concurrency), str(per_worker)],
            text=True, capture_output=True,
        )
        after = server.sample()
        if completed.returncode != 0:
            trial_rows.append({
                "trial": trial, "client_error": completed.stderr.strip()[-500:],
                "requests_completed": 0, "elapsed_s": None, "rps": 0.0,
                "bytes_per_s": 0.0, "p50_ms": None, "p95_ms": None,
                "p99_ms": None, "errors": ["native-client-failed"],
                "error_count": 1, "truncations": 0, "timeouts": 0,
            })
            continue
        record = json.loads(completed.stdout)
        trial_rows.append({
            "trial": trial,
            "requests_completed": record["requests"],
            "requests_expected": record["expected_requests"],
            "elapsed_s": record["elapsed_s"],
            "rps": record["rps"],
            "bytes_per_s": record["bytes_per_s"],
            "p50_ms": record["p50_latency_ms"],
            "p95_ms": record["p95_latency_ms"],
            "p99_ms": record["p99_latency_ms"],
            "rss_before_kb": before.get("rss_kb"),
            "rss_after_kb": after.get("rss_kb"),
            "peak_rss_kb": after.get("peak_rss_kb"),
            "fds_after": after.get("fds"),
            "threads_after": after.get("threads"),
            "cpu_delta_s": (
                (after.get("cpu_time_s") or 0) - (before.get("cpu_time_s") or 0)
            ),
            "errors": record["errors"][:10],
            "error_count": record["error_count"],
            "truncations": 0,
            "timeouts": 0,
        })
    rps_values = [row["rps"] for row in trial_rows]
    return {
        "path": path, "response_size": size, "concurrency": concurrency,
        "requests_per_worker": per_worker, "trials": trial_rows,
        "median_rps": statistics.median(rps_values),
        "rps_min": min(rps_values), "rps_max": max(rps_values),
        "total_errors": sum(row["error_count"] for row in trial_rows),
    }


def quantile(values: list[float], q: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    index = (len(ordered) - 1) * q
    lower = int(index)
    upper = min(lower + 1, len(ordered) - 1)
    return ordered[lower] + (ordered[upper] - ordered[lower]) * (index - lower)


def run_range_case(server: ServerProcess, expected: bytes, range_len: int,
                   concurrency: int, trials: int) -> dict:
    start = 12345 % (len(expected) - range_len)
    want = expected[start:start + range_len]
    per_worker = max(1, RANGE_REQUEST_BUDGET[range_len] // concurrency)

    def one_trial() -> dict:
        lock = threading.Lock()
        latencies: list[float] = []
        errors: list[str] = []
        exact = [0]

        def worker() -> None:
            try:
                conn = http.client.HTTPConnection("127.0.0.1", server.port, timeout=30)
                for _ in range(per_worker):
                    begin = time.perf_counter()
                    conn.request(
                        "GET", "/frange.bin",
                        headers={
                            "Range": f"bytes={start}-{start + range_len - 1}",
                            "Connection": "keep-alive",
                        },
                    )
                    response = conn.getresponse()
                    body = response.read()
                    elapsed = (time.perf_counter() - begin) * 1000
                    with lock:
                        latencies.append(elapsed)
                    content_length = response.getheader("Content-Length")
                    content_range = response.getheader("Content-Range")
                    if response.status != 206:
                        errors.append(f"status={response.status}")
                    elif content_length != str(range_len):
                        errors.append(f"content-length={content_length}")
                    elif content_range != f"bytes {start}-{start + range_len - 1}/{len(expected)}":
                        errors.append(f"content-range={content_range}")
                    elif body != want:
                        errors.append(f"bytes-mismatch len={len(body)}")
                    else:
                        exact[0] += 1
                conn.close()
            except Exception as exc:  # noqa: BLE001 - benchmark errors are data
                with lock:
                    errors.append(f"{type(exc).__name__}: {exc}")

        workers = [threading.Thread(target=worker) for _ in range(concurrency)]
        for worker_thread in workers:
            worker_thread.start()
        begun = time.perf_counter()
        for worker_thread in workers:
            worker_thread.join()
        elapsed_s = time.perf_counter() - begun
        total = len(latencies)
        return {
            "requests": total, "elapsed_s": elapsed_s,
            "rps": total / elapsed_s if elapsed_s else 0,
            "bytes_per_s": exact[0] * range_len / elapsed_s if elapsed_s else 0,
            "p50_ms": quantile(latencies, 0.50),
            "p95_ms": quantile(latencies, 0.95),
            "p99_ms": quantile(latencies, 0.99),
            "all_status_206": all(not err.startswith("status=") for err in errors),
            "all_exact_bytes": exact[0] == total and total > 0,
            "errors": errors[:10], "error_count": len(errors),
        }

    # One excluded warm-up per case.
    one_trial()
    trial_rows = []
    for trial in range(1, trials + 1):
        before = server.sample()
        measurement = one_trial()
        after = server.sample()
        measurement.update({
            "trial": trial,
            "rss_before_kb": before.get("rss_kb"),
            "rss_after_kb": after.get("rss_kb"),
            "peak_rss_kb": after.get("peak_rss_kb"),
            "fds_after": after.get("fds"),
            "threads_after": after.get("threads"),
        })
        trial_rows.append(measurement)
    rps_values = [row["rps"] for row in trial_rows]
    return {
        "range_start": start, "range_len": range_len,
        "representation_size": len(expected), "concurrency": concurrency,
        "requests_per_worker": per_worker, "trials": trial_rows,
        "median_rps": statistics.median(rps_values),
        "rps_min": min(rps_values), "rps_max": max(rps_values),
        "total_errors": sum(row["error_count"] for row in trial_rows),
        "all_exact": all(row["all_exact_bytes"] for row in trial_rows),
    }


def stdlib_load(port: int, path: str, size: int, concurrency: int,
                tls: bool, per_worker: int) -> dict:
    lock = threading.Lock()
    latencies: list[float] = []
    errors: list[str] = []

    def worker() -> None:
        try:
            if tls:
                import ssl

                conn = http.client.HTTPSConnection(
                    "127.0.0.1", port, timeout=30,
                    context=ssl._create_unverified_context(),
                )
            else:
                conn = http.client.HTTPConnection("127.0.0.1", port, timeout=30)
            for _ in range(per_worker):
                begin = time.perf_counter()
                conn.request("GET", path, headers={"Connection": "keep-alive"})
                response = conn.getresponse()
                body = response.read()
                elapsed = (time.perf_counter() - begin) * 1000
                with lock:
                    latencies.append(elapsed)
                if response.status != 200 or len(body) != size:
                    with lock:
                        errors.append(f"status={response.status},bytes={len(body)}")
            conn.close()
        except Exception as exc:  # noqa: BLE001 - benchmark errors are data
            with lock:
                errors.append(f"{type(exc).__name__}: {exc}")

    workers = [threading.Thread(target=worker) for _ in range(concurrency)]
    for worker_thread in workers:
        worker_thread.start()
    begun = time.perf_counter()
    for worker_thread in workers:
        worker_thread.join()
    elapsed_s = time.perf_counter() - begun
    total = len(latencies)
    return {
        "requests": total, "elapsed_s": elapsed_s,
        "rps": total / elapsed_s if elapsed_s else 0,
        "bytes_per_s": total * size / elapsed_s if elapsed_s else 0,
        "p50_ms": quantile(latencies, 0.50),
        "p95_ms": quantile(latencies, 0.95),
        "p99_ms": quantile(latencies, 0.99),
        "errors": errors[:10], "error_count": len(errors),
    }


def run_tls_established(server: ServerProcess, trials: int) -> list[dict]:
    server.wait_ready(tls=True)
    records = []
    for size, name, concurrencies in TLS_CASES:
        for concurrency in concurrencies:
            per_worker = max(1, {1024: 3000, 1024 * 1024: 120}[size] // concurrency)
            stdlib_load(server.port, f"/{name}", size, min(concurrency, 16), True, per_worker)
            trial_rows = []
            for trial in range(1, trials + 1):
                before = server.sample()
                measurement = stdlib_load(
                    server.port, f"/{name}", size, concurrency, True, per_worker
                )
                after = server.sample()
                measurement.update({
                    "trial": trial, "path": f"/{name}", "response_size": size,
                    "concurrency": concurrency,
                    "rss_before_kb": before.get("rss_kb"),
                    "rss_after_kb": after.get("rss_kb"),
                    "peak_rss_kb": after.get("peak_rss_kb"),
                    "fds_after": after.get("fds"),
                    "threads_after": after.get("threads"),
                    "cpu_delta_s": (
                        (after.get("cpu_time_s") or 0) - (before.get("cpu_time_s") or 0)
                    ),
                })
                trial_rows.append(measurement)
            rps_values = [row["rps"] for row in trial_rows]
            records.append({
                "path": f"/{name}", "response_size": size, "concurrency": concurrency,
                "requests_per_worker": per_worker, "trials": trial_rows,
                "median_rps": statistics.median(rps_values),
                "rps_min": min(rps_values), "rps_max": max(rps_values),
                "total_errors": sum(row["error_count"] for row in trial_rows),
            })
    return records


def run_tls_handshake(cli: Path, root: Path, cert: Path, key: Path, trials: int) -> list[dict]:
    rows = []
    for trial in range(1, trials + 1):
        server = ServerProcess(base_command(cli, root, cert, key))
        try:
            server.wait_ready(tls=True)
            lock = threading.Lock()
            errors: list[str] = []
            ok = [0]

            def one() -> None:
                try:
                    import ssl

                    conn = http.client.HTTPSConnection(
                        "127.0.0.1", server.port, timeout=30,
                        context=ssl._create_unverified_context(),
                    )
                    conn.request("GET", "/f1k.bin", headers={"Connection": "close"})
                    response = conn.getresponse()
                    body = response.read()
                    conn.close()
                    if response.status == 200 and len(body) == 1024:
                        with lock:
                            ok[0] += 1
                    else:
                        with lock:
                            errors.append(f"status={response.status},bytes={len(body)}")
                except Exception as exc:  # noqa: BLE001 - benchmark errors are data
                    with lock:
                        errors.append(f"{type(exc).__name__}: {exc}")

            workers = [threading.Thread(target=one) for _ in range(HANDSHAKE_CONNECTIONS)]
            begun = time.perf_counter()
            for worker_thread in workers:
                worker_thread.start()
            for worker_thread in workers:
                worker_thread.join()
            elapsed_s = time.perf_counter() - begun
            rows.append({
                "trial": trial, "attempted_connections": HANDSHAKE_CONNECTIONS,
                "successful_handshakes": ok[0], "elapsed_s": elapsed_s,
                "handshakes_per_s": ok[0] / elapsed_s if elapsed_s else 0,
                "failures": HANDSHAKE_CONNECTIONS - ok[0],
                "errors": errors[:10], "error_count": len(errors),
                "server_resources": server.sample(),
            })
        finally:
            server.close()
    return rows


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cli", type=Path, default=ROOT / "target/release/eggserve")
    parser.add_argument("--chunk-label", required=True, choices=("64k", "128k"))
    parser.add_argument("--chunk-bytes", required=True, type=int)
    parser.add_argument("--source-diff-sha256", default=None)
    parser.add_argument("--trials", type=int, default=3)
    parser.add_argument("--output-dir", type=Path, default=OUT)
    parser.add_argument("--tls-cert", type=Path, default=None)
    parser.add_argument("--tls-key", type=Path, default=None)
    parser.add_argument("--skip-native", action="store_true")
    parser.add_argument("--skip-ranges", action="store_true")
    parser.add_argument("--tls-only", action="store_true")
    args = parser.parse_args()
    if args.trials < 3:
        parser.error("Plan 233 requires at least three measured trials")
    if not args.cli.exists():
        parser.error("build target/release/eggserve first")
    if bool(args.tls_cert) != bool(args.tls_key):
        parser.error("--tls-cert and --tls-key must be supplied together")

    client = Path(tempfile.mkdtemp(prefix="eggserve-233-client-")) / "native_client"
    subprocess.run(
        ["rustc", "-O", str(NATIVE_CLIENT_SRC), "-o", str(client)], check=True
    )
    source_sha = command_output(["git", "rev-parse", "HEAD"])
    cargo_lock = hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest()
    raw_dir = args.output_dir / "raw"
    raw_dir.mkdir(parents=True, exist_ok=True)

    root = Path(tempfile.mkdtemp(prefix="eggserve-233-static-"))
    try:
        contents = write_static_files(root)
        identity = {
            "source_sha": source_sha,
            "cargo_lock_sha256": cargo_lock,
            "chunk_label": args.chunk_label,
            "chunk_bytes": args.chunk_bytes,
            "temporary_source_diff_sha256": args.source_diff_sha256,
        }
        if not args.tls_only:
            server = ServerProcess(base_command(args.cli, root, None, None))
            try:
                server.wait_ready()
                if not args.skip_native:
                    native_cases = [
                        run_native_case(
                            client, server, f"/{name}", size, concurrency, args.trials
                        )
                        for size, name, concurrencies in NATIVE_CASES
                        for concurrency in concurrencies
                    ]
                    (raw_dir / f"native-{args.chunk_label}-trials.json").write_text(
                        json.dumps({
                            "schema_version": 1, "plan": "233",
                            "workload": "native static HTTP/1 keep-alive",
                            "identity": identity,
                            "client": "native_client.rs rust-std-tcp",
                            "method": {
                                "trials": args.trials,
                                "warmup": "one excluded native-client run per case",
                            },
                            "cases": native_cases,
                        }, indent=2) + "\n"
                    )
                if not args.skip_ranges:
                    range_cases = [
                        run_range_case(
                            server, contents["frange.bin"], range_len,
                            concurrency, args.trials,
                        )
                        for range_len, concurrencies in RANGE_CASES
                        for concurrency in concurrencies
                    ]
                    (raw_dir / f"ranges-{args.chunk_label}-trials.json").write_text(
                        json.dumps({
                            "schema_version": 1, "plan": "233",
                            "workload": "exact range probes with throughput",
                            "identity": identity,
                            "client": "CPython stdlib http.client + threading",
                            "method": {
                                "trials": args.trials,
                                "warmup": "one excluded threaded run per case",
                            },
                            "cases": range_cases,
                        }, indent=2) + "\n"
                    )
            finally:
                server.close()
        if args.tls_cert:
            tls_server = ServerProcess(
                base_command(args.cli, root, args.tls_cert, args.tls_key)
            )
            try:
                established = run_tls_established(tls_server, args.trials)
            finally:
                tls_server.close()
            (raw_dir / "tls-established-trials.json").write_text(
                json.dumps({
                    "schema_version": 1, "plan": "233",
                    "workload": "TLS established keep-alive",
                    "identity": {**identity, "chunk_label": "128k",
                                 "chunk_bytes": 131072},
                    "client": "CPython stdlib http.client + threading",
                    "certificate": (
                        "ephemeral local RSA-2048: openssl req -x509 "
                        "-newkey rsa:2048 -nodes -days 1 -subj /CN=localhost"
                    ),
                    "method": {
                        "trials": args.trials,
                        "warmup": "one excluded threaded run per case",
                    },
                    "cases": established,
                }, indent=2) + "\n"
            )
            handshake = run_tls_handshake(
                args.cli, root, args.tls_cert, args.tls_key, args.trials
            )
            (raw_dir / "tls-handshake-trials.json").write_text(
                json.dumps({
                    "schema_version": 1, "plan": "233",
                    "workload": "TLS handshake churn",
                    "identity": {**identity, "chunk_label": "128k",
                                 "chunk_bytes": 131072},
                    "client": "CPython stdlib http.client + threading",
                    "new_connections_per_trial": HANDSHAKE_CONNECTIONS,
                    "trials": handshake,
                }, indent=2) + "\n"
            )
    finally:
        shutil_root = root
        try:
            client.unlink()
            client.parent.rmdir()
        except OSError:
            pass
        import shutil

        shutil.rmtree(shutil_root, ignore_errors=True)
    print(json.dumps({"output_dir": str(raw_dir), "chunk": args.chunk_label,
                      "source_sha": source_sha}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

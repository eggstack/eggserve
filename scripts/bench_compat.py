#!/usr/bin/env python3
"""Lightweight `http.server` compat benchmark for Plan 126.

Measures installed-wheel throughput / latency for representative static
requests through the compatibility facade. The same script is run against
two installed wheels:

- pre-fast-path baseline
- corrected current implementation

Results are emitted as JSON lines on stdout so they can be diffed across
benchmarks. The script intentionally uses only the Python standard
library so we don't introduce a benchmark dependency.
"""

from __future__ import annotations

import argparse
import functools
import http.client
import json
import os
import resource
import socket
import sys
import tempfile
import threading
import time
from pathlib import Path

from eggserve.server import SimpleHTTPRequestHandler, ThreadingHTTPServer


def _make_fixture(root: Path, size: int) -> None:
    payload = (b"x" * (size - 1)) + b"\n"
    (root / "small.txt").write_bytes(b"hello from rust\n")
    (root / "big.bin").write_bytes(payload)


def _start_server(root: Path, port: int, max_workers: int):
    handler = functools.partial(SimpleHTTPRequestHandler, directory=str(root))
    server = ThreadingHTTPServer(("127.0.0.1", port), handler, max_workers=max_workers)
    server._start()
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()

    class Holder:
        def __init__(self) -> None:
            self.server = server
            self.thread = thread
            self.address = server.server_address

        def stop(self) -> None:
            server.server_close()
            thread.join(5)

    return Holder()


def _request_once(host: str, port: int, method: str, target: str, headers: dict | None = None) -> tuple[int, int, bytes, float]:
    """Return ``(status, length, body, elapsed_seconds)``."""
    start = time.perf_counter()
    conn = http.client.HTTPConnection(host, port, timeout=5)
    conn.request(method, target, headers=headers or {})
    response = conn.getresponse()
    body = response.read()
    conn.close()
    elapsed = time.perf_counter() - start
    return response.status, len(body), body, elapsed


def _fetch_etag(host: str, port: int, target: str) -> str | None:
    conn = http.client.HTTPConnection(host, port, timeout=5)
    conn.request("GET", target)
    response = conn.getresponse()
    response.read()
    conn.close()
    return response.getheader("ETag")


def _run_bench(label: str, host: str, port: int, method: str, target: str,
                headers: dict | None, duration: float, concurrency: int) -> dict:
    deadline = time.monotonic() + duration
    stop = threading.Event()
    successes: list[float] = []
    errors = 0

    def worker() -> None:
        nonlocal errors
        local: list[float] = []
        while not stop.is_set():
            try:
                status, _, _, elapsed = _request_once(host, port, method, target, headers)
            except OSError:
                errors += 1
                continue
            if status < 200 or status >= 400:
                errors += 1
                continue
            local.append(elapsed)
        successes.extend(local)

    threads = [threading.Thread(target=worker, daemon=True) for _ in range(concurrency)]
    for t in threads:
        t.start()
    while time.monotonic() < deadline:
        time.sleep(0.05)
    stop.set()
    for t in threads:
        t.join(5)

    if not successes:
        return {"label": label, "requests": 0, "errors": errors}

    sorted_lat = sorted(successes)
    return {
        "label": label,
        "requests": len(successes),
        "errors": errors,
        "median_latency_ms": round(sorted_lat[len(sorted_lat) // 2] * 1000, 4),
        "p95_latency_ms": round(sorted_lat[int(len(sorted_lat) * 0.95)] * 1000, 4),
        "min_latency_ms": round(min(successes) * 1000, 4),
    }


def _measure_cpu() -> dict:
    usage = resource.getrusage(resource.RUSAGE_SELF)
    return {
        "user_cpu_seconds": round(usage.ru_utime, 3),
        "system_cpu_seconds": round(usage.ru_stime, 3),
        "max_rss_kb": usage.ru_maxrss,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--duration", type=float, default=3.0)
    parser.add_argument("--max-workers", type=int, default=64)
    parser.add_argument("--size", type=int, default=64 * 1024)
    parser.add_argument("--label", default="current")
    args = parser.parse_args()

    with tempfile.TemporaryDirectory(prefix="eggserve-bench-") as tmp:
        root = Path(tmp)
        _make_fixture(root, args.size)

        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            port = probe.getsockname()[1]

        holder = _start_server(root, port, args.max_workers)
        try:
            host, port = holder.address
            etag = _fetch_etag(host, port, "/small.txt")

            # Warm up
            for _ in range(8):
                _request_once(host, port, "GET", "/small.txt")

            scenarios = {
                "small_get": _run_bench("small_get", host, port, "GET", "/small.txt", None, args.duration, 8),
                "big_get": _run_bench("big_get", host, port, "GET", "/big.bin", None, args.duration, 8),
                "head": _run_bench("head", host, port, "HEAD", "/small.txt", None, args.duration, 8),
                "range": _run_bench("range", host, port, "GET", "/big.bin", {"Range": "bytes=0-1023"}, args.duration, 8),
                "conditional_304": _run_bench(
                    "conditional_304", host, port, "GET", "/small.txt",
                    {"If-None-Match": etag} if etag else None,
                    args.duration, 8,
                ),
                "moderate_concurrency": _run_bench(
                    "moderate_concurrency", host, port, "GET", "/small.txt",
                    None, args.duration, 32,
                ),
            }
        finally:
            holder.stop()

    output = {
        "label": args.label,
        "scenarios": scenarios,
        "process": _measure_cpu(),
    }
    print(json.dumps(output))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

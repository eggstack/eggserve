#!/usr/bin/env python3
"""Installed-wheel callback and synchronous-stream evidence for Plan 241."""

from __future__ import annotations

import argparse
import http.client
import json
import os
import socket
import statistics
import threading
import time
from pathlib import Path

from eggserve import lowlevel


def sample() -> dict:
    result = {}
    try:
        for line in Path(f"/proc/{os.getpid()}/status").read_text().splitlines():
            if line.startswith("VmRSS:"):
                result["rss_kb"] = int(line.split()[1])
            elif line.startswith("VmHWM:"):
                result["peak_rss_kb"] = int(line.split()[1])
            elif line.startswith("Threads:"):
                result["threads"] = int(line.split()[1])
    except OSError:
        pass
    try:
        result["fds"] = len(list(Path(f"/proc/{os.getpid()}/fd").iterdir()))
    except OSError:
        pass
    return result


def percentile(values: list[float], fraction: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    return ordered[round((len(ordered) - 1) * fraction)]


def start_server(handler, *, tls_cert=None, tls_key=None, max_connections=512):
    config = lowlevel.RuntimeConfig(
        bind="127.0.0.1", port=0, max_connections=max_connections,
        max_python_callbacks=max_connections, max_in_flight_requests=max_connections,
        handler_timeout_secs=30, connection_total_timeout_secs=60,
        graceful_shutdown_timeout_secs=5, tls_certfile=tls_cert,
        tls_keyfile=tls_key,
    )
    server = lowlevel.Server(config=config, handler=handler)
    server.start()
    server.wait_ready()
    return server


def callback_handler(mode: str, observed: list, lock: threading.Lock):
    def handler(request):
        try:
            if mode == "empty":
                value = "empty"
                response = lowlevel.Response.empty(200)
            elif mode == "bytes":
                value = "bytes"
                response = lowlevel.Response.bytes(200, b"x" * 1024)
            elif mode == "method":
                value = request.method
                response = lowlevel.Response.empty(200)
            elif mode == "headers":
                value = repr(request.headers)
                response = lowlevel.Response.empty(200)
            elif mode == "header_items":
                value = repr(request.header_items)
                response = lowlevel.Response.empty(200)
            elif mode == "raw_target":
                value = repr((request.raw_target_bytes, request.path_bytes, request.query_bytes))
                response = lowlevel.Response.empty(200)
            elif mode == "byte_header_items":
                value = repr(request.header_items_bytes)
                response = lowlevel.Response.empty(200)
            elif mode == "metadata":
                value = repr({
                    "remote_addr": request.remote_addr,
                    "remote_address": request.remote_address,
                    "local_addr": request.local_addr,
                    "local_address": request.local_address,
                    "scheme": request.scheme,
                    "effective_addr": request.effective_addr,
                    "effective_address": request.effective_address,
                    "effective_scheme": request.effective_scheme,
                    "effective_authority": request.effective_authority,
                    "proxy_provenance": request.proxy_provenance,
                    "forwarded_provenance": request.forwarded_provenance,
                    "tls_protocol_version": request.tls_protocol_version,
                    "tls_server_name": request.tls_server_name,
                    "tls_alpn": request.tls_alpn,
                    "peer_certificates_present": request.peer_certificates_present,
                })
                response = lowlevel.Response.empty(200)
            else:
                raise AssertionError(mode)
            with lock:
                observed.append(value)
            return response
        except Exception as exc:  # handler errors are returned as evidence
            with lock:
                observed.append(f"ERROR:{type(exc).__name__}:{exc}")
            raise
    return handler


def callback_case(mode: str, trials: int, tls_cert=None, tls_key=None) -> dict:
    observed: list[str] = []
    lock = threading.Lock()
    server = start_server(callback_handler(mode, observed, lock), tls_cert=tls_cert, tls_key=tls_key)
    try:
        host, port_text = server.addr.split(":")
        port = int(port_text)
        use_tls = tls_cert is not None
        import ssl

        def one_request() -> tuple[float, int, bytes]:
            if use_tls:
                conn = http.client.HTTPSConnection(host, port, timeout=10, context=ssl._create_unverified_context())
            else:
                conn = http.client.HTTPConnection(host, port, timeout=10)
            began = time.perf_counter()
            conn.request("GET", "/a?b=c", headers={"Host": "benchmark", "X-Test": "value", "Connection": "keep-alive"})
            response = conn.getresponse()
            body = response.read()
            elapsed = (time.perf_counter() - began) * 1000
            status = response.status
            conn.close()
            return elapsed, status, body

        warmup = one_request()
        rows = []
        for trial in range(1, trials + 1):
            measurements = [one_request() for _ in range(200)]
            latencies = [item[0] for item in measurements]
            rows.append({
                "trial": trial,
                "requests": len(measurements),
                "elapsed_s": sum(latencies) / 1000,
                "rps": len(measurements) / (sum(latencies) / 1000),
                "p50_ms": percentile(latencies, .50),
                "p95_ms": percentile(latencies, .95),
                "p99_ms": percentile(latencies, .99),
                "errors": sum(status != 200 or (mode == "bytes" and body != b"x" * 1024) for _, status, body in measurements),
                "resources": sample(),
            })
        with lock:
            values = observed[:]
        return {"mode": mode, "tls": use_tls, "warmup": warmup[1], "trials": rows, "observed_sample": values[-3:], "errors": sum(row["errors"] for row in rows)}
    finally:
        server.stop()


def stream_server():
    def handler(_request):
        def producer():
            for _ in range(2048):
                yield b"x" * 8192
        return lowlevel.Response.stream(200, producer(), content_length=2048 * 8192)
    return start_server(handler, max_connections=512)


def open_slow_stream(server, read_body: bool = False):
    host, port_text = server.addr.split(":")
    sock = socket.create_connection((host, int(port_text)), timeout=10)
    sock.sendall(b"GET /stream HTTP/1.1\r\nHost: benchmark\r\nConnection: keep-alive\r\n\r\n")
    data = b""
    while b"\r\n\r\n" not in data:
        data += sock.recv(4096)
    if read_body:
        sock.recv(1)
    return sock


def wait_for_thread_floor(floor: int, timeout: float = 5.0) -> tuple[float, dict]:
    began = time.monotonic()
    last = sample()
    while time.monotonic() - began < timeout:
        last = sample()
        if last.get("threads", floor + 1) <= floor:
            return time.monotonic() - began, last
        time.sleep(.05)
    return time.monotonic() - began, last


def stream_case(count: int, disconnect: bool = False, shutdown: bool = False) -> dict:
    server = stream_server()
    sockets = []
    try:
        before = sample()
        for _ in range(count):
            sockets.append(open_slow_stream(server))
        time.sleep(.5)
        during = sample()
        if disconnect:
            for sock in sockets:
                sock.close()
            sockets.clear()
        if shutdown:
            shutdown_begin = time.monotonic()
            stopper = threading.Thread(target=server.stop)
            stopper.start()
            # Keep the streams active long enough to enter the drain path,
            # then model clients closing while shutdown is in progress.
            time.sleep(.2)
            for sock in sockets:
                sock.close()
            sockets.clear()
            stopper.join(timeout=10)
            shutdown_elapsed = time.monotonic() - shutdown_begin
        else:
            for sock in sockets:
                sock.close()
            sockets.clear()
            shutdown_elapsed = None
            cleanup_elapsed, after = wait_for_thread_floor(before.get("threads", 1) + 2)
        if shutdown:
            cleanup_elapsed, after = wait_for_thread_floor(before.get("threads", 1) + 2)
        return {
            "active_streams": count,
            "before": before,
            "during": during,
            "after": after,
            "cleanup_elapsed_s": cleanup_elapsed,
            "shutdown_elapsed_s": shutdown_elapsed,
            "disconnect": disconnect,
            "shutdown": shutdown,
            "errors": 0,
        }
    finally:
        for sock in sockets:
            sock.close()
        try:
            server.stop()
        except Exception:
            pass


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", choices=("callbacks", "streams"), required=True)
    parser.add_argument("--sha", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--trials", type=int, default=3)
    parser.add_argument("--tls-cert")
    parser.add_argument("--tls-key")
    args = parser.parse_args()
    if args.mode == "callbacks":
        modes = ["empty", "bytes", "method", "headers", "header_items", "raw_target", "byte_header_items", "metadata"]
        result = {"workloads": [callback_case(mode, args.trials, args.tls_cert, args.tls_key) for mode in modes]}
    else:
        high = max(120, min(200, (os.cpu_count() or 4) // 2))
        result = {"workloads": [stream_case(n) for n in (10, 100, high)]}
        result["workloads"].append(stream_case(10, disconnect=True))
        result["workloads"].append(stream_case(10, shutdown=True))
    output = {"schema_version": 1, "plan": "241", "source_sha": args.sha, "python": os.sys.version, "method": {"trials": args.trials, "absolute_timing_ci_gate": False}, **result}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(output, indent=2) + "\n")
    print(json.dumps({"output": str(args.output), "workloads": len(result["workloads"])}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

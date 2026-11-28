#!/usr/bin/env python3
"""Bounded contention soak for the free-threaded CPython wheel (Plan 334).

This script is the documented substitute for a TSan-instrumented CPython:
no TSan-instrumented interpreter is available in the qualification
environment, and Rust-only TSan would not observe Python callback state.
It drives the public eggserve.lowlevel surface on loopback with a mixed
concurrent workload and writes one JSON record per run.

Scenarios per run, across two server phases (a stop/start cycle):

- concurrent buffered POSTs with a locked counter and payload digests
- concurrent Response.stream responses with exact byte verification
- one-shot RequestBody.read() races (exactly one winner)
- concurrent BodyChunkIterator.__next__ races (typed outcomes only)
- repeated tunnel upgrades with concurrent Tunnel.recv contention
- client disconnects during a slow streamed response

The script never logs request or response content. It exits non-zero on any
unexpected exception, content mismatch, invariant violation, GIL re-enable,
or request-budget shortfall.

Usage:

    python3.14t scripts/soak-python-free-threaded.py --self-test
    python3.14t scripts/soak-python-free-threaded.py \
        --requests 400 --workers 8 --duration 120 --json /tmp/soak-1.json
"""

import argparse
import hashlib
import http.client
import json
import platform
import queue
import resource
import socket
import sys
import sysconfig
import threading
import time

HTTP_SCENARIOS = ("post", "stream", "race", "iter")
SCENARIOS = HTTP_SCENARIOS + ("tunnel", "disconnect")
CHUNK_COUNT = 6
SLOW_CHUNK_COUNT = 64
SLOW_CHUNK_DELAY = 0.01
TUNNEL_RECV_SETTLE = 0.2

_lowlevel = None
_consumed_error = None


def _load_bindings():
    """Import the installed extension and cache the public symbols."""
    global _lowlevel, _consumed_error
    if _lowlevel is not None:
        return
    from eggserve import lowlevel
    from eggserve._native import RequestBodyConsumedError

    _lowlevel = lowlevel
    _consumed_error = RequestBodyConsumedError


class Stats:
    """Thread-safe counters for one soak run."""

    def __init__(self):
        self._lock = threading.Lock()
        self.http_requests = 0
        self.ok = 0
        self.fail = 0
        self.by_scenario = {name: {"ok": 0, "fail": 0} for name in SCENARIOS}
        self.errors = []

    def request_done(self):
        with self._lock:
            self.http_requests += 1

    def record(self, scenario, error=None):
        with self._lock:
            bucket = self.by_scenario[scenario]
            if error is None:
                self.ok += 1
                bucket["ok"] += 1
            else:
                self.fail += 1
                bucket["fail"] += 1
                if len(self.errors) < 25:
                    self.errors.append(
                        {"scenario": scenario, "error": str(error)[:300]}
                    )

    def snapshot(self):
        with self._lock:
            return {
                "http_requests": self.http_requests,
                "ok": self.ok,
                "fail": self.fail,
                "by_scenario": {
                    name: dict(bucket)
                    for name, bucket in self.by_scenario.items()
                },
                "errors": list(self.errors),
            }


class Concurrency:
    """Track observed concurrent entries without blocking."""

    def __init__(self):
        self._lock = threading.Lock()
        self.active = 0
        self.peak = 0

    def enter(self):
        with self._lock:
            self.active += 1
            if self.active > self.peak:
                self.peak = self.active

    def leave(self):
        with self._lock:
            self.active -= 1

    def snapshot(self):
        with self._lock:
            return {"active": self.active, "peak": self.peak}


def payload_for(index):
    """Deterministic request body for a request index."""
    seed = hashlib.sha256(("payload-%d" % index).encode("ascii")).digest()
    size = 32 + (index % 8) * 64
    return (seed * ((size // len(seed)) + 1))[:size]


def payload_digest(payload):
    return hashlib.sha256(payload).hexdigest()


def stream_chunks(index, count=CHUNK_COUNT):
    """Deterministic chunk list for a streamed response."""
    seed = hashlib.sha256(("stream-%d" % index).encode("ascii")).digest()
    chunks = []
    for offset in range(count):
        digest = hashlib.sha256(seed + bytes([offset % 256])).digest()
        size = 1 + (index + offset) % 97
        chunks.append((digest * ((size // len(digest)) + 1))[:size])
    return chunks


def stream_bytes(chunks):
    return b"".join(chunks)


def scenario_for(index):
    return HTTP_SCENARIOS[index % len(HTTP_SCENARIOS)]


def upgrade_request(path="/tunnel"):
    return (
        "GET %s HTTP/1.1\r\nHost: soak\r\nUpgrade: soak-echo\r\n"
        "Connection: upgrade\r\n\r\n" % path
    ).encode("ascii")


def disconnect_request(index):
    return (
        "GET /stream/slow/%d HTTP/1.1\r\nHost: soak\r\n"
        "Connection: close\r\n\r\n" % index
    ).encode("ascii")


def read_head(sock, limit=65536):
    """Read up to and including the response head terminator."""
    data = b""
    while b"\r\n\r\n" not in data:
        chunk = sock.recv(256)
        if not chunk:
            break
        data += chunk
        if len(data) > limit:
            raise RuntimeError("response head exceeded %d bytes" % limit)
    return data


def _gil_enabled():
    probe = getattr(sys, "_is_gil_enabled", None)
    if probe is None:
        return None
    return bool(probe())


def _peak_rss_bytes():
    usage = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    if sys.platform == "darwin":
        return int(usage)
    return int(usage) * 1024


def _interpreter():
    return {
        "implementation": sys.implementation.name,
        "version": platform.python_version(),
        "build": sys.version.replace("\n", " "),
        "platform": sysconfig.get_platform(),
        "machine": platform.machine(),
        "os": platform.platform(),
        "gil_disabled_build": bool(sysconfig.get_config_var("Py_GIL_DISABLED")),
        "debug_build": bool(sysconfig.get_config_var("Py_DEBUG")),
    }


class Phase:
    """One server lifetime with its own handler, queues, and counters."""

    def __init__(self, callbacks, max_body_bytes):
        _load_bindings()
        self.server_concurrency = Concurrency()
        self.client_concurrency = Concurrency()
        self.tunnels = queue.Queue()
        self.stats = Stats()
        config = _lowlevel.RuntimeConfig(
            port=0,
            request_body_mode="buffer",
            max_request_body_bytes=max_body_bytes,
            max_python_callbacks=callbacks,
        )
        self.server = _lowlevel.Server(config=config, handler=self.handle)
        self.host = None
        self.port = None

    def start(self):
        self.server.start()
        self.server.wait_ready()
        host, port = self.server.addr.split(":")
        self.host = host
        self.port = int(port)

    def stop(self):
        try:
            self.server.shutdown()
        except Exception:
            pass
        try:
            self.server.wait()
        except Exception:
            pass

    def handle(self, request):
        self.server_concurrency.enter()
        try:
            return self._dispatch(request)
        except BaseException as exc:
            return _lowlevel.Response.text(
                500, "handler-error:%s" % type(exc).__name__
            )
        finally:
            self.server_concurrency.leave()

    def _dispatch(self, request):
        path = request.path
        if path.startswith("/stream/slow/"):
            return self._slow_stream(path)
        if path.startswith("/stream/"):
            return self._stream(path)
        if path.startswith("/post"):
            return self._post(request)
        if path.startswith("/race"):
            return self._race(request)
        if path.startswith("/iter"):
            return self._iter(request)
        if path.startswith("/tunnel"):
            return self._tunnel(request)
        return _lowlevel.Response.text(404, "unknown")

    def _post(self, request):
        body = request.body.read()
        return _lowlevel.Response.text(200, payload_digest(body))

    def _stream(self, path):
        try:
            index = int(path.rsplit("/", 1)[1])
        except ValueError:
            return _lowlevel.Response.text(400, "bad-index")
        chunks = stream_chunks(index)
        return _lowlevel.Response.stream(
            200, chunks, content_length=len(stream_bytes(chunks))
        )

    def _slow_stream(self, path):
        try:
            index = int(path.rsplit("/", 1)[1])
        except ValueError:
            index = 0

        def generate():
            for offset in range(SLOW_CHUNK_COUNT):
                time.sleep(SLOW_CHUNK_DELAY)
                yield hashlib.sha256(
                    b"slow-%d-%d" % (index, offset)
                ).digest()

        return _lowlevel.Response.stream(200, generate())

    def _race(self, request):
        body = request.body
        barrier = threading.Barrier(2)
        results = []
        lock = threading.Lock()

        def read_once():
            barrier.wait(timeout=10)
            try:
                outcome = ("ok", body.read())
            except BaseException as exc:
                outcome = ("err", exc)
            with lock:
                results.append(outcome)

        threads = [
            threading.Thread(target=read_once, daemon=True) for _ in range(2)
        ]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join(15)
        if any(thread.is_alive() for thread in threads):
            return _lowlevel.Response.text(500, "race-timeout")
        wins = [value for kind, value in results if kind == "ok"]
        errors = [value for kind, value in results if kind == "err"]
        valid = (
            len(wins) == 1
            and len(errors) == 1
            and isinstance(errors[0], _consumed_error)
        )
        if not valid:
            return _lowlevel.Response.text(500, "race-invariant")
        return _lowlevel.Response.text(200, "ok")

    def _iter(self, request):
        iterator = request.body.iter_chunks()
        barrier = threading.Barrier(2)
        results = []
        lock = threading.Lock()

        def pull():
            barrier.wait(timeout=10)
            try:
                outcome = ("ok", next(iterator))
            except BaseException as exc:
                outcome = ("err", exc)
            with lock:
                results.append(outcome)

        threads = [
            threading.Thread(target=pull, daemon=True) for _ in range(2)
        ]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join(15)
        if any(thread.is_alive() for thread in threads):
            return _lowlevel.Response.text(500, "iterator-timeout")
        if not any(kind == "ok" for kind, _ in results):
            return _lowlevel.Response.text(500, "iterator-no-progress")
        for kind, value in results:
            if kind != "err" or isinstance(value, StopIteration):
                continue
            typed = isinstance(value, RuntimeError) and "borrow" in str(
                value
            ).lower()
            if not typed:
                return _lowlevel.Response.text(500, "iterator-untyped")
        return _lowlevel.Response.text(200, "ok")

    def _tunnel(self, request):
        capability = request.take_tunnel()
        if capability is None:
            return _lowlevel.Response.text(400, "no-tunnel")
        handshake, tunnel = capability.accept()
        self.tunnels.put(tunnel)
        return handshake


def _run_scenario(phase, scenario, index, stats):
    connection = http.client.HTTPConnection(
        phase.host, phase.port, timeout=20
    )
    try:
        if scenario == "post":
            payload = payload_for(index)
            connection.request(
                "POST", "/post", body=payload, headers={"Connection": "close"}
            )
            response = connection.getresponse()
            body = response.read()
            stats.request_done()
            if response.status != 200:
                raise RuntimeError("post status %d" % response.status)
            if body.decode("ascii", "replace") != payload_digest(payload):
                raise RuntimeError("post payload digest mismatch")
        elif scenario == "stream":
            chunks = stream_chunks(index)
            expected = stream_bytes(chunks)
            connection.request(
                "GET",
                "/stream/%d" % index,
                headers={"Connection": "close"},
            )
            response = connection.getresponse()
            body = response.read()
            stats.request_done()
            if response.status != 200:
                raise RuntimeError("stream status %d" % response.status)
            if body != expected:
                raise RuntimeError("stream bytes mismatch")
        elif scenario == "race":
            payload = payload_for(index)
            connection.request(
                "POST", "/race", body=payload, headers={"Connection": "close"}
            )
            response = connection.getresponse()
            body = response.read()
            stats.request_done()
            if response.status != 200 or body != b"ok":
                raise RuntimeError(
                    "race rejected: %d %s"
                    % (response.status, body[:40].decode("ascii", "replace"))
                )
        else:
            payload = payload_for(index)
            connection.request(
                "POST", "/iter", body=payload, headers={"Connection": "close"}
            )
            response = connection.getresponse()
            body = response.read()
            stats.request_done()
            if response.status != 200 or body != b"ok":
                raise RuntimeError(
                    "iterator rejected: %d %s"
                    % (response.status, body[:40].decode("ascii", "replace"))
                )
        stats.record(scenario)
    finally:
        connection.close()


def http_worker(phase, stats, stop, pace, index):
    while not stop.is_set():
        scenario = scenario_for(index)
        phase.client_concurrency.enter()
        started = time.monotonic()
        try:
            _run_scenario(phase, scenario, index, stats)
        except BaseException as exc:
            message = str(exc)[:200]
            stats.record(scenario, "%s: %s" % (type(exc).__name__, message))
        finally:
            phase.client_concurrency.leave()
        index += 1
        elapsed = time.monotonic() - started
        if elapsed < pace:
            time.sleep(pace - elapsed)


def _one_tunnel(host, port, phase, stats):
    sock = socket.create_connection((host, port), timeout=15)
    sock.settimeout(15)
    try:
        sock.sendall(upgrade_request())
        head = read_head(sock)
        status_line = head.split(b"\r\n", 1)[0]
        if b" 101 " not in status_line:
            raise RuntimeError(
                "upgrade rejected: %s"
                % status_line[:80].decode("ascii", "replace")
            )
        tunnel = phase.tunnels.get(timeout=15)
        started = threading.Event()
        holder = {}

        def blocking_recv():
            started.set()
            try:
                holder["value"] = tunnel.recv()
            except BaseException as exc:
                holder["error"] = exc

        worker = threading.Thread(target=blocking_recv, daemon=True)
        worker.start()
        if not started.wait(10):
            raise RuntimeError("blocking recv did not start")
        time.sleep(TUNNEL_RECV_SETTLE)
        try:
            tunnel.recv()
        except RuntimeError as exc:
            if "concurrent Tunnel.recv" not in str(exc):
                raise RuntimeError(
                    "unexpected contention error: %s" % str(exc)[:120]
                )
        else:
            raise RuntimeError("concurrent Tunnel.recv was accepted")
        sock.shutdown(socket.SHUT_RDWR)
        worker.join(15)
        if worker.is_alive():
            raise RuntimeError("blocking recv did not finish")
        try:
            tunnel.recv()
        except RuntimeError as exc:
            if "already closed" not in str(exc):
                raise RuntimeError(
                    "unexpected post-close error: %s" % str(exc)[:120]
                )
        else:
            raise RuntimeError("closed tunnel.recv returned data")
        if not holder:
            raise RuntimeError("blocking recv produced no result")
        stats.record("tunnel")
    finally:
        try:
            sock.close()
        except OSError:
            pass


def tunnel_worker(phase, stats, stop, pace):
    while not stop.is_set():
        started = time.monotonic()
        try:
            _one_tunnel(phase.host, phase.port, phase, stats)
        except BaseException as exc:
            message = str(exc)[:200]
            stats.record("tunnel", "%s: %s" % (type(exc).__name__, message))
        elapsed = time.monotonic() - started
        if elapsed < pace:
            time.sleep(pace - elapsed)


def _one_disconnect(host, port):
    sock = socket.create_connection((host, port), timeout=15)
    sock.settimeout(15)
    try:
        sock.sendall(disconnect_request(7))
        head = read_head(sock)
        status_line = head.split(b"\r\n", 1)[0]
        if b" 200 " not in status_line:
            raise RuntimeError(
                "slow stream status: %s"
                % status_line[:80].decode("ascii", "replace")
            )
    finally:
        sock.close()


def disconnect_worker(phase, stats, stop, pace):
    while not stop.is_set():
        started = time.monotonic()
        try:
            _one_disconnect(phase.host, phase.port)
            stats.record("disconnect")
        except BaseException as exc:
            message = str(exc)[:200]
            stats.record(
                "disconnect", "%s: %s" % (type(exc).__name__, message)
            )
        elapsed = time.monotonic() - started
        if elapsed < pace:
            time.sleep(pace - elapsed)


def run_phase(phase, stats, seconds, workers, pace, base):
    stop = threading.Event()
    threads = []
    for worker_index in range(workers):
        thread = threading.Thread(
            target=http_worker,
            args=(phase, stats, stop, pace, base + worker_index),
            name="soak-http-%d" % worker_index,
            daemon=True,
        )
        threads.append(thread)
    threads.append(
        threading.Thread(
            target=tunnel_worker,
            args=(phase, stats, stop, 0.05),
            name="soak-tunnel",
            daemon=True,
        )
    )
    threads.append(
        threading.Thread(
            target=disconnect_worker,
            args=(phase, stats, stop, 0.2),
            name="soak-disconnect",
            daemon=True,
        )
    )
    started = time.monotonic()
    for thread in threads:
        thread.start()
    stop.wait(seconds)
    stop.set()
    for thread in threads:
        thread.join(20)
    return {
        "seconds": time.monotonic() - started,
        "stuck_threads": [thread.name for thread in threads if thread.is_alive()],
    }


def _empty_record(args):
    return {
        "plan": "334",
        "tool": "soak-python-free-threaded",
        "started_at": None,
        "interpreter": _interpreter(),
        "gil": {
            "disabled_build": bool(sysconfig.get_config_var("Py_GIL_DISABLED")),
            "enabled_before": None,
            "enabled_after": None,
        },
        "config": {
            "requests_minimum": args.requests,
            "workers": args.workers,
            "duration_requested_seconds": args.duration,
            "max_python_callbacks": args.callbacks,
            "request_body_mode": "buffer",
            "max_request_body_bytes": args.max_body_bytes,
            "pace_ms": args.pace_ms,
        },
        "phases": [],
        "totals": None,
        "resources": {"peak_rss_bytes": None, "threads_at_end": None},
        "duration_seconds": None,
        "problems": [],
        "qualified": False,
    }


def run(args):
    record = _empty_record(args)
    record["started_at"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    record["gil"]["enabled_before"] = _gil_enabled()
    stats = Stats()
    started = time.monotonic()
    phase_seconds = max(1.0, args.duration / 2.0)
    base = 0
    for phase_index in range(2):
        phase = Phase(args.callbacks, args.max_body_bytes)
        phase.start()
        try:
            outcome = run_phase(
                phase, stats, phase_seconds, args.workers, args.pace_ms / 1000.0, base
            )
        finally:
            phase.stop()
        server_peak = phase.server_concurrency.snapshot()["peak"]
        record["phases"].append(
            {
                "index": phase_index,
                "seconds": round(outcome["seconds"], 3),
                "stuck_threads": outcome["stuck_threads"],
                "server_peak_concurrency": server_peak,
                "client_peak_concurrency": phase.client_concurrency.snapshot()[
                    "peak"
                ],
                "tunnel_queue_remaining": phase.tunnels.qsize(),
            }
        )
        if outcome["stuck_threads"]:
            record["problems"].append(
                "phase %d left worker threads running: %s"
                % (phase_index, ",".join(outcome["stuck_threads"]))
            )
        if server_peak > args.callbacks:
            record["problems"].append(
                "phase %d exceeded max_python_callbacks: %d > %d"
                % (phase_index, server_peak, args.callbacks)
            )
        base += 100000
        time.sleep(0.2)
    record["duration_seconds"] = round(time.monotonic() - started, 3)
    record["totals"] = stats.snapshot()
    record["resources"]["peak_rss_bytes"] = _peak_rss_bytes()
    record["resources"]["threads_at_end"] = threading.active_count()
    record["gil"]["enabled_after"] = _gil_enabled()
    if record["totals"]["fail"]:
        record["problems"].append(
            "%d recorded scenario failure(s)" % record["totals"]["fail"]
        )
    if record["totals"]["http_requests"] < args.requests:
        record["problems"].append(
            "request budget not met: %d < %d"
            % (record["totals"]["http_requests"], args.requests)
        )
    if record["gil"]["enabled_after"] is not False:
        record["problems"].append("GIL state after the run is not False")
    record["qualified"] = not record["problems"]
    return record


def validate_args(args):
    problems = []
    if args.requests < 1:
        problems.append("--requests must be at least 1")
    if not 1 <= args.workers <= 64:
        problems.append("--workers must be between 1 and 64")
    if args.duration < 2:
        problems.append("--duration must be at least 2 seconds")
    if not 1 <= args.callbacks <= 64:
        problems.append("--callbacks must be between 1 and 64")
    if args.pace_ms < 0:
        problems.append("--pace-ms must not be negative")
    if args.max_body_bytes < 1024:
        problems.append("--max-body-bytes must be at least 1024")
    return problems


def parse_args(argv=None):
    parser = argparse.ArgumentParser(
        description="Bounded free-threaded contention soak (Plan 334)."
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="run the bookkeeping and fixture self-tests, then exit",
    )
    parser.add_argument("--requests", type=int, default=400)
    parser.add_argument("--workers", type=int, default=8)
    parser.add_argument("--duration", type=float, default=120.0)
    parser.add_argument("--callbacks", type=int, default=8)
    parser.add_argument("--pace-ms", type=float, default=10.0)
    parser.add_argument("--max-body-bytes", type=int, default=1048576)
    parser.add_argument("--json", default=None)
    return parser.parse_args(argv)


def _valid_namespace():
    return argparse.Namespace(
        requests=400,
        workers=8,
        duration=120.0,
        callbacks=8,
        pace_ms=10.0,
        max_body_bytes=1048576,
        json=None,
        self_test=False,
    )


def self_test():
    """Exercise bookkeeping and fixtures without starting a server."""
    results = []

    def check(name, condition, detail=""):
        results.append((name, bool(condition), detail))

    stats = Stats()
    stats.request_done()
    stats.request_done()
    stats.record("post")
    stats.record("post", "boom")
    snapshot = stats.snapshot()
    check(
        "stats-counts",
        snapshot["http_requests"] == 2
        and snapshot["ok"] == 1
        and snapshot["fail"] == 1
        and snapshot["by_scenario"]["post"] == {"ok": 1, "fail": 1},
        repr(snapshot["by_scenario"]),
    )
    for _ in range(40):
        stats.record("stream", "x")
    check("stats-error-cap", len(stats.snapshot()["errors"]) == 25)

    concurrency = Concurrency()
    concurrency.enter()
    concurrency.enter()
    concurrency.leave()
    check(
        "concurrency-peak",
        concurrency.snapshot() == {"active": 1, "peak": 2},
        repr(concurrency.snapshot()),
    )

    payload = payload_for(7)
    check(
        "payload-deterministic",
        payload == payload_for(7) and len(payload) == 32 + (7 % 8) * 64,
        "len=%d" % len(payload),
    )
    check(
        "payload-digest",
        payload_digest(payload) == hashlib.sha256(payload).hexdigest(),
    )

    chunks = stream_chunks(3)
    check(
        "stream-chunks",
        chunks == stream_chunks(3)
        and len(chunks) == CHUNK_COUNT
        and stream_bytes(chunks) == b"".join(chunks)
        and len(stream_chunks(3, 1)) == 1,
    )

    check(
        "scenario-cycle",
        [scenario_for(index) for index in range(4)] == list(HTTP_SCENARIOS),
    )

    request = upgrade_request()
    check(
        "upgrade-request",
        request.startswith(b"GET /tunnel HTTP/1.1\r\n")
        and request.endswith(b"\r\n\r\n")
        and b"Upgrade: soak-echo" in request,
    )
    disconnect = disconnect_request(7)
    check(
        "disconnect-request",
        disconnect.startswith(b"GET /stream/slow/7 HTTP/1.1\r\n")
        and disconnect.endswith(b"\r\n\r\n"),
    )

    left, right = socket.socketpair()
    try:
        right.sendall(
            b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: x\r\n\r\n"
        )
        head = read_head(left)
        check(
            "read-head",
            head.split(b"\r\n", 1)[0] == b"HTTP/1.1 101 Switching Protocols",
        )
    finally:
        left.close()
        right.close()

    check("peak-rss-positive", _peak_rss_bytes() > 0)
    check("gil-probe", _gil_enabled() in (True, False, None))

    interpreter = _interpreter()
    check(
        "interpreter-shape",
        isinstance(interpreter["version"], str)
        and isinstance(interpreter["gil_disabled_build"], bool),
    )

    record = _empty_record(_valid_namespace())
    encoded = json.dumps(record, sort_keys=True)
    decoded = json.loads(encoded)
    required = {
        "plan",
        "tool",
        "interpreter",
        "gil",
        "config",
        "phases",
        "totals",
        "resources",
        "problems",
        "qualified",
    }
    check("record-shape", required.issubset(decoded))

    namespace = _valid_namespace()
    check("validate-accepts", validate_args(namespace) == [])
    namespace.requests = 0
    namespace.workers = 0
    namespace.callbacks = 0
    namespace.duration = 1
    namespace.pace_ms = -1
    namespace.max_body_bytes = 1
    check("validate-rejects", len(validate_args(namespace)) == 6)

    failures = [name for name, ok, _ in results if not ok]
    for name, ok, detail in results:
        print("%s %s%s" % ("ok  " if ok else "FAIL", name, "" if ok else ": " + detail))
    print("self-test: %d checks, %d failure(s)" % (len(results), len(failures)))
    if failures:
        print("failed: " + ", ".join(failures), file=sys.stderr)
        return 1
    return 0


def main(argv=None):
    args = parse_args(argv)
    if args.self_test:
        return self_test()
    problems = validate_args(args)
    if problems:
        for problem in problems:
            print("error: " + problem, file=sys.stderr)
        return 2
    if not sysconfig.get_config_var("Py_GIL_DISABLED"):
        print(
            "error: this soak requires a free-threaded CPython build "
            "(Py_GIL_DISABLED is not set)",
            file=sys.stderr,
        )
        return 2
    record = run(args)
    if args.json:
        with open(args.json, "w", encoding="utf-8") as handle:
            json.dump(record, handle, indent=2, sort_keys=True)
            handle.write("\n")
    totals = record["totals"]
    print(
        "soak: %d http requests (%d ok, %d fail), tunnel=%d, disconnect=%d, %.1fs, qualified=%s"
        % (
            totals["http_requests"],
            totals["ok"],
            totals["fail"],
            totals["by_scenario"]["tunnel"]["ok"],
            totals["by_scenario"]["disconnect"]["ok"],
            record["duration_seconds"],
            record["qualified"],
        )
    )
    if record["problems"]:
        for problem in record["problems"]:
            print("problem: " + problem, file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())

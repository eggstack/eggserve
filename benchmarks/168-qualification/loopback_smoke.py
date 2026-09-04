"""Plan 168 qualification smoke: loopback throughput snapshot (native CLI).

Starts the release CLI on an ephemeral port, drives keep-alive GET load with
a fixed worker count, repeats trials, and records elapsed/rps/RSS. This is a
suitability smoke measurement, not a release gate or cross-machine benchmark:
absolute numbers are platform- and machine-specific and must not be copied
into prose as headline claims. Machine-readable output goes to stdout as JSON.

Usage (from the repository root)::

    cargo build --release --locked -p eggserve-bin
    python3 benchmarks/168-qualification/loopback_smoke.py
"""

import http.client
import json
import os
import socket
import subprocess
import sys
import tempfile
import threading
import time

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
CLI = os.path.join(REPO, "target", "release", "eggserve")

REQUESTS = 2000
WORKERS = 16
TRIALS = 3
PATHS = ("/f1k.bin", "/f1m.bin")


def free_port():
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def worker(path, port, count, errors):
    try:
        conn = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        for _ in range(count):
            conn.request("GET", path)
            resp = conn.getresponse()
            body = resp.read()
            if resp.status != 200 or len(body) == 0:
                errors.append((resp.status, len(body)))
        conn.close()
    except Exception as exc:  # noqa: BLE001 - counted, reported
        errors.append(str(exc))


def trial(path, port):
    per_worker, rem = divmod(REQUESTS, WORKERS)
    counts = [per_worker + (1 if i < rem else 0) for i in range(WORKERS)]
    errors: list = []
    threads = [
        threading.Thread(target=worker, args=(path, port, n, errors)) for n in counts
    ]
    start = time.monotonic()
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    elapsed = time.monotonic() - start
    return elapsed, errors


def peak_rss_kb(proc):
    try:
        with open(f"/proc/{proc.pid}/status") as fh:
            for line in fh:
                if line.startswith("VmHWM"):
                    return int(line.split()[1])
    except OSError:
        return None
    return None


def main():
    if not os.path.isfile(CLI):
        raise SystemExit(f"build the release CLI first: {CLI} missing")
    with tempfile.TemporaryDirectory(prefix="eggserve-168-smoke-") as root:
        with open(os.path.join(root, "f1k.bin"), "wb") as fh:
            fh.write(b"x" * 1024)
        with open(os.path.join(root, "f1m.bin"), "wb") as fh:
            fh.write(b"y" * 1048576)
        port = free_port()
        proc = subprocess.Popen(
            [CLI, "--directory", root, "--port", str(port), "--log-format", "none"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        try:
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                try:
                    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=1)
                    conn.request("GET", "/f1k.bin")
                    resp = conn.getresponse()
                    resp.read()
                    conn.close()
                    if resp.status == 200:
                        break
                except OSError:
                    time.sleep(0.05)
            else:
                raise SystemExit("server did not become ready")

            out = {"requests": REQUESTS, "workers": WORKERS, "trials": {}}
            for path in PATHS:
                path_trials = []
                for _ in range(TRIALS):
                    elapsed, errors = trial(path, port)
                    path_trials.append(
                        {
                            "elapsed_s": round(elapsed, 4),
                            "rps": round(REQUESTS / elapsed, 1),
                            "errors": errors[:5],
                            "error_count": len(errors),
                        }
                    )
                out["trials"][path] = path_trials
            out["server_peak_rss_kb"] = peak_rss_kb(proc)
            print(json.dumps(out, indent=2))
        finally:
            proc.terminate()
            proc.wait(timeout=15)


if __name__ == "__main__":
    main()

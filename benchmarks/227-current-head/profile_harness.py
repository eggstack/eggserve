#!/usr/bin/env python3
"""Collect compact Linux perf evidence for Plan 227 response paths."""

from __future__ import annotations

import argparse
import json
import os
import signal
import shutil
import socket
import subprocess
import tempfile
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
CLIENT = HERE / "native_client"


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def wait_ready(port: int, process: subprocess.Popen) -> None:
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError("profiled server exited before readiness")
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.5):
                return
        except OSError:
            time.sleep(0.05)
    raise RuntimeError("profiled server did not become ready")


def one_profile(label: str, command: list[str], path: str, size: int, output: Path, bind_flag: bool = True) -> dict:
    strace = shutil.which("strace")
    if strace is None:
        return {"label": label, "status": "unavailable", "reason": "perf is restricted and strace is not installed"}
    port = free_port()
    summary = output.with_suffix(".txt")
    bind = ["--bind", f"127.0.0.1:{port}"] if bind_flag else [f"127.0.0.1:{port}"]
    profiled = subprocess.Popen(
        [strace, "-f", "-c", "-o", str(summary), "--", *command, *bind],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    try:
        wait_ready(port, profiled)
        requests = 3000 if size == 1024 else 120
        subprocess.run([str(CLIENT), "127.0.0.1", str(port), path, str(size), "1", str(requests)], check=True, stdout=subprocess.DEVNULL)
    finally:
        if profiled.poll() is None:
            # Let the traced program exit normally so strace flushes its
            # aggregate table; SIGTERM can leave -c output empty.
            os.killpg(profiled.pid, signal.SIGINT)
        try:
            profiled.wait(timeout=10)
        except subprocess.TimeoutExpired:
            profiled.kill()
            profiled.wait(timeout=10)
    if not summary.exists() or summary.stat().st_size == 0:
        return {"label": label, "status": "unavailable", "reason": "strace summary failed"}
    return {"label": label, "status": "captured", "tool": "strace -f -c", "summary": str(summary)}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-dir", type=Path, default=HERE / "profiles")
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    subprocess.run(["rustc", "-O", str(HERE / "native_client.rs"), "-o", str(CLIENT)], check=True)
    cli = ROOT / "target/release/eggserve"
    streaming = ROOT / "target/release/examples/streaming_service"
    with tempfile.TemporaryDirectory(prefix="eggserve-227-profile-") as root_name:
        root = Path(root_name)
        (root / "f1k.bin").write_bytes(b"x" * 1024)
        (root / "f1m.bin").write_bytes(b"x" * (1024 * 1024))
        static_common = [str(cli), "--directory", str(root), "--log-format", "none", "--max-connections", "128", "--max-file-streams", "128", "--max-in-flight-requests", "128"]
        captures = [
            one_profile("static_1k_keepalive", static_common, "/f1k.bin", 1024, args.output_dir / "static-1k"),
            one_profile("static_1m_keepalive", static_common, "/f1m.bin", 1024 * 1024, args.output_dir / "static-1m"),
            one_profile("application_known_stream_1m", [str(streaming)], "/known/1048576", 1024 * 1024, args.output_dir / "application-1m", bind_flag=False),
        ]
    (args.output_dir / "capture.json").write_text(json.dumps({"tool": "strace -f -c", "perf_note": "perf is installed but blocked by perf_event_paranoid=4; syscall summaries are the available equivalent on this host", "captures": captures}, indent=2) + "\n")
    try:
        CLIENT.unlink()
    except FileNotFoundError:
        pass
    print(json.dumps(captures, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

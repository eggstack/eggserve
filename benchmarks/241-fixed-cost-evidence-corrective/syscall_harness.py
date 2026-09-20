#!/usr/bin/env python3
"""Focused Unix resolver syscall capture for Plan 241."""

from __future__ import annotations

import argparse
import json
import os
import signal
import socket
import subprocess
import tempfile
import time
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CLIENT_SOURCE = ROOT / "benchmarks/227-current-head/native_client.rs"


def port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def ready(p: subprocess.Popen, value: int) -> None:
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        if p.poll() is not None:
            raise RuntimeError("server exited before syscall capture readiness")
        try:
            with socket.create_connection(("127.0.0.1", value), timeout=0.5):
                return
        except OSError:
            time.sleep(0.05)
    raise RuntimeError("server did not become ready")


def capture(cli: Path, root: Path, client: Path, output: Path) -> dict:
    value = port()
    summary = output.with_suffix(".txt")
    command = [
        str(cli), "--directory", str(root), "--log-format", "none",
        "--max-connections", "128", "--max-file-streams", "128",
        "--max-in-flight-requests", "128",
    ]
    traced = subprocess.Popen(
        ["strace", "-f", "-c", "-o", str(summary), "--", *command,
         "--bind", f"127.0.0.1:{value}"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    try:
        ready(traced, value)
        subprocess.run(
            [str(client), "127.0.0.1", str(value), "/nested/deep.bin",
             "2048", "1", "3000"],
            check=True, stdout=subprocess.DEVNULL,
        )
        subprocess.run(
            [str(client), "127.0.0.1", str(value), "/one.bin",
             "1024", "1", "3000"],
            check=True, stdout=subprocess.DEVNULL,
        )
    finally:
        if traced.poll() is None:
            os.killpg(traced.pid, signal.SIGINT)
        try:
            traced.wait(timeout=10)
        except subprocess.TimeoutExpired:
            traced.kill()
            traced.wait(timeout=10)
    output.write_text(summary.read_text() if summary.exists() else "")
    return {
        "status": "captured" if summary.exists() and summary.stat().st_size else "unavailable",
        "tool": "strace -f -c",
        "summary": str(output),
        "workload": ["one-component file", "nested file"],
        "security_syscalls_expected": [
            "newfstatat/statx with AT_SYMLINK_NOFOLLOW where emitted",
            "openat with O_NOFOLLOW where emitted",
            "close for opened descriptors",
        ],
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--sha", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="eggserve-241-syscall-") as temp:
        root = Path(temp)
        (root / "nested").mkdir()
        (root / "one.bin").write_bytes(b"o" * 1024)
        (root / "nested" / "deep.bin").write_bytes(b"d" * 2048)
        client = root / "native_client"
        subprocess.run(["rustc", "-O", str(CLIENT_SOURCE), "-o", str(client)], check=True)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        result = capture(args.cli, root, client, args.output)
    args.output.with_suffix(".json").write_text(json.dumps({
        "schema_version": 1,
        "plan": "241",
        "source_sha": args.sha,
        "capture": result,
        "interpretation": "The candidate's ordinary one-component path must not add a root-FD dup/close pair; nested traversal must retain intermediate descriptor ownership and close behavior. Syscall tables include startup/runtime noise and are not timing gates.",
    }, indent=2) + "\n")
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

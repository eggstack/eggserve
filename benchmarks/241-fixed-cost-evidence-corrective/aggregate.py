#!/usr/bin/env python3
"""Assemble Plan 241 raw captures into the committed evidence layout."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import subprocess
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
OUT = Path(__file__).resolve().parent
BASE = "504c3d31f46399d361e57a5bab51a6325a0f4acd"
CAND = "af9727870236e684746858bc32eb37aa22892251"


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def git_output(*args: str) -> str:
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()


def rewrite_identity(value: dict, source_sha: str) -> dict:
    value = json.loads(json.dumps(value))
    value["source_sha"] = source_sha
    if "identity" in value:
        value["identity"]["source_sha"] = source_sha
    return value


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-native", type=Path, required=True)
    parser.add_argument("--candidate-native", type=Path, required=True)
    parser.add_argument("--base-tls", type=Path, required=True)
    parser.add_argument("--candidate-tls", type=Path, required=True)
    parser.add_argument("--base-tls-callbacks", type=Path, required=True)
    parser.add_argument("--candidate-tls-callbacks", type=Path, required=True)
    parser.add_argument("--base-callbacks", type=Path, required=True)
    parser.add_argument("--candidate-callbacks", type=Path, required=True)
    parser.add_argument("--base-streams", type=Path, required=True)
    parser.add_argument("--candidate-streams", type=Path, required=True)
    args = parser.parse_args()

    raw = OUT / "raw"
    for name in ("native-custom", "native-path", "tls", "python-callback", "python-stream"):
        (raw / name).mkdir(parents=True, exist_ok=True)
    (OUT / "ci").mkdir(parents=True, exist_ok=True)

    base_native = json.loads(args.base_native.read_text())
    cand_native = json.loads(args.candidate_native.read_text())
    for label, value in (("baseline", base_native), ("candidate", cand_native)):
        (raw / "native-custom" / f"{label}.json").write_text(json.dumps({
            "schema_version": 1, "plan": "241", "source_sha": value["source_sha"],
            "build": value["build"], "method": value["method"],
            "environment": value["environment"], "runtime_limits": value["runtime_limits"],
            "workload": value["workloads"]["custom_h1_1k"],
        }, indent=2) + "\n")
        (raw / "native-path" / f"{label}.json").write_text(json.dumps({
            "schema_version": 1, "plan": "241", "source_sha": value["source_sha"],
            "build": value["build"], "method": value["method"],
            "environment": value["environment"], "runtime_limits": value["runtime_limits"],
            "workload": value["workloads"]["static_response_shapes"],
            "caller_owned_h1": value["workloads"]["caller_owned_h1"],
        }, indent=2) + "\n")

    for label, source, sha in (("baseline", args.base_tls, BASE), ("candidate", args.candidate_tls, CAND)):
        for name in ("tls-established-trials.json", "tls-handshake-trials.json"):
            value = rewrite_identity(json.loads((source / "raw" / name).read_text()), sha)
            (raw / "tls" / f"{label}-{name}").write_text(json.dumps(value, indent=2) + "\n")

    for label, plain, tls, sha in (
        ("baseline", args.base_callbacks, args.base_tls_callbacks, BASE),
        ("candidate", args.candidate_callbacks, args.candidate_tls_callbacks, CAND),
    ):
        value = {
            "schema_version": 1, "plan": "241", "source_sha": sha,
            "wheel": "installed isolated abi3 wheel",
            "plain_http": json.loads(plain.read_text()),
            "established_tls": json.loads(tls.read_text()),
        }
        (raw / "python-callback" / f"{label}.json").write_text(json.dumps(value, indent=2) + "\n")

    for label, source, sha in (("baseline", args.base_streams, BASE), ("candidate", args.candidate_streams, CAND)):
        value = json.loads(source.read_text())
        value["source_sha"] = sha
        (raw / "python-stream" / f"{label}.json").write_text(json.dumps(value, indent=2) + "\n")

    wheels = {}
    for label in ("base", "cand"):
        wheel = next((Path(path) for path in (Path("/tmp/eggserve-241-wheels") / label).glob("*.whl")), None)
        if wheel is not None:
            wheels["baseline" if label == "base" else "candidate"] = {
                "filename": wheel.name, "sha256": sha256(wheel), "python": "3.11.15",
            }
    (raw / "python-callback" / "wheel-identity.json").write_text(json.dumps(wheels, indent=2) + "\n")

    native_summary = {}
    for label, value in (("baseline", base_native), ("candidate", cand_native)):
        native_summary[label] = {
            "custom_h1_1k": [
                {"concurrency": case["concurrency"], "median_rps": statistics_median(case), "errors": case["total_errors"], "correct": case["all_correct"]}
                for case in value["workloads"]["custom_h1_1k"]["cases"]
            ],
            "static_response_shapes": [
                {"name": case["name"], "median_latency_ms": case["median_latency_ms"], "errors": case["errors"], "correct": case["all_correct"]}
                for case in value["workloads"]["static_response_shapes"]["cases"]
            ],
            "caller_owned_h1": value["workloads"]["caller_owned_h1"],
        }

    tls_summary = {}
    for label, source in (("baseline", args.base_tls), ("candidate", args.candidate_tls)):
        value = json.loads((source / "raw" / "tls-established-trials.json").read_text())
        tls_summary[label] = [
            {"response_size": case["response_size"], "concurrency": case["concurrency"], "median_rps": case["median_rps"], "errors": case["total_errors"]}
            for case in value["cases"]
        ]

    python_summary = {}
    for label, source in (("baseline", args.base_callbacks), ("candidate", args.candidate_callbacks)):
        value = json.loads(source.read_text())
        python_summary[label] = [
            {"mode": case["mode"], "median_rps": statistics_median(case), "errors": case["errors"]}
            for case in value["workloads"]
        ]

    lock = json.loads((ROOT / "benchmarks/240-fixed-cost-closure/results.json").read_text())
    cpu = "unknown"
    cpuinfo = Path("/proc/cpuinfo")
    if cpuinfo.exists():
        cpu = next((line.split(":", 1)[1].strip() for line in cpuinfo.read_text().splitlines() if line.lower().startswith("model name")), "unknown")
    results = {
        "schema_version": 1,
        "plan": "241",
        "status": "evidence-content-complete-pending-remote-ci",
        "baseline_sha": BASE,
        "candidate_sha": CAND,
        "evidence_content_sha": None,
        "root_lock_sha256": lock["cargo_lock_sha256"],
        "python_crate_lock_sha256": lock["python_crate_lock_sha256"],
        "compiler": {"benchmark": subprocess.check_output(["rustc", "--version"], text=True).strip(), "requested_build_toolchain": subprocess.check_output(["rustup", "run", "1.89.0", "rustc", "--version"], text=True).strip()},
        "environment": {
            "os": platform.platform(), "arch": platform.machine(),
            "cpu": cpu,
            "logical_cpus": os.cpu_count(),
            "host_note": "See raw JSON environment fields; no allocator profiler available.",
        },
        "builds": {
            "native": "cargo +1.89 build --release --locked -p eggserve-bin",
            "native_tls": "cargo +1.89 build --release --locked -p eggserve-bin --features tls",
            "examples": "cargo +1.89 build --release --locked -p eggserve-core --example streaming_service --example caller_owned_stream",
            "wheel": "python3.11 -m maturin build --profile dist --interpreter python3.11",
        },
        "runtime_limits": {"max_connections": 512, "max_file_streams": 512, "max_in_flight_requests": 512, "max_buf_size": 65536, "max_headers": 100, "max_header_bytes": 32768, "max_request_target_bytes": 8192},
        "method": {"native_trials": 3, "python_callback_trials": 3, "warmup": "one excluded case run", "absolute_timing_ci_gate": False},
        "measured": {"native": native_summary, "tls_established": tls_summary, "python_callbacks": python_summary, "wheels": wheels},
        "resource_matrix": {"source": "raw/python-stream/{baseline,candidate}.json", "model": "one producer thread and bounded 16-chunk channel per active synchronous stream", "disconnect_and_shutdown": "captured with cleanup observations"},
        "syscall_proof": {"source": "raw/{baseline,candidate}[-nested]-detail.txt and raw/{baseline,candidate}-syscalls.txt", "candidate_one_component": "no per-request fcntl(F_DUPFD_CLOEXEC) before statat/openat", "nested": "candidate retains statat/openat intermediate descriptor and closes it", "security_checks": ["AT_SYMLINK_NOFOLLOW", "O_NOFOLLOW", "post-open statx/type validation", "close"]},
        "source_mechanical": {"plan_237_metadata_sharing": "DEFER remains unchanged", "plan_238": "NO-GO remains unchanged", "plan_239_producer_redesign": "DEFER remains unchanged", "peer_certificate_chain": "unavailable in the selected deterministic fixture; no mTLS fixture was added under this evidence-only plan"},
        "decisions": {"custom_h1": "CONFIRMS", "static_response_shapes": "CONFIRMS", "caller_owned_h1": "CONFIRMS", "established_tls": "NEUTRAL", "tls_handshake_churn": "NEUTRAL", "python_lazy_views": "CONFIRMS", "python_metadata_heavy": "NEUTRAL", "python_slow_stream_resources": "CONFIRMS", "unix_resolver_syscalls": "CONFIRMS"},
        "provenance": {"plan_240_ci": {"sha": "5b048cbf66f57957625c9ad8b658635a56ac9593", "run_id": "35538302042", "conclusion": "success"}, "plan_241_ci": None},
    }
    (OUT / "results.json").write_text(json.dumps(results, indent=2) + "\n")
    print(json.dumps({"output": str(OUT / "results.json"), "raw": str(raw)}, indent=2))
    return 0


def statistics_median(case: dict) -> float:
    values = [row.get("rps", 0.0) for row in case["trials"]]
    values.sort()
    return values[len(values) // 2]


if __name__ == "__main__":
    raise SystemExit(main())

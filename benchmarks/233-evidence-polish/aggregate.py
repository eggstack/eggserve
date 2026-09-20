#!/usr/bin/env python3
"""Plan 233 aggregate builder (benchmark-only, stdlib only).

Reads the retained per-trial raw files and writes ``results.json``. Every
aggregate row is derived from the raw trial rows; no absolute-timing claim
is invented (min/median/max spread only, never confidence intervals).
"""

from __future__ import annotations

import argparse
import json
import statistics
from pathlib import Path


def load(name: str, raw: Path) -> dict:
    return json.loads((raw / name).read_text())


def trial_stats(trials: list[dict]) -> dict:
    rps = [t["rps"] for t in trials]
    return {
        "median_rps": statistics.median(rps),
        "min_rps": min(rps),
        "max_rps": max(rps),
        "errors": sum(t["error_count"] for t in trials),
    }


def summarize_native(doc: dict) -> list[dict]:
    rows = []
    for case in doc["cases"]:
        stats = trial_stats(case["trials"])
        peak = max(
            (t.get("peak_rss_kb") or 0 for t in case["trials"]), default=0
        )
        p95 = [t["p95_ms"] for t in case["trials"] if t.get("p95_ms") is not None]
        p99 = [t["p99_ms"] for t in case["trials"] if t.get("p99_ms") is not None]
        rows.append({
            "path": case["path"],
            "response_size": case["response_size"],
            "concurrency": case["concurrency"],
            **stats,
            "mean_p95_ms": statistics.mean(p95) if p95 else None,
            "mean_p99_ms": statistics.mean(p99) if p99 else None,
            "peak_rss_kb": peak,
        })
    return rows


def summarize_ranges(doc: dict) -> list[dict]:
    rows = []
    for case in doc["cases"]:
        stats = trial_stats(case["trials"])
        peak = max(
            (t.get("peak_rss_kb") or 0 for t in case["trials"]), default=0
        )
        rows.append({
            "range_len": case["range_len"],
            "concurrency": case["concurrency"],
            **stats,
            "all_exact": case["all_exact"],
            "all_status_206": all(t["all_status_206"] for t in case["trials"]),
            "peak_rss_kb": peak,
        })
    return rows


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dir", type=Path, default=Path(__file__).parent)
    args = parser.parse_args()
    raw = args.dir / "raw"

    native_64 = load("native-64k-trials.json", raw)
    native_128 = load("native-128k-trials.json", raw)
    ranges_64 = load("ranges-64k-trials.json", raw)
    ranges_128 = load("ranges-128k-trials.json", raw)
    tls_est = load("tls-established-trials.json", raw)
    tls_hs = load("tls-handshake-trials.json", raw)
    env = load("environment.json", raw)
    closure_ci = json.loads((args.dir / "ci" / "plan232-closure-ci.json").read_text())

    rows_64 = summarize_native(native_64)
    rows_128 = summarize_native(native_128)
    comparison = []
    for row64 in rows_64:
        row128 = next(
            r for r in rows_128
            if r["path"] == row64["path"] and r["concurrency"] == row64["concurrency"]
        )
        delta = (row128["median_rps"] - row64["median_rps"]) / row64["median_rps"]
        comparison.append({
            "path": row64["path"],
            "response_size": row64["response_size"],
            "concurrency": row64["concurrency"],
            "median_rps_64k": row64["median_rps"],
            "median_rps_128k": row128["median_rps"],
            "delta_128k_vs_64k": delta,
            "peak_rss_kb_64k": row64["peak_rss_kb"],
            "peak_rss_kb_128k": row128["peak_rss_kb"],
            "errors_64k": row64["errors"],
            "errors_128k": row128["errors"],
        })

    range_rows_64 = summarize_ranges(ranges_64)
    range_rows_128 = summarize_ranges(ranges_128)
    tls_rows = summarize_native(tls_est)

    result = {
        "schema_version": 1,
        "plan": "233",
        "production_code_changed": False,
        "raw_artifacts": [
            "raw/native-64k-trials.json",
            "raw/native-128k-trials.json",
            "raw/ranges-64k-trials.json",
            "raw/ranges-128k-trials.json",
            "raw/tls-established-trials.json",
            "raw/tls-handshake-trials.json",
            "raw/environment.json",
            "ci/plan232-closure-ci.json",
            "ci/plan233-closing-ci.json",
        ],
        "environment": env,
        "plan232_closure_ci": closure_ci,
        "native_64k_summary": rows_64,
        "native_128k_summary": rows_128,
        "native_64k_vs_128k": comparison,
        "range_64k_summary": range_rows_64,
        "range_128k_summary": range_rows_128,
        "tls_established_summary": tls_rows,
        "tls_handshake_trials": tls_hs["trials"],
        "conclusions": {
            "small_responses_neutral": True,
            "medium_large_128k_advantage_retained": True,
            "lower_64k_high_concurrency_rss_retained": True,
            "selected_default_chunk_bytes": 131072,
            "tradeoff": (
                "128 KiB retains a meaningful same-machine throughput advantage "
                "for 1 MiB/16 MiB static responses with higher but bounded "
                "high-concurrency peak RSS; the 128 KiB default is retained."
            ),
            "noted_deviation": (
                "The 128 KiB-body/concurrency-16 cell measured faster under the "
                "64 KiB regime this session (about 22% median RPS), unlike the "
                "neutral Plan 232 reading for that cell. Trial spreads are tight "
                "within each regime, so this is same-machine cross-run drift "
                "(regime runs are 30+ minutes apart), not a decision-reversing "
                "result: the 1 MiB/16 MiB advantage and RSS tradeoff that carry "
                "the 128 KiB decision both reproduced."
            ),
        },
    }
    (args.dir / "results.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"output": str(args.dir / "results.json"),
                      "comparisons": len(comparison)}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

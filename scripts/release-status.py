#!/usr/bin/env python3
"""Release-candidate status generator (Plan 089, Track A).

Reads criteria.toml and support-profiles.toml to produce a machine-readable
release-candidate status file showing gate status per profile.

Usage:
    python3 scripts/release-status.py [--sha <commit>] [--evidence-dir <dir>] [--output <file>]
    python3 scripts/release-status.py --freeze <commit>   # freeze a candidate SHA
"""

import argparse
import hashlib
import json
import os
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
CRITERIA_TOML = REPO_ROOT / "release" / "criteria.toml"
PROFILES_TOML = REPO_ROOT / "release" / "support-profiles.toml"
STATUS_FILE = REPO_ROOT / "release" / "rc-status.json"


def get_sha(ref="HEAD"):
    """Get the full SHA for a git ref."""
    result = subprocess.run(
        ["git", "rev-parse", ref], capture_output=True, text=True, cwd=REPO_ROOT
    )
    if result.returncode != 0:
        return "unknown"
    return result.stdout.strip()


def get_toolchain_info():
    """Collect build environment metadata."""
    info = {}
    for key, cmd in [
        ("rust_toolchain", ["rustc", "--version"]),
        ("cargo_version", ["cargo", "--version"]),
        ("python_version", ["python3", "--version"]),
    ]:
        try:
            result = subprocess.run(cmd, capture_output=True, text=True, cwd=REPO_ROOT)
            info[key] = result.stdout.strip() if result.returncode == 0 else "unknown"
        except FileNotFoundError:
            info[key] = "not found"
    return info


def cargo_lock_hash():
    """Hash of Cargo.lock for reproducibility binding."""
    lock_path = REPO_ROOT / "Cargo.lock"
    if not lock_path.exists():
        return "missing"
    return hashlib.sha256(lock_path.read_bytes()).hexdigest()[:16]


def parse_toml_simple(path):
    """Minimal TOML parser for criteria/profile files (no external deps)."""
    sections = []
    current = None
    with open(path) as f:
        for line in f:
            stripped = line.strip()
            if not stripped or stripped.startswith("#"):
                continue
            if stripped.startswith("[[") and stripped.endswith("]]"):
                if current is not None:
                    sections.append(current)
                current = {"_type": stripped[2:-2], "_raw": []}
            elif stripped.startswith("[") and stripped.endswith("]"):
                if current is not None:
                    sections.append(current)
                current = {"_type": stripped[1:-1], "_raw": []}
            elif current is not None:
                current["_raw"].append(stripped)
    if current is not None:
        sections.append(current)
    return sections


def parse_criteria(path):
    """Parse criteria.toml into a dict of gate_id -> gate_def."""
    gates = {}
    sections = parse_toml_simple(path)
    for sec in sections:
        if sec.get("_type") != "gate":
            continue
        gate = {}
        for raw in sec.get("_raw", []):
            if "=" in raw:
                key, _, val = raw.partition("=")
                key = key.strip()
                val = val.strip()
                if key == "id":
                    gate["id"] = val.strip('"')
                elif key == "title":
                    gate["title"] = val.strip('"')
                elif key == "required":
                    gate["required"] = val == "true"
                elif key == "evidence_classes":
                    gate["evidence_classes"] = [
                        v.strip().strip('"')
                        for v in val.strip("[]").split(",")
                        if v.strip()
                    ]
                elif key == "workflow_job":
                    gate["workflow_job"] = val.strip('"')
                elif key == "platforms":
                    gate["platforms"] = [
                        v.strip().strip('"')
                        for v in val.strip("[]").split(",")
                        if v.strip()
                    ]
                elif key == "triggers":
                    gate["triggers"] = [
                        v.strip().strip('"')
                        for v in val.strip("[]").split(",")
                        if v.strip()
                    ]
        if "id" in gate:
            gates[gate["id"]] = gate
    return gates


def parse_profiles(path):
    """Parse support-profiles.toml into a list of profile dicts."""
    profiles = []
    sections = parse_toml_simple(path)
    current = None
    for sec in sections:
        if sec.get("_type") == "profile":
            if current is not None:
                profiles.append(current)
            current = {"required_gates": []}
        # Process raw lines for ALL sections (including profile sections)
        if current is not None:
            in_gates = False
            for raw in sec.get("_raw", []):
                if raw.startswith("required_gates"):
                    in_gates = True
                    val = raw.split("=", 1)[1].strip()
                    if val.startswith("["):
                        items = val.strip("[]").split(",")
                        for item in items:
                            item = item.strip().strip('"').strip()
                            if item:
                                current["required_gates"].append(item)
                        if val.endswith("]"):
                            in_gates = False
                    continue
                if in_gates:
                    val = raw.rstrip(",").strip()
                    if val.endswith("]"):
                        in_gates = False
                        val = val.rstrip("]").strip()
                    val = val.strip('"').strip()
                    if val:
                        current["required_gates"].append(val)
                    continue
                if "=" in raw:
                    key, _, val = raw.partition("=")
                    key = key.strip()
                    val = val.strip()
                    if key == "profile":
                        current["profile"] = val.strip('"')
                    elif key == "status":
                        current["status"] = val.strip('"')
    if current is not None:
        profiles.append(current)
    return profiles


def check_evidence(gate_id, evidence_dir):
    """Check if evidence exists for a gate."""
    if evidence_dir is None:
        return "no-evidence-dir"
    gate_dir = evidence_dir / gate_id.replace(".", "_")
    if gate_dir.exists():
        return "present"
    # Also check flat file pattern
    for f in evidence_dir.glob(f"*{gate_id}*"):
        return "present"
    return "missing"


def build_status(sha, evidence_dir, gates, profiles):
    """Build the complete RC status structure."""
    toolchain = get_toolchain_info()
    now = datetime.now(timezone.utc).isoformat()

    status = {
        "schema_version": "1.0.0",
        "frozen_at": now,
        "source_sha": sha,
        "cargo_lock_hash": cargo_lock_hash(),
        "toolchain": toolchain,
        "evidence_dir": str(evidence_dir) if evidence_dir else None,
        "profiles": {},
        "gate_summary": {
            "total": len(gates),
            "required": sum(1 for g in gates.values() if g.get("required")),
            "evidence_present": 0,
            "evidence_missing": 0,
            "pending_human": 0,
        },
        "open_findings": [],
        "independent_review": {
            "status": "pending",
            "reviewer": None,
            "findings": [],
        },
        "promotion_decisions": {},
    }

    for profile in profiles:
        pname = profile["profile"]
        required = profile.get("required_gates", [])
        gate_status = {}
        for gid in required:
            gate_def = gates.get(gid, {})
            ev = check_evidence(gate_id=gid, evidence_dir=evidence_dir)
            triggers = gate_def.get("triggers", [])
            is_human = "approval-record" in gate_def.get("evidence_classes", [])
            gate_status[gid] = {
                "title": gate_def.get("title", gid),
                "required": gate_def.get("required", True),
                "evidence": ev,
                "triggers": triggers,
                "is_human_gate": is_human,
            }
            if ev == "present":
                status["gate_summary"]["evidence_present"] += 1
            elif is_human:
                status["gate_summary"]["pending_human"] += 1
            else:
                status["gate_summary"]["evidence_missing"] += 1

        status["profiles"][pname] = {
            "status": profile.get("status", "unknown"),
            "required_gates": required,
            "gate_status": gate_status,
            "promotion_eligible": all(
                gate_status.get(g, {}).get("evidence") == "present" or gate_status.get(g, {}).get("is_human_gate")
                for g in required
            ),
        }
        status["promotion_decisions"][pname] = {
            "status": "pending",
            "decision": None,
            "decided_at": None,
            "decided_by": None,
        }

    return status


def freeze_candidate(sha, evidence_dir, output_path=None):
    """Freeze a release candidate and write the status file."""
    output_path = output_path or STATUS_FILE
    sha = get_sha(sha)
    gates = parse_criteria(CRITERIA_TOML)
    profiles = parse_profiles(PROFILES_TOML)
    status = build_status(sha, evidence_dir, gates, profiles)

    output_path.write_text(json.dumps(status, indent=2) + "\n")
    print(f"Frozen release candidate: {sha[:12]}")
    print(f"Status written to: {output_path}")
    print(f"Toolchain: {status['toolchain']}")
    print(f"Cargo.lock hash: {status['cargo_lock_hash']}")
    print(f"Profiles: {', '.join(status['profiles'].keys())}")
    print(
        f"Gates: {status['gate_summary']['total']} total, "
        f"{status['gate_summary']['required']} required, "
        f"{status['gate_summary']['evidence_present']} evidence present, "
        f"{status['gate_summary']['evidence_missing']} evidence missing, "
        f"{status['gate_summary']['pending_human']} pending human"
    )
    return status


def show_status():
    """Display current RC status."""
    if not STATUS_FILE.exists():
        print("No release candidate frozen. Run: python3 scripts/release-status.py --freeze HEAD")
        return 1
    status = json.loads(STATUS_FILE.read_text())
    print(f"Release Candidate: {status['source_sha'][:12]}")
    print(f"Frozen at: {status['frozen_at']}")
    print(f"Cargo.lock: {status['cargo_lock_hash']}")
    print()
    for pname, pinfo in status["profiles"].items():
        eligible = "ELIGIBLE" if pinfo["promotion_eligible"] else "NOT ELIGIBLE"
        print(f"  [{pinfo['status']}] {pname} — {eligible}")
        missing = [
            gid
            for gid, gs in pinfo["gate_status"].items()
            if gs["evidence"] == "missing" and not gs["is_human_gate"]
        ]
        if missing:
            print(f"    Missing evidence: {', '.join(missing)}")
    return 0


def main():
    parser = argparse.ArgumentParser(description="Release-candidate status manager")
    parser.add_argument("--sha", default="HEAD", help="Git ref to freeze (default: HEAD)")
    parser.add_argument("--evidence-dir", type=Path, default=None, help="Evidence directory")
    parser.add_argument("--output", type=Path, default=None, help="Output file")
    parser.add_argument(
        "--freeze", metavar="SHA", nargs="?", const="HEAD", help="Freeze a candidate SHA"
    )
    parser.add_argument("--show", action="store_true", help="Show current RC status")
    args = parser.parse_args()

    output = args.output or STATUS_FILE

    if args.show:
        return show_status()

    sha = args.freeze or args.sha
    freeze_candidate(sha, args.evidence_dir, output)
    return 0


if __name__ == "__main__":
    sys.exit(main())

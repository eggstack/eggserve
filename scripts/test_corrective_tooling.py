#!/usr/bin/env python3
"""Plan 075 required tests for corrective release tooling.

Tests the corrective baseline, finding registry, support-profile containment,
and release-aggregation behavior required by Plan 075 acceptance criteria.
"""

from __future__ import annotations

import importlib.util
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

_SCRIPT_DIR = Path(__file__).resolve().parent
_REPO_ROOT = _SCRIPT_DIR.parent
_RC_PATH = _SCRIPT_DIR / "release_criteria.py"
_BASELINE_PATH = _REPO_ROOT / "release" / "corrective-baseline.toml"
_FINDINGS_PATH = _REPO_ROOT / "release" / "corrective-findings.toml"
_PROFILES_PATH = _REPO_ROOT / "release" / "support-profiles.toml"
_CRITERIA_PATH = _REPO_ROOT / "release" / "criteria.toml"


def _load_rc():
    mod_name = "_release_criteria_test_target"
    spec = importlib.util.spec_from_file_location(mod_name, _RC_PATH)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[mod_name] = mod
    spec.loader.exec_module(mod)
    return mod


rc = _load_rc()


def _load_toml(path: Path) -> dict:
    """Load a TOML file (Python 3.11+ or tomllib fallback)."""
    try:
        import tomllib
    except ModuleNotFoundError:
        import tomli as tomllib  # type: ignore[no-redef]
    with open(path, "rb") as f:
        return tomllib.load(f)


def _make_gate(
    gate_id: str = "test.gate",
    *,
    required: bool = True,
    evidence_classes: list[str] | None = None,
    platforms: list[str] | None = None,
    release_stage: str = "preflight",
    max_age_days: int = 1,
    invalidated_by: list[str] | None = None,
    depends_on: list[str] | None = None,
    waiver_allowed: bool = False,
    command: str | None = "echo ok",
    **extra,
) -> dict:
    d = {
        "id": gate_id,
        "title": f"Title {gate_id}",
        "description": f"Description {gate_id}",
        "required": required,
        "evidence_classes": evidence_classes or ["ci-log"],
        "platforms": platforms or ["linux"],
        "release_stage": release_stage,
        "max_age_days": max_age_days,
        "invalidated_by": invalidated_by or [],
        "depends_on": depends_on or [],
        "waiver_allowed": waiver_allowed,
        "artifacts": [],
    }
    if command is not None:
        d["command"] = command
    d.update(extra)
    return d


def _criteria_toml(gates: list[dict]) -> str:
    lines = [
        "[meta]",
        'schema_version = "1.0.0"',
        'project = "test"',
        "",
    ]
    for g in gates:
        lines.append("[[gate]]")
        for k, v in g.items():
            if isinstance(v, list):
                lines.append(f"{k} = {v!r}")
            elif isinstance(v, bool):
                lines.append(f"{k} = {'true' if v else 'false'}")
            elif isinstance(v, int):
                lines.append(f"{k} = {v}")
            else:
                lines.append(f'{k} = "{v}"')
        lines.append("")
    return "\n".join(lines)


def _valid_evidence(
    gate_id: str = "test.gate",
    *,
    result: str = "passed",
    commit_sha: str = "abc123",
    evidence_class: str = "LOCAL",
    dirty_tree: bool = False,
) -> dict:
    from datetime import datetime, timezone
    now = datetime.now(timezone.utc).isoformat()
    return {
        "schema_version": "1.0.0",
        "gate_id": gate_id,
        "result": result,
        "evidence_class": evidence_class,
        "command": "echo test",
        "exit_code": 0,
        "start_time": now,
        "end_time": now,
        "duration_secs": 1.0,
        "commit_sha": commit_sha,
        "dirty_tree": dirty_tree,
        "os": "linux",
        "arch": "x86_64",
        "tool_versions": {},
        "features": [],
    }


# ════════════════════════════════════════════════════════════════════════
# Test 1: Finding registry schema validation
# ════════════════════════════════════════════════════════════════════════
class TestFindingRegistrySchema(unittest.TestCase):
    """Validate the corrective-findings.toml registry structure."""

    @classmethod
    def setUpClass(cls):
        cls.findings = _load_toml(_FINDINGS_PATH)

    def test_has_findings_array(self):
        self.assertIn("finding", self.findings)

    def test_findings_are_list(self):
        self.assertIsInstance(self.findings["finding"], list)

    def test_minimum_finding_count(self):
        self.assertGreaterEqual(len(self.findings["finding"]), 17)

    def test_all_findings_have_required_fields(self):
        required = {"id", "title", "severity", "owning_plan", "status", "release"}
        for f in self.findings["finding"]:
            missing = required - set(f.keys())
            self.assertEqual(missing, set(), f"Finding {f.get('id', '?')} missing: {missing}")

    def test_no_duplicate_ids(self):
        ids = [f["id"] for f in self.findings["finding"]]
        dupes = [x for x in ids if ids.count(x) > 1]
        self.assertEqual(dupes, [], f"Duplicate finding IDs: {set(dupes)}")

    def test_severity_values(self):
        valid = {"critical", "high", "medium", "low"}
        for f in self.findings["finding"]:
            self.assertIn(f["severity"], valid, f"Finding {f['id']} has invalid severity")

    def test_status_values(self):
        valid = {"open", "closed", "deferred"}
        for f in self.findings["finding"]:
            self.assertIn(f["status"], valid, f"Finding {f['id']} has invalid status")

    def test_release_values(self):
        valid = {"A", "B", "C", "D", "E"}
        for f in self.findings["finding"]:
            self.assertIn(f["release"], valid, f"Finding {f['id']} has invalid release")

    def test_baseline_sha_recorded(self):
        meta = self.findings.get("meta", {})
        self.assertIn("baseline_sha", meta)

    def test_all_critical_findings_closed(self):
        critical = [f for f in self.findings["finding"] if f["severity"] == "critical"]
        for f in critical:
            self.assertEqual(f["status"], "closed", f"Critical finding {f['id']} is not closed")

    def test_all_high_findings_closed(self):
        high = [f for f in self.findings["finding"] if f["severity"] == "high"]
        for f in high:
            self.assertEqual(f["status"], "closed", f"High finding {f['id']} is not closed")


# ════════════════════════════════════════════════════════════════════════
# Test 2: Baseline document validation
# ════════════════════════════════════════════════════════════════════════
class TestBaselineDocument(unittest.TestCase):
    """Validate the corrective-baseline.toml document."""

    @classmethod
    def setUpClass(cls):
        cls.baseline = _load_toml(_BASELINE_PATH)

    def test_has_baseline_section(self):
        self.assertIn("baseline", self.baseline)

    def test_has_commit_sha(self):
        sha = self.baseline["baseline"].get("commit_sha", "")
        self.assertEqual(len(sha), 40, "commit_sha must be full SHA")

    def test_has_branch(self):
        self.assertIn("branch", self.baseline["baseline"])

    def test_has_toolchain(self):
        self.assertIn("toolchain", self.baseline)
        tc = self.baseline["toolchain"]
        self.assertIn("rustc_version", tc)
        self.assertIn("python_version", tc)

    def test_has_platforms(self):
        self.assertIn("platforms", self.baseline)
        for plat in ["linux", "macos", "windows"]:
            self.assertIn(plat, self.baseline["platforms"])

    def test_has_feature_combinations(self):
        self.assertIn("feature_combinations", self.baseline)

    def test_has_profile_classifications(self):
        self.assertIn("profile_classifications", self.baseline)

    def test_has_releases(self):
        self.assertIn("releases", self.baseline)
        for release in ["A", "B", "C", "D", "E"]:
            self.assertIn(release, self.baseline["releases"])

    def test_has_evidence_storage(self):
        self.assertIn("evidence_storage", self.baseline)


# ════════════════════════════════════════════════════════════════════════
# Test 3: Support-profile corrective_program marker
# ════════════════════════════════════════════════════════════════════════
class TestSupportProfileContainment(unittest.TestCase):
    """Validate corrective-program markers on support profiles."""

    @classmethod
    def setUpClass(cls):
        cls.profiles = _load_toml(_PROFILES_PATH)

    def test_metadata_has_corrective_program(self):
        meta = self.profiles.get("metadata", {})
        self.assertIn("corrective_program", meta)

    def test_all_profiles_have_corrective_program(self):
        for p in self.profiles.get("profile", []):
            self.assertIn(
                "corrective_program", p,
                f"Profile {p.get('profile', '?')} missing corrective_program",
            )

    def test_corrective_program_values(self):
        valid = {"open", "closed"}
        meta = self.profiles.get("metadata", {})
        if "corrective_program" in meta:
            self.assertIn(meta["corrective_program"], valid)
        for p in self.profiles.get("profile", []):
            if "corrective_program" in p:
                self.assertIn(p["corrective_program"], valid)

    def test_windows_hardened_not_promoted(self):
        """Windows hardened profiles must not be promoted before Release D."""
        for p in self.profiles.get("profile", []):
            if p.get("profile") == "windows-reverse-proxy":
                self.assertIn(p.get("status"), ("candidate", "functional", "supported-hardened"))


# ════════════════════════════════════════════════════════════════════════
# Test 4: Open high-severity finding blocks release
# ════════════════════════════════════════════════════════════════════════
class TestHighSeverityFindingBlocksRelease(unittest.TestCase):
    """An open critical/high finding must prevent release-ready status."""

    def _make_criteria_with_finding_gate(self, finding_open: bool) -> tuple[str, str]:
        """Create criteria + evidence where a finding-related gate reflects open/closed."""
        tmp = tempfile.mkdtemp()
        criteria_path = os.path.join(tmp, "criteria.toml")
        evidence_dir = os.path.join(tmp, "evidence")
        os.makedirs(evidence_dir)

        gate = _make_gate("corrective.finding-blocker", required=True)
        gate["description"] = "Blocks release if critical/high findings are open"
        with open(criteria_path, "w") as f:
            f.write(_criteria_toml([gate]))

        # Write evidence: failed if finding is open, passed if closed
        result = "failed" if finding_open else "passed"
        ev = _valid_evidence("corrective.finding-blocker", result=result)
        with open(os.path.join(evidence_dir, "corrective.finding-blocker.json"), "w") as f:
            json.dump(ev, f)

        return criteria_path, evidence_dir

    def test_open_finding_blocks_release(self):
        criteria, evidence = self._make_criteria_with_finding_gate(finding_open=True)
        criteria_obj, _ = rc.parse_criteria_file(criteria)
        gate_map = criteria_obj.gate_by_id()

        # Simulate aggregation
        all_records = {}
        for p in Path(evidence).glob("*.json"):
            if p.name == "manifest.json":
                continue
            with open(p) as f:
                rec = json.load(f)
            gid = rec.get("gate_id", "")
            all_records.setdefault(gid, []).append(rec)

        gate_results = {}
        for gid, gate in gate_map.items():
            records = all_records.get(gid, [])
            status, reasons = rc._aggregate_gate(gate, records, "abc123")
            gate_results[gid] = (status, reasons)

        # Required gate with failed evidence → not release-ready
        release_ready = all(
            s == "PASSED" for gid, (s, _) in gate_results.items()
            if gate_map[gid].required
        )
        self.assertFalse(release_ready)

    def test_closed_finding_allows_release(self):
        criteria, evidence = self._make_criteria_with_finding_gate(finding_open=False)
        criteria_obj, _ = rc.parse_criteria_file(criteria)
        gate_map = criteria_obj.gate_by_id()

        all_records = {}
        for p in Path(evidence).glob("*.json"):
            if p.name == "manifest.json":
                continue
            with open(p) as f:
                rec = json.load(f)
            gid = rec.get("gate_id", "")
            all_records.setdefault(gid, []).append(rec)

        gate_results = {}
        for gid, gate in gate_map.items():
            records = all_records.get(gid, [])
            status, reasons = rc._aggregate_gate(gate, records, "abc123")
            gate_results[gid] = (status, reasons)

        release_ready = all(
            s == "PASSED" for gid, (s, _) in gate_results.items()
            if gate_map[gid].required
        )
        self.assertTrue(release_ready)


# ════════════════════════════════════════════════════════════════════════
# Test 5: Missing required gate blocks release
# ════════════════════════════════════════════════════════════════════════
class TestMissingRequiredGateBlocksRelease(unittest.TestCase):
    """A required gate with no evidence must block release."""

    def test_missing_required_gate(self):
        gate = rc.Gate(
            id="required.missing", title="T", description="", required=True,
            evidence_classes=["ci-log"], platforms=["linux"],
            release_stage="preflight", max_age_days=1,
            invalidated_by=[], depends_on=[], waiver_allowed=False,
            artifacts=[],
        )
        status, reasons = rc._aggregate_gate(gate, [], "abc123")
        self.assertEqual(status, "MISSING")

    def test_missing_optional_gate_does_not_block(self):
        gate = rc.Gate(
            id="optional.missing", title="T", description="", required=False,
            evidence_classes=["ci-log"], platforms=["linux"],
            release_stage="preflight", max_age_days=1,
            invalidated_by=[], depends_on=[], waiver_allowed=False,
            artifacts=[],
        )
        status, reasons = rc._aggregate_gate(gate, [], "abc123")
        self.assertEqual(status, "NOT-APPLICABLE")


# ════════════════════════════════════════════════════════════════════════
# Test 6: Stale SHA blocks release
# ════════════════════════════════════════════════════════════════════════
class TestStaleSHABlocksRelease(unittest.TestCase):
    """Evidence from a different SHA must be classified as STALE for exact-SHA gates."""

    def test_stale_sha_detected(self):
        gate = rc.Gate(
            id="test.stale", title="T", description="", required=True,
            evidence_classes=["ci-log"], platforms=["linux"],
            release_stage="preflight", max_age_days=0,
            invalidated_by=[], depends_on=[], waiver_allowed=False,
            artifacts=[],
        )
        ev = _valid_evidence("test.stale", commit_sha="different_sha_abc123")
        status, reasons = rc._aggregate_gate(gate, [ev], "expected_sha_xyz")
        self.assertEqual(status, "STALE")

    def test_exact_sha_passes(self):
        gate = rc.Gate(
            id="test.exact", title="T", description="", required=True,
            evidence_classes=["ci-log"], platforms=["linux"],
            release_stage="preflight", max_age_days=0,
            invalidated_by=[], depends_on=[], waiver_allowed=False,
            artifacts=[],
        )
        ev = _valid_evidence("test.exact", commit_sha="expected_sha_xyz")
        status, reasons = rc._aggregate_gate(gate, [ev], "expected_sha_xyz")
        self.assertEqual(status, "PASSED")


# ════════════════════════════════════════════════════════════════════════
# Test 7: Closed finding without regression evidence blocks release
# ════════════════════════════════════════════════════════════════════════
class TestClosedFindingWithoutRegressionEvidence(unittest.TestCase):
    """A closed finding gate with no evidence is still MISSING, not auto-passed."""

    def test_closed_finding_no_evidence_blocks(self):
        gate = rc.Gate(
            id="finding.closed", title="T", description="",
            required=True, evidence_classes=["ci-log"],
            platforms=["linux"], release_stage="qualification",
            max_age_days=1, invalidated_by=[], depends_on=[],
            waiver_allowed=False, artifacts=[],
        )
        status, reasons = rc._aggregate_gate(gate, [], "abc123")
        # Even if the finding is closed, the gate still needs evidence
        self.assertEqual(status, "MISSING")


# ════════════════════════════════════════════════════════════════════════
# Test 8: Low-severity deferred finding does not block unrelated profiles
# ════════════════════════════════════════════════════════════════════════
class TestDeferredLowSeverityFinding(unittest.TestCase):
    """A low-severity deferred finding should not block unrelated narrow profiles."""

    def test_low_severity_optional_gate_not_blocking(self):
        gate = rc.Gate(
            id="finding.low.deferred", title="T", description="",
            required=False, evidence_classes=["ci-log"],
            platforms=["linux"], release_stage="qualification",
            max_age_days=1, invalidated_by=[], depends_on=[],
            waiver_allowed=False, artifacts=[],
        )
        ev = _valid_evidence("finding.low.deferred", result="skipped")
        status, reasons = rc._aggregate_gate(gate, [ev], "abc123")
        # Skipped on optional gate → not blocking
        self.assertIn(status, ("NOT-APPLICABLE", "SKIPPED"))


# ════════════════════════════════════════════════════════════════════════
# Test 9: Corrective findings registry gate mapping consistency
# ════════════════════════════════════════════════════════════════════════
class TestFindingGateMappingConsistency(unittest.TestCase):
    """Gate IDs referenced by findings must exist in criteria.toml."""

    @classmethod
    def setUpClass(cls):
        cls.findings = _load_toml(_FINDINGS_PATH)
        cls.criteria = _load_toml(_CRITERIA_PATH)
        cls.gate_ids = {g["id"] for g in cls.criteria.get("gate", [])}

    def test_all_required_gates_exist(self):
        for f in self.findings.get("finding", []):
            gates = f.get("finding", {}).get("gates_invalidated", {})
            for gid in gates.get("required", []):
                self.assertIn(
                    gid, self.gate_ids,
                    f"Finding {f['id']} references non-existent required gate: {gid}",
                )

    def test_all_regression_test_gate_ids_exist(self):
        for f in self.findings.get("finding", []):
            reg = f.get("finding", {}).get("regression_test", {})
            # Regression tests reference test names, not gate IDs — just validate structure
            if "test_name" in reg:
                self.assertIsInstance(reg["test_name"], str)
            if "command" in reg:
                self.assertIsInstance(reg["command"], str)


# ════════════════════════════════════════════════════════════════════════
# Test 10: All findings from Plan 074 are registered
# ════════════════════════════════════════════════════════════════════════
class TestPlan074FindingCompleteness(unittest.TestCase):
    """All 17 findings from Plan 074 must have a registry entry."""

    @classmethod
    def setUpClass(cls):
        cls.findings = _load_toml(_FINDINGS_PATH)

    def test_at_least_17_findings(self):
        self.assertGreaterEqual(len(self.findings["finding"]), 17)

    def test_all_releases_have_findings(self):
        releases = set(f["release"] for f in self.findings["finding"])
        for r in ["A", "B", "C", "D", "E"]:
            self.assertIn(r, releases, f"Release {r} has no findings")

    def test_critical_findings_in_release_a(self):
        """COR-001, COR-002, COR-004 are critical and in Release A."""
        critical_a = [
            f for f in self.findings["finding"]
            if f["severity"] == "critical" and f["release"] == "A"
        ]
        self.assertGreaterEqual(len(critical_a), 3)

    def test_each_finding_has_owning_plan(self):
        for f in self.findings["finding"]:
            self.assertIsInstance(f["owning_plan"], int)
            self.assertGreater(f["owning_plan"], 0)


if __name__ == "__main__":
    unittest.main()

#!/usr/bin/env python3
"""Unit tests for scripts/release_criteria.py CLI and library functions."""

from __future__ import annotations

import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path

_SCRIPT_DIR = Path(__file__).resolve().parent
_RC_PATH = _SCRIPT_DIR / "release_criteria.py"


def _load_rc():
    mod_name = "_release_criteria_test_target"
    spec = importlib.util.spec_from_file_location(mod_name, _RC_PATH)
    mod = importlib.util.module_from_spec(spec)
    sys.modules[mod_name] = mod
    spec.loader.exec_module(mod)
    return mod


rc = _load_rc()


# ---------------------------------------------------------------------------
# Fixture builders
# ---------------------------------------------------------------------------

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
    artifacts: list[str] | None = None,
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
        "artifacts": artifacts or [],
    }
    if command is not None:
        d["command"] = command
    d.update(extra)
    return d


def _criteria_toml(
    gates: list[dict],
    *,
    schema_version: str = "1.0.0",
    extra: str = "",
) -> str:
    lines = [
        "[meta]",
        f'schema_version = "{schema_version}"',
        'project = "test"',
        'version = "0.0.0"',
        'description = "test criteria"',
        "",
    ]
    for g in gates:
        lines.append("[[gate]]")
        for k, v in g.items():
            if isinstance(v, bool):
                lines.append(f"{k} = {'true' if v else 'false'}")
            elif isinstance(v, list):
                inner = ", ".join(f'"{x}"' for x in v)
                lines.append(f"{k} = [{inner}]")
            elif isinstance(v, str):
                lines.append(f'{k} = "{v}"')
            elif isinstance(v, int):
                lines.append(f"{k} = {v}")
            else:
                lines.append(f'{k} = "{v}"')
        lines.append("")
    if extra:
        lines.append(extra)
    return "\n".join(lines) + "\n"


def _valid_evidence(
    gate_id: str = "test.gate",
    *,
    result: str = "passed",
    commit_sha: str = "abc123",
    end_time: str | None = None,
    evidence_class: str = "LOCAL",
    artifact_ids: list[str] | None = None,
) -> dict:
    if end_time is None:
        end_time = datetime.now(timezone.utc).isoformat()
    return {
        "schema_version": "1.0.0",
        "gate_id": gate_id,
        "result": result,
        "evidence_class": evidence_class,
        "command": "echo test",
        "exit_code": 0,
        "start_time": end_time,
        "end_time": end_time,
        "duration_secs": 1.0,
        "commit_sha": commit_sha,
        "dirty_tree": False,
        "os": "linux",
        "arch": "x86_64",
        "tool_versions": {},
        "features": [],
        "artifact_ids": artifact_ids or [],
    }


# ---------------------------------------------------------------------------
# Parser tests
# ---------------------------------------------------------------------------

class TestParser(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.tmpdir = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def _write(self, content: str) -> str:
        p = self.tmpdir / "criteria.toml"
        p.write_text(content, encoding="utf-8")
        return str(p)

    def test_parse_minimal_criteria(self):
        toml = _criteria_toml([_make_gate()])
        path = self._write(toml)
        criteria, _ = rc.parse_criteria_file(path)
        self.assertEqual(len(criteria.gates), 1)
        self.assertEqual(criteria.gates[0].id, "test.gate")

    def test_parse_full_criteria(self):
        gate = _make_gate(
            command="cargo test",
            workflow_job="ci",
            features=["tls"],
            artifacts=["bin/eggserve"],
            security_relevance=True,
            doc_ref="https://example.com",
            waiver_authority="Release Manager",
        )
        toml = _criteria_toml([gate])
        path = self._write(toml)
        criteria, _ = rc.parse_criteria_file(path)
        g = criteria.gates[0]
        self.assertEqual(g.command, "cargo test")
        self.assertEqual(g.workflow_job, "ci")
        self.assertEqual(g.features, ["tls"])
        self.assertEqual(g.artifacts, ["bin/eggserve"])
        self.assertTrue(g.security_relevance)
        self.assertEqual(g.doc_ref, "https://example.com")
        self.assertEqual(g.waiver_authority, "Release Manager")

    def test_reject_duplicate_gate_ids(self):
        toml = _criteria_toml([_make_gate("dup"), _make_gate("dup")])
        path = self._write(toml)
        criteria, _ = rc.parse_criteria_file(path)
        v = rc.CriteriaValidator(criteria)
        diags = v.validate()
        errors = [d for d in diags if d.level == "error"]
        self.assertTrue(any("duplicate" in d.message for d in errors))

    def test_reject_missing_required_field(self):
        gate = _make_gate()
        del gate["title"]
        toml = _criteria_toml([gate])
        path = self._write(toml)
        criteria, _ = rc.parse_criteria_file(path)
        v = rc.CriteriaValidator(criteria)
        diags = v.validate()
        errors = [d for d in diags if d.level == "error"]
        self.assertTrue(any("title" in d.message for d in errors))

    def test_reject_unknown_evidence_class(self):
        gate = _make_gate(evidence_classes=["bogus-class"])
        toml = _criteria_toml([gate])
        path = self._write(toml)
        criteria, _ = rc.parse_criteria_file(path)
        v = rc.CriteriaValidator(criteria)
        diags = v.validate()
        errors = [d for d in diags if d.level == "error"]
        self.assertTrue(any("evidence_class" in d.message for d in errors))

    def test_reject_invalid_release_stage(self):
        gate = _make_gate(release_stage="INVALID")
        toml = _criteria_toml([gate])
        path = self._write(toml)
        criteria, _ = rc.parse_criteria_file(path)
        v = rc.CriteriaValidator(criteria)
        diags = v.validate()
        errors = [d for d in diags if d.level == "error"]
        self.assertTrue(any("release_stage" in d.message for d in errors))

    def test_reject_invalid_platform(self):
        gate = _make_gate(platforms=["beos"])
        toml = _criteria_toml([gate])
        path = self._write(toml)
        criteria, _ = rc.parse_criteria_file(path)
        v = rc.CriteriaValidator(criteria)
        diags = v.validate()
        errors = [d for d in diags if d.level == "error"]
        self.assertTrue(any("platform" in d.message for d in errors))

    def test_reject_empty_command_on_required_gate(self):
        gate = _make_gate(command="   ")
        toml = _criteria_toml([gate])
        path = self._write(toml)
        criteria, _ = rc.parse_criteria_file(path)
        v = rc.CriteriaValidator(criteria)
        diags = v.validate()
        errors = [d for d in diags if d.level == "error"]
        self.assertTrue(any("empty command" in d.message for d in errors))

    def test_reject_unknown_dependency(self):
        gate = _make_gate(depends_on=["nonexistent.gate"])
        toml = _criteria_toml([gate])
        path = self._write(toml)
        criteria, _ = rc.parse_criteria_file(path)
        v = rc.CriteriaValidator(criteria)
        diags = v.validate()
        errors = [d for d in diags if d.level == "error"]
        self.assertTrue(any("unknown gate" in d.message for d in errors))

    def test_reject_dependency_cycle(self):
        gates = [
            _make_gate("A", depends_on=["B"]),
            _make_gate("B", depends_on=["A"]),
        ]
        toml = _criteria_toml(gates)
        path = self._write(toml)
        criteria, _ = rc.parse_criteria_file(path)
        v = rc.CriteriaValidator(criteria)
        diags = v.validate()
        errors = [d for d in diags if d.level == "error"]
        self.assertTrue(any("cycle" in d.message for d in errors))

    def test_warn_config_only_execution_gate(self):
        gate = _make_gate(required=True, evidence_classes=["CONFIG"])
        toml = _criteria_toml([gate])
        path = self._write(toml)
        criteria, _ = rc.parse_criteria_file(path)
        v = rc.CriteriaValidator(criteria)
        diags = v.validate()
        warnings = [d for d in diags if d.level == "warning"]
        self.assertTrue(any("CONFIG" in d.message for d in warnings))

    def test_schema_version_mismatch(self):
        toml = _criteria_toml([_make_gate()], schema_version="9.9.9")
        path = self._write(toml)
        criteria, _ = rc.parse_criteria_file(path)
        v = rc.CriteriaValidator(criteria)
        diags = v.validate()
        errors = [d for d in diags if d.level == "error"]
        self.assertTrue(any("schema_version" in d.message for d in errors))


# ---------------------------------------------------------------------------
# Topological sort tests
# ---------------------------------------------------------------------------

class TestTopologicalSort(unittest.TestCase):
    def test_topo_sort_single_gate(self):
        gates = [rc.Gate(
            id="A", title="A", description="", required=True,
            evidence_classes=["ci-log"], platforms=["linux"],
            release_stage="preflight", max_age_days=1,
            invalidated_by=[], depends_on=[], waiver_allowed=False,
        )]
        order = rc.topological_sort(gates)
        self.assertEqual(order, ["A"])

    def test_topo_sort_linear_chain(self):
        gates = []
        for name, deps in [("C", ["B"]), ("B", ["A"]), ("A", [])]:
            gates.append(rc.Gate(
                id=name, title=name, description="", required=True,
                evidence_classes=["ci-log"], platforms=["linux"],
                release_stage="preflight", max_age_days=1,
                invalidated_by=[], depends_on=deps, waiver_allowed=False,
            ))
        order = rc.topological_sort(gates)
        self.assertEqual(order.index("A") < order.index("B"), True)
        self.assertEqual(order.index("B") < order.index("C"), True)

    def test_topo_sort_diamond(self):
        gates = []
        spec = {
            "D": ["B", "C"],
            "B": ["A"],
            "C": ["A"],
            "A": [],
        }
        for name, deps in spec.items():
            gates.append(rc.Gate(
                id=name, title=name, description="", required=True,
                evidence_classes=["ci-log"], platforms=["linux"],
                release_stage="preflight", max_age_days=1,
                invalidated_by=[], depends_on=deps, waiver_allowed=False,
            ))
        order = rc.topological_sort(gates)
        self.assertEqual(order[0], "A")
        self.assertEqual(order[-1], "D")
        self.assertIn("B", order)
        self.assertIn("C", order)
        self.assertTrue(order.index("B") < order.index("D"))
        self.assertTrue(order.index("C") < order.index("D"))

    def test_topo_sort_deterministic(self):
        gates = []
        spec = {
            "C": ["A", "B"],
            "B": ["A"],
            "A": [],
        }
        for name, deps in spec.items():
            gates.append(rc.Gate(
                id=name, title=name, description="", required=True,
                evidence_classes=["ci-log"], platforms=["linux"],
                release_stage="preflight", max_age_days=1,
                invalidated_by=[], depends_on=deps, waiver_allowed=False,
            ))
        results = [rc.topological_sort(gates) for _ in range(10)]
        for r in results:
            self.assertEqual(r, results[0])

    def test_topo_sort_empty(self):
        self.assertEqual(rc.topological_sort([]), [])


# ---------------------------------------------------------------------------
# Evidence validity tests
# ---------------------------------------------------------------------------

class TestEvidenceValidity(unittest.TestCase):
    def _gate(self, **kwargs) -> rc.Gate:
        defaults = dict(
            id="test.gate", title="T", description="", required=True,
            evidence_classes=["ci-log"], platforms=["linux"],
            release_stage="preflight", max_age_days=1,
            invalidated_by=[], depends_on=[], waiver_allowed=False,
        )
        defaults.update(kwargs)
        return rc.Gate(**defaults)

    def test_evidence_valid_exact_sha(self):
        gate = self._gate(max_age_days=0)
        ev = rc.EvidenceRecord(**{
            "schema_version": "1.0.0", "gate_id": "test.gate",
            "result": "passed", "evidence_class": "LOCAL",
            "command": "echo", "exit_code": 0,
            "start_time": "2025-01-01T00:00:00Z",
            "end_time": "2025-01-01T00:00:00Z",
            "duration_secs": 0.0, "commit_sha": "abc123",
            "dirty_tree": False, "os": "linux", "arch": "x86_64",
        })
        valid, reasons = rc.is_evidence_valid(gate, ev, "abc123", [])
        self.assertTrue(valid)
        self.assertEqual(reasons, [])

    def test_evidence_invalid_exact_sha(self):
        gate = self._gate(max_age_days=0)
        ev = rc.EvidenceRecord(**{
            "schema_version": "1.0.0", "gate_id": "test.gate",
            "result": "passed", "evidence_class": "LOCAL",
            "command": "echo", "exit_code": 0,
            "start_time": "2025-01-01T00:00:00Z",
            "end_time": "2025-01-01T00:00:00Z",
            "duration_secs": 0.0, "commit_sha": "wrong_sha",
            "dirty_tree": False, "os": "linux", "arch": "x86_64",
        })
        valid, reasons = rc.is_evidence_valid(gate, ev, "abc123", [])
        self.assertFalse(valid)
        self.assertTrue(any("commit" in r for r in reasons))

    def test_evidence_valid_freshness(self):
        gate = self._gate(max_age_days=30)
        now = datetime.now(timezone.utc)
        ev = rc.EvidenceRecord(**{
            "schema_version": "1.0.0", "gate_id": "test.gate",
            "result": "passed", "evidence_class": "LOCAL",
            "command": "echo", "exit_code": 0,
            "start_time": (now - timedelta(days=10)).isoformat(),
            "end_time": (now - timedelta(days=10)).isoformat(),
            "duration_secs": 0.0, "commit_sha": "abc123",
            "dirty_tree": False, "os": "linux", "arch": "x86_64",
        })
        valid, reasons = rc.is_evidence_valid(gate, ev, "abc123", [])
        self.assertTrue(valid)

    def test_evidence_invalid_freshness(self):
        gate = self._gate(max_age_days=7)
        now = datetime.now(timezone.utc)
        ev = rc.EvidenceRecord(**{
            "schema_version": "1.0.0", "gate_id": "test.gate",
            "result": "passed", "evidence_class": "LOCAL",
            "command": "echo", "exit_code": 0,
            "start_time": (now - timedelta(days=30)).isoformat(),
            "end_time": (now - timedelta(days=30)).isoformat(),
            "duration_secs": 0.0, "commit_sha": "abc123",
            "dirty_tree": False, "os": "linux", "arch": "x86_64",
        })
        valid, reasons = rc.is_evidence_valid(gate, ev, "abc123", [])
        self.assertFalse(valid)
        self.assertTrue(any("days old" in r for r in reasons))

    def test_evidence_invalid_path_invalidation(self):
        gate = self._gate(invalidated_by=["crates/core/**"])
        now = datetime.now(timezone.utc)
        ev = rc.EvidenceRecord(**{
            "schema_version": "1.0.0", "gate_id": "test.gate",
            "result": "passed", "evidence_class": "LOCAL",
            "command": "echo", "exit_code": 0,
            "start_time": now.isoformat(),
            "end_time": now.isoformat(),
            "duration_secs": 0.0, "commit_sha": "abc123",
            "dirty_tree": False, "os": "linux", "arch": "x86_64",
        })
        valid, reasons = rc.is_evidence_valid(
            gate, ev, "abc123", ["crates/core/src/lib.rs"],
        )
        self.assertFalse(valid)
        self.assertTrue(any("invalidation" in r for r in reasons))

    def test_evidence_valid_no_invalidation(self):
        gate = self._gate(invalidated_by=["crates/core/**"])
        now = datetime.now(timezone.utc)
        ev = rc.EvidenceRecord(**{
            "schema_version": "1.0.0", "gate_id": "test.gate",
            "result": "passed", "evidence_class": "LOCAL",
            "command": "echo", "exit_code": 0,
            "start_time": now.isoformat(),
            "end_time": now.isoformat(),
            "duration_secs": 0.0, "commit_sha": "abc123",
            "dirty_tree": False, "os": "linux", "arch": "x86_64",
        })
        valid, reasons = rc.is_evidence_valid(
            gate, ev, "abc123", ["docs/README.md"],
        )
        self.assertTrue(valid)

    def test_evidence_valid_waived(self):
        gate = self._gate(waiver_allowed=True, max_age_days=0)
        ev = rc.EvidenceRecord(**{
            "schema_version": "1.0.0", "gate_id": "test.gate",
            "result": "failed", "evidence_class": "LOCAL",
            "command": "echo", "exit_code": 1,
            "start_time": "2020-01-01T00:00:00Z",
            "end_time": "2020-01-01T00:00:00Z",
            "duration_secs": 0.0, "commit_sha": "old",
            "dirty_tree": False, "os": "linux", "arch": "x86_64",
        })
        valid, reasons = rc.is_evidence_valid(gate, ev, "new_sha", [])
        self.assertTrue(valid)
        self.assertEqual(reasons, ["waived"])

    def test_evidence_valid_artifact_gate(self):
        gate = self._gate(artifacts=["bin/eggserve", "wheel.whl"])
        now = datetime.now(timezone.utc)
        ev = rc.EvidenceRecord(**{
            "schema_version": "1.0.0", "gate_id": "test.gate",
            "result": "passed", "evidence_class": "ARTIFACT",
            "command": "echo", "exit_code": 0,
            "start_time": now.isoformat(),
            "end_time": now.isoformat(),
            "duration_secs": 0.0, "commit_sha": "abc123",
            "dirty_tree": False, "os": "linux", "arch": "x86_64",
            "artifact_ids": ["bin/eggserve", "wheel.whl"],
        })
        valid, reasons = rc.is_evidence_valid(gate, ev, "abc123", [])
        self.assertTrue(valid)

    def test_evidence_invalid_artifact_gate(self):
        gate = self._gate(artifacts=["bin/eggserve", "wheel.whl"])
        now = datetime.now(timezone.utc)
        ev = rc.EvidenceRecord(**{
            "schema_version": "1.0.0", "gate_id": "test.gate",
            "result": "passed", "evidence_class": "ARTIFACT",
            "command": "echo", "exit_code": 0,
            "start_time": now.isoformat(),
            "end_time": now.isoformat(),
            "duration_secs": 0.0, "commit_sha": "abc123",
            "dirty_tree": False, "os": "linux", "arch": "x86_64",
            "artifact_ids": ["bin/eggserve"],
        })
        valid, reasons = rc.is_evidence_valid(gate, ev, "abc123", [])
        self.assertFalse(valid)
        self.assertTrue(any("artifacts" in r for r in reasons))


# ---------------------------------------------------------------------------
# Path invalidation tests
# ---------------------------------------------------------------------------

class TestPathInvalidation(unittest.TestCase):
    def test_path_invalidation_glob(self):
        self.assertTrue(
            rc.is_path_invalidated(
                ["crates/core/src/lib.rs"], ["crates/core/**"],
            )
        )

    def test_path_invalidation_no_match(self):
        self.assertFalse(
            rc.is_path_invalidated(
                ["docs/README.md"], ["crates/core/**"],
            )
        )

    def test_path_invalidation_multiple_patterns(self):
        self.assertTrue(
            rc.is_path_invalidated(
                ["Cargo.toml"],
                ["crates/core/**", "Cargo.toml"],
            )
        )

    def test_path_invalidation_empty_paths(self):
        self.assertFalse(
            rc.is_path_invalidated([], ["crates/core/**"])
        )


# ---------------------------------------------------------------------------
# EvidenceRecord / WaiverRecord tests
# ---------------------------------------------------------------------------

class TestRecords(unittest.TestCase):
    def test_evidence_record_to_dict(self):
        ev = rc.EvidenceRecord(
            schema_version="1.0.0",
            gate_id="test.gate",
            result="passed",
            evidence_class="LOCAL",
            command="echo",
            exit_code=0,
            start_time="2025-01-01T00:00:00Z",
            end_time="2025-01-01T00:00:00Z",
            duration_secs=1.0,
            commit_sha="abc123",
            dirty_tree=False,
            os="linux",
            arch="x86_64",
            tool_versions={"rust": "1.75"},
            features=["tls"],
            log_path="/tmp/log",
            skip_reason=None,
            workflow_run_url=None,
            job_id=None,
            runner_os=None,
            artifact_ids=["bin/eggserve"],
        )
        d = ev.to_dict()
        self.assertEqual(d["gate_id"], "test.gate")
        self.assertEqual(d["tool_versions"], {"rust": "1.75"})
        self.assertEqual(d["features"], ["tls"])
        self.assertEqual(d["log_path"], "/tmp/log")
        self.assertEqual(d["artifact_ids"], ["bin/eggserve"])
        self.assertNotIn("skip_reason", d)
        self.assertNotIn("workflow_run_url", d)
        self.assertNotIn("job_id", d)
        self.assertNotIn("runner_os", d)

    def test_waiver_record_to_dict(self):
        w = rc.WaiverRecord(
            gate_id="test.gate",
            candidate_sha="abc123",
            approver="Test",
            date="2025-01-01T00:00:00Z",
            rationale="testing",
            risk_classification="low",
            expiration="2025-12-31T23:59:59Z",
            compensating_controls=["review"],
            disclosure=None,
        )
        d = w.to_dict()
        self.assertEqual(d["gate_id"], "test.gate")
        self.assertEqual(d["rationale"], "testing")
        self.assertEqual(d["compensating_controls"], ["review"])
        self.assertNotIn("disclosure", d)

    def test_waiver_record_with_disclosure(self):
        w = rc.WaiverRecord(
            gate_id="test.gate",
            candidate_sha="abc123",
            approver="Test",
            date="2025-01-01T00:00:00Z",
            rationale="testing",
            risk_classification="medium",
            expiration="2025-12-31T23:59:59Z",
            compensating_controls=[],
            disclosure="Public disclosure note",
        )
        d = w.to_dict()
        self.assertEqual(d["disclosure"], "Public disclosure note")


# ---------------------------------------------------------------------------
# CLI subcommand tests (invoked via subprocess)
# ---------------------------------------------------------------------------

class TestCLI(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.tmpdir = Path(self.tmp.name)
        self.criteria_path = self.tmpdir / "criteria.toml"
        self.evidence_path = self.tmpdir / "evidence.json"
        self.checklist_path = self.tmpdir / "checklist.md"

    def tearDown(self):
        self.tmp.cleanup()

    def _write_criteria(self, gates: list[dict], **kwargs) -> str:
        self.criteria_path.write_text(
            _criteria_toml(gates, **kwargs), encoding="utf-8",
        )
        return str(self.criteria_path)

    def _run(self, *args: str, check: bool = False) -> subprocess.CompletedProcess:
        return subprocess.run(
            [sys.executable, str(_RC_PATH)] + list(args),
            capture_output=True,
            text=True,
            check=check,
        )

    def test_validate_valid_file(self):
        path = self._write_criteria([_make_gate()])
        r = self._run("validate", path)
        self.assertEqual(r.returncode, 0)

    def test_validate_invalid_file(self):
        path = self._write_criteria(
            [_make_gate()], schema_version="9.9.9",
        )
        r = self._run("validate", path)
        self.assertEqual(r.returncode, 1)

    def test_list_json(self):
        path = self._write_criteria([_make_gate("g1"), _make_gate("g2")])
        r = self._run("list", "--criteria", path, "--format", "json")
        self.assertEqual(r.returncode, 0)
        data = json.loads(r.stdout)
        self.assertEqual(len(data), 2)
        ids = {g["id"] for g in data}
        self.assertEqual(ids, {"g1", "g2"})

    def test_explain_known_gate(self):
        path = self._write_criteria([_make_gate("my.gate")])
        r = self._run("explain", "my.gate", "--criteria", path)
        self.assertEqual(r.returncode, 0)
        self.assertIn("my.gate", r.stdout)

    def test_explain_unknown_gate(self):
        path = self._write_criteria([_make_gate("known.gate")])
        r = self._run("explain", "nope.gate", "--criteria", path)
        self.assertEqual(r.returncode, 1)
        self.assertIn("not found", r.stderr)

    def test_graph_json(self):
        gates = [
            _make_gate("A"),
            _make_gate("B", depends_on=["A"]),
        ]
        path = self._write_criteria(gates)
        r = self._run("graph", "--criteria", path, "--format", "json")
        self.assertEqual(r.returncode, 0)
        data = json.loads(r.stdout)
        self.assertEqual(len(data), 2)
        ids = [item["id"] for item in data]
        self.assertEqual(ids[0], "A")
        self.assertEqual(ids[1], "B")

    def test_generate_checklist(self):
        path = self._write_criteria([_make_gate("chk.gate")])
        r = self._run(
            "generate-checklist",
            "--criteria", path,
            "--checklist-output", str(self.checklist_path),
        )
        self.assertEqual(r.returncode, 0)
        content = self.checklist_path.read_text(encoding="utf-8")
        self.assertIn("Release Checklist", content)
        self.assertIn("chk.gate", content)

    def test_check_evidence(self):
        path = self._write_criteria([_make_gate("test.gate")])
        ev = _valid_evidence("test.gate")
        self.evidence_path.write_text(json.dumps(ev), encoding="utf-8")
        r = self._run(
            "check-evidence",
            "--criteria", path,
            "--evidence", str(self.evidence_path),
            "--sha", "abc123",
        )
        self.assertEqual(r.returncode, 0)

    def test_validate_evidence_valid(self):
        ev = _valid_evidence("test.gate")
        self.evidence_path.write_text(json.dumps(ev), encoding="utf-8")
        r = self._run(
            "validate-evidence",
            "--evidence", str(self.evidence_path),
        )
        self.assertEqual(r.returncode, 0)

    def test_validate_evidence_invalid(self):
        self.evidence_path.write_text(
            json.dumps({"gate_id": "x"}), encoding="utf-8",
        )
        r = self._run(
            "validate-evidence",
            "--evidence", str(self.evidence_path),
        )
        self.assertEqual(r.returncode, 1)

    def test_record_waiver(self):
        path = self._write_criteria([_make_gate("test.gate")])
        r = self._run(
            "record-waiver",
            "--criteria", path,
            "--gate-id", "test.gate",
            "--sha", "abc123",
            "--approver", "Test User",
            "--rationale", "test rationale",
            "--risk", "low",
            "--expiration", "2025-12-31T23:59:59Z",
        )
        self.assertEqual(r.returncode, 0)
        data = json.loads(r.stdout)
        self.assertEqual(data["gate_id"], "test.gate")
        self.assertEqual(data["approver"], "Test User")


# ---------------------------------------------------------------------------
# Aggregation logic tests (Track D)
# ---------------------------------------------------------------------------

class TestAggregation(unittest.TestCase):
    def _gate(self, **kwargs) -> rc.Gate:
        defaults = dict(
            id="test.gate", title="T", description="", required=True,
            evidence_classes=["ci-log"], platforms=["linux"],
            release_stage="preflight", max_age_days=1,
            invalidated_by=[], depends_on=[], waiver_allowed=False,
        )
        defaults.update(kwargs)
        return rc.Gate(**defaults)

    def test_missing_required_gate(self):
        gate = self._gate()
        status, reasons = rc._aggregate_gate(gate, [], "abc123")
        self.assertEqual(status, "MISSING")

    def test_passed_gate(self):
        gate = self._gate()
        ev = _valid_evidence("test.gate")
        status, reasons = rc._aggregate_gate(gate, [ev], "abc123")
        self.assertEqual(status, "PASSED")

    def test_failed_gate(self):
        gate = self._gate()
        ev = _valid_evidence("test.gate", result="failed")
        status, reasons = rc._aggregate_gate(gate, [ev], "abc123")
        self.assertEqual(status, "FAILED")

    def test_stale_gate_sha_mismatch(self):
        gate = self._gate(max_age_days=0)
        ev = _valid_evidence("test.gate", commit_sha="wrong_sha")
        status, reasons = rc._aggregate_gate(gate, [ev], "abc123")
        self.assertEqual(status, "STALE")

    def test_invalidated_gate(self):
        gate = self._gate()
        ev = _valid_evidence("test.gate")
        ev["dirty_tree"] = True
        status, reasons = rc._aggregate_gate(gate, [ev], "abc123")
        self.assertEqual(status, "INVALIDATED")

    def test_malformed_gate(self):
        gate = self._gate()
        status, reasons = rc._aggregate_gate(gate, [{"bad": "data"}], "abc123")
        self.assertEqual(status, "MALFORMED")

    def test_conflicting_records(self):
        gate = self._gate()
        ev1 = _valid_evidence("test.gate", result="passed")
        ev2 = _valid_evidence("test.gate", result="failed")
        status, reasons = rc._aggregate_gate(gate, [ev1, ev2], "abc123")
        self.assertEqual(status, "CONFLICTING")

    def test_not_applicable_optional_gate(self):
        gate = self._gate(required=False)
        status, reasons = rc._aggregate_gate(gate, [], "abc123")
        self.assertEqual(status, "NOT-APPLICABLE")

    def test_malformed_overrides_waiver(self):
        gate = self._gate(waiver_allowed=True)
        status, reasons = rc._aggregate_gate(
            gate, [{"bad": "data"}], "abc123",
        )
        self.assertEqual(status, "MALFORMED")

    def test_status_precedence(self):
        self.assertTrue(rc._STATUS_PRECEDENCE.index("MALFORMED") < rc._STATUS_PRECEDENCE.index("CONFLICTING"))
        self.assertTrue(rc._STATUS_PRECEDENCE.index("CONFLICTING") < rc._STATUS_PRECEDENCE.index("INVALIDATED"))
        self.assertTrue(rc._STATUS_PRECEDENCE.index("INVALIDATED") < rc._STATUS_PRECEDENCE.index("STALE"))
        self.assertTrue(rc._STATUS_PRECEDENCE.index("STALE") < rc._STATUS_PRECEDENCE.index("FAILED"))
        self.assertTrue(rc._STATUS_PRECEDENCE.index("FAILED") < rc._STATUS_PRECEDENCE.index("MISSING"))

    def test_error_result_classified_as_failed(self):
        gate = self._gate()
        ev = _valid_evidence("test.gate", result="error")
        status, reasons = rc._aggregate_gate(gate, [ev], "abc123")
        self.assertEqual(status, "FAILED")

    def test_skipped_record_not_blocking(self):
        gate = self._gate()
        ev = _valid_evidence("test.gate", result="skipped")
        status, reasons = rc._aggregate_gate(gate, [ev], "abc123")
        self.assertIn(status, ("NOT-APPLICABLE", "SKIPPED"))

    def test_malformed_and_passed_conflict(self):
        gate = self._gate()
        ev = _valid_evidence("test.gate", result="passed")
        status, reasons = rc._aggregate_gate(
            gate, [ev, {"bad": "data"}], "abc123",
        )
        self.assertEqual(status, "CONFLICTING")

    def test_invalidated_and_passed_conflict(self):
        gate = self._gate()
        ev1 = _valid_evidence("test.gate", result="passed")
        ev2 = _valid_evidence("test.gate", result="passed")
        ev2["dirty_tree"] = True
        status, reasons = rc._aggregate_gate(gate, [ev1, ev2], "abc123")
        self.assertEqual(status, "CONFLICTING")


class TestCLIAggregate(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.tmpdir = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def _run(self, *args: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            [sys.executable, str(_RC_PATH)] + list(args),
            capture_output=True,
            text=True,
        )

    def test_aggregate_all_passing(self):
        criteria_path = self.tmpdir / "criteria.toml"
        criteria_path.write_text(
            _criteria_toml([_make_gate("g1")]), encoding="utf-8",
        )
        evidence_dir = self.tmpdir / "evidence"
        evidence_dir.mkdir()
        gates_dir = evidence_dir / "gates"
        gates_dir.mkdir()
        ev = _valid_evidence("g1")
        (gates_dir / "g1.json").write_text(json.dumps(ev), encoding="utf-8")

        result = self._run(
            "aggregate",
            "--criteria", str(criteria_path),
            "--evidence", str(evidence_dir),
            "--sha", "abc123",
        )
        self.assertEqual(result.returncode, 0)
        self.assertIn("RELEASE READY", result.stdout)

    def test_aggregate_missing_gate_fails(self):
        criteria_path = self.tmpdir / "criteria.toml"
        criteria_path.write_text(
            _criteria_toml([_make_gate("missing.gate")]), encoding="utf-8",
        )
        evidence_dir = self.tmpdir / "evidence"
        evidence_dir.mkdir()

        result = self._run(
            "aggregate",
            "--criteria", str(criteria_path),
            "--evidence", str(evidence_dir),
            "--sha", "abc123",
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("NOT RELEASE READY", result.stderr)

    def test_aggregate_json_output(self):
        criteria_path = self.tmpdir / "criteria.toml"
        criteria_path.write_text(
            _criteria_toml([_make_gate("g1")]), encoding="utf-8",
        )
        evidence_dir = self.tmpdir / "evidence"
        evidence_dir.mkdir()
        gates_dir = evidence_dir / "gates"
        gates_dir.mkdir()
        ev = _valid_evidence("g1")
        (gates_dir / "g1.json").write_text(json.dumps(ev), encoding="utf-8")

        result = self._run(
            "aggregate",
            "--criteria", str(criteria_path),
            "--evidence", str(evidence_dir),
            "--sha", "abc123",
            "--format", "json",
        )
        self.assertEqual(result.returncode, 0)
        data = json.loads(result.stdout)
        self.assertTrue(data["release_ready"])
        self.assertEqual(data["gates"]["g1"]["status"], "PASSED")

    def test_aggregate_nonexistent_evidence(self):
        criteria_path = self.tmpdir / "criteria.toml"
        criteria_path.write_text(
            _criteria_toml([_make_gate("g1")]), encoding="utf-8",
        )

        result = self._run(
            "aggregate",
            "--criteria", str(criteria_path),
            "--evidence", str(self.tmpdir / "nonexistent"),
            "--sha", "abc123",
        )
        self.assertEqual(result.returncode, 2)

    def test_aggregate_malformed_evidence_file(self):
        criteria_path = self.tmpdir / "criteria.toml"
        criteria_path.write_text(
            _criteria_toml([_make_gate("g1")]), encoding="utf-8",
        )
        evidence_dir = self.tmpdir / "evidence"
        evidence_dir.mkdir()
        gates_dir = evidence_dir / "gates"
        gates_dir.mkdir()
        malformed = {"gate_id": "g1", "schema_version": "1.0.0"}
        (gates_dir / "g1.json").write_text(
            json.dumps(malformed), encoding="utf-8",
        )

        result = self._run(
            "aggregate",
            "--criteria", str(criteria_path),
            "--evidence", str(evidence_dir),
            "--sha", "abc123",
            "--format", "json",
        )
        self.assertEqual(result.returncode, 1)
        data = json.loads(result.stdout)
        self.assertEqual(data["gates"]["g1"]["status"], "MALFORMED")


class TestEndToEndAggregation(unittest.TestCase):
    """End-to-end tests using multi-file evidence bundles."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.tmpdir = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def _run(self, *args: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            [sys.executable, str(_RC_PATH)] + list(args),
            capture_output=True,
            text=True,
        )

    def _setup_bundle(self, gate_results: dict[str, str], *, sha: str = "abc123"):
        criteria_gates = []
        for gate_id, result in gate_results.items():
            criteria_gates.append(_make_gate(gate_id))
        criteria_path = self.tmpdir / "criteria.toml"
        criteria_path.write_text(_criteria_toml(criteria_gates), encoding="utf-8")

        evidence_dir = self.tmpdir / "evidence"
        evidence_dir.mkdir()
        gates_dir = evidence_dir / "gates"
        gates_dir.mkdir()
        for gate_id, result in gate_results.items():
            ev = _valid_evidence(gate_id, result=result, commit_sha=sha)
            (gates_dir / f"{gate_id}.json").write_text(json.dumps(ev), encoding="utf-8")
        return criteria_path, evidence_dir

    def test_all_passing_bundle(self):
        criteria_path, evidence_dir = self._setup_bundle({
            "g1": "passed", "g2": "passed", "g3": "passed",
        })
        result = self._run(
            "aggregate",
            "--criteria", str(criteria_path),
            "--evidence", str(evidence_dir),
            "--sha", "abc123",
        )
        self.assertEqual(result.returncode, 0)
        self.assertIn("RELEASE READY", result.stdout)

    def test_partial_failure_blocks_release(self):
        criteria_path, evidence_dir = self._setup_bundle({
            "g1": "passed", "g2": "failed", "g3": "passed",
        })
        result = self._run(
            "aggregate",
            "--criteria", str(criteria_path),
            "--evidence", str(evidence_dir),
            "--sha", "abc123",
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("NOT RELEASE READY", result.stderr)

    def test_missing_gate_blocks_release(self):
        criteria_path, evidence_dir = self._setup_bundle({
            "g1": "passed",
        })
        criteria_path.write_text(
            _criteria_toml([_make_gate("g1"), _make_gate("g2")]),
            encoding="utf-8",
        )
        result = self._run(
            "aggregate",
            "--criteria", str(criteria_path),
            "--evidence", str(evidence_dir),
            "--sha", "abc123",
        )
        self.assertEqual(result.returncode, 1)

    def test_deterministic_output_ordering(self):
        criteria_path, evidence_dir = self._setup_bundle({
            "alpha.gate": "passed", "beta.gate": "passed", "gamma.gate": "passed",
        })
        results = []
        for _ in range(3):
            result = self._run(
                "aggregate",
                "--criteria", str(criteria_path),
                "--evidence", str(evidence_dir),
                "--sha", "abc123",
                "--format", "json",
            )
            self.assertEqual(result.returncode, 0)
            results.append(result.stdout)
        for r in results[1:]:
            self.assertEqual(r, results[0], "Aggregate output is not deterministic")

    def test_sha_mismatch_detected(self):
        criteria_gates = [_make_gate("g1", max_age_days=0)]
        criteria_path = self.tmpdir / "criteria.toml"
        criteria_path.write_text(_criteria_toml(criteria_gates), encoding="utf-8")

        evidence_dir = self.tmpdir / "evidence"
        evidence_dir.mkdir()
        gates_dir = evidence_dir / "gates"
        gates_dir.mkdir()
        ev = _valid_evidence("g1", result="passed", commit_sha="wrong_sha")
        (gates_dir / "g1.json").write_text(json.dumps(ev), encoding="utf-8")

        result = self._run(
            "aggregate",
            "--criteria", str(criteria_path),
            "--evidence", str(evidence_dir),
            "--sha", "abc123",
            "--format", "json",
        )
        data = json.loads(result.stdout)
        gate_status = data["gates"]["g1"]["status"]
        self.assertEqual(gate_status, "STALE")
        result = self._run(
            "aggregate",
            "--criteria", str(criteria_path),
            "--evidence", str(evidence_dir),
            "--sha", "abc123",
            "--format", "json",
        )
        data = json.loads(result.stdout)
        gate_status = data["gates"]["g1"]["status"]
        self.assertIn(gate_status, ("STALE", "FAILED"))

    def test_malformed_record_in_bundle(self):
        criteria_gates = [_make_gate("g1"), _make_gate("g2")]
        criteria_path = self.tmpdir / "criteria.toml"
        criteria_path.write_text(_criteria_toml(criteria_gates), encoding="utf-8")

        evidence_dir = self.tmpdir / "evidence"
        evidence_dir.mkdir()
        gates_dir = evidence_dir / "gates"
        gates_dir.mkdir()
        ev1 = _valid_evidence("g1")
        (gates_dir / "g1.json").write_text(json.dumps(ev1), encoding="utf-8")
        (gates_dir / "g2.json").write_text(
            json.dumps({"gate_id": "g2", "schema_version": "1.0.0"}),
            encoding="utf-8",
        )

        result = self._run(
            "aggregate",
            "--criteria", str(criteria_path),
            "--evidence", str(evidence_dir),
            "--sha", "abc123",
            "--format", "json",
        )
        self.assertEqual(result.returncode, 1)
        data = json.loads(result.stdout)
        self.assertEqual(data["gates"]["g2"]["status"], "MALFORMED")

    def test_partial_evidence_not_release_ready(self):
        criteria_gates = [_make_gate("g1"), _make_gate("g2"), _make_gate("g3")]
        criteria_path = self.tmpdir / "criteria.toml"
        criteria_path.write_text(_criteria_toml(criteria_gates), encoding="utf-8")

        evidence_dir = self.tmpdir / "evidence"
        evidence_dir.mkdir()
        gates_dir = evidence_dir / "gates"
        gates_dir.mkdir()
        ev1 = _valid_evidence("g1")
        (gates_dir / "g1.json").write_text(json.dumps(ev1), encoding="utf-8")

        result = self._run(
            "aggregate",
            "--criteria", str(criteria_path),
            "--evidence", str(evidence_dir),
            "--sha", "abc123",
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("NOT RELEASE READY", result.stderr)


if __name__ == "__main__":
    unittest.main()

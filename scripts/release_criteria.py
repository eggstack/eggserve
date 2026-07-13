#!/usr/bin/env python3
"""Release criteria validator and query CLI for eggserve.

Parses and validates ``release/criteria.toml``, the single source of truth for
every release gate.  Provides machine-readable JSON output and human-readable
text, dependency-graph analysis, and Markdown checklist generation.

Requires Python 3.11+ (``tomllib``).  No external dependencies.
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import platform
import re
import subprocess
import sys
from collections import deque
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

SCHEMA_VERSION = "1.0.0"

# Canonical evidence classes from Plan 044 (uppercase) plus the lowercase
# kebab-case values currently used in the checked-in criteria.toml.
VALID_EVIDENCE_CLASSES = frozenset({
    "LOCAL", "GITHUB", "ARTIFACT", "HUMAN", "CONFIG",
    "ci-log", "lint-output", "test-output", "audit-output",
    "deny-output", "package-output", "release-output", "checksum",
    "provenance", "approval-record", "wheel",
})

# Ordered tuple for deterministic iteration; frozenset variant for O(1) lookup.
_RELEASE_STAGES_LIST = (
    "preflight",
    "qualification",
    "artifact",
    "approval",
    "publication",
    "post-publication",
)
VALID_RELEASE_STAGES = frozenset(_RELEASE_STAGES_LIST)

VALID_PLATFORMS = frozenset({
    "linux",
    "macos",
    "windows",
})

# Fields that every [[gate]] must contain.
REQUIRED_GATE_FIELDS = frozenset({
    "id",
    "title",
    "description",
    "required",
    "evidence_classes",
    "platforms",
    "release_stage",
    "max_age_days",
    "invalidated_by",
    "depends_on",
    "waiver_allowed",
})

OPTIONAL_GATE_FIELDS = frozenset({
    "command",
    "workflow_job",
    "features",
    "artifacts",
    "triggers",
    "security_relevance",
    "doc_ref",
    "waiver_authority",
})

ALL_KNOWN_GATE_FIELDS = REQUIRED_GATE_FIELDS | OPTIONAL_GATE_FIELDS


# ---------------------------------------------------------------------------
# Data model
# ---------------------------------------------------------------------------

@dataclass
class Gate:
    """A single release gate definition."""

    id: str
    title: str
    description: str
    required: bool
    evidence_classes: list[str]
    platforms: list[str]
    release_stage: str
    max_age_days: int
    invalidated_by: list[str]
    depends_on: list[str]
    waiver_allowed: bool
    command: str | None = None
    workflow_job: str | None = None
    features: list[str] = field(default_factory=list)
    artifacts: list[str] = field(default_factory=list)
    triggers: list[str] = field(default_factory=list)
    security_relevance: bool = False
    doc_ref: str | None = None
    waiver_authority: str | None = None
    line_hint: int | None = None

    def to_dict(self) -> dict[str, Any]:
        """Serialize gate to a plain dictionary."""
        d: dict[str, Any] = {
            "id": self.id,
            "title": self.title,
            "description": self.description,
            "required": self.required,
            "evidence_classes": self.evidence_classes,
            "platforms": self.platforms,
            "release_stage": self.release_stage,
            "max_age_days": self.max_age_days,
            "invalidated_by": self.invalidated_by,
            "depends_on": self.depends_on,
            "waiver_allowed": self.waiver_allowed,
        }
        if self.command is not None:
            d["command"] = self.command
        if self.workflow_job is not None:
            d["workflow_job"] = self.workflow_job
        if self.features:
            d["features"] = self.features
        if self.artifacts:
            d["artifacts"] = self.artifacts
        if self.triggers:
            d["triggers"] = self.triggers
        if self.security_relevance:
            d["security_relevance"] = self.security_relevance
        if self.doc_ref is not None:
            d["doc_ref"] = self.doc_ref
        if self.waiver_authority is not None:
            d["waiver_authority"] = self.waiver_authority
        return d


@dataclass
class EvidenceRecord:
    """A single evidence record for a gate execution."""

    schema_version: str
    gate_id: str
    result: str  # "passed", "failed", "skipped", "not-applicable", "error"
    evidence_class: str  # "LOCAL", "GITHUB", "ARTIFACT", "HUMAN", "CONFIG"
    command: str
    exit_code: int
    start_time: str  # ISO 8601
    end_time: str  # ISO 8601
    duration_secs: float
    commit_sha: str
    dirty_tree: bool
    os: str
    arch: str
    tool_versions: dict[str, str] = field(default_factory=dict)
    features: list[str] = field(default_factory=list)
    log_path: str | None = None
    skip_reason: str | None = None
    target_triple: str | None = None
    invalidation_info: str | None = None
    workflow_run_url: str | None = None  # GITHUB only
    job_id: str | None = None  # GITHUB only
    runner_os: str | None = None  # GITHUB only
    artifact_ids: list[str] = field(default_factory=list)  # ARTIFACT only

    def to_dict(self) -> dict[str, Any]:
        d: dict[str, Any] = {
            "schema_version": self.schema_version,
            "gate_id": self.gate_id,
            "result": self.result,
            "evidence_class": self.evidence_class,
            "command": self.command,
            "exit_code": self.exit_code,
            "start_time": self.start_time,
            "end_time": self.end_time,
            "duration_secs": self.duration_secs,
            "commit_sha": self.commit_sha,
            "dirty_tree": self.dirty_tree,
            "os": self.os,
            "arch": self.arch,
            "tool_versions": self.tool_versions,
            "features": self.features,
        }
        if self.log_path is not None:
            d["log_path"] = self.log_path
        if self.skip_reason is not None:
            d["skip_reason"] = self.skip_reason
        if self.target_triple is not None:
            d["target_triple"] = self.target_triple
        if self.invalidation_info is not None:
            d["invalidation_info"] = self.invalidation_info
        if self.workflow_run_url is not None:
            d["workflow_run_url"] = self.workflow_run_url
        if self.job_id is not None:
            d["job_id"] = self.job_id
        if self.runner_os is not None:
            d["runner_os"] = self.runner_os
        if self.artifact_ids:
            d["artifact_ids"] = self.artifact_ids
        return d


@dataclass
class WaiverRecord:
    """A waiver for a specific gate."""

    gate_id: str
    candidate_sha: str
    approver: str
    date: str  # ISO 8601
    rationale: str
    risk_classification: str  # "low", "medium", "high", "critical"
    expiration: str  # ISO 8601
    compensating_controls: list[str] = field(default_factory=list)
    disclosure: str | None = None

    def to_dict(self) -> dict[str, Any]:
        d: dict[str, Any] = {
            "gate_id": self.gate_id,
            "candidate_sha": self.candidate_sha,
            "approver": self.approver,
            "date": self.date,
            "rationale": self.rationale,
            "risk_classification": self.risk_classification,
            "expiration": self.expiration,
            "compensating_controls": self.compensating_controls,
        }
        if self.disclosure is not None:
            d["disclosure"] = self.disclosure
        return d


@dataclass
class PlatformInfo:
    """Platform support declaration from [platforms] section."""

    name: str
    runner: str
    targets: list[str]


@dataclass
class LanguageInfo:
    """Language support declaration from [languages] section."""

    name: str
    version: str
    toolchain: str = ""
    edition: str = ""
    constraint: str = ""


@dataclass
class Meta:
    """File-level metadata from [meta] section."""

    schema_version: str = ""
    project: str = ""
    version: str = ""
    description: str = ""


@dataclass
class CriteriaFile:
    """Parsed and validated criteria file."""

    meta: Meta
    gates: list[Gate]
    platforms: dict[str, PlatformInfo] = field(default_factory=dict)
    languages: dict[str, LanguageInfo] = field(default_factory=dict)
    source_path: str = ""

    def gate_by_id(self) -> dict[str, Gate]:
        """Index gates by their id."""
        return {g.id: g for g in self.gates}


# ---------------------------------------------------------------------------
# Diagnostic helpers
# ---------------------------------------------------------------------------

@dataclass
class Diagnostic:
    """A single validation diagnostic."""

    level: str  # "error", "warning"
    message: str
    file: str = ""
    line: int | None = None
    gate_id: str | None = None
    context: str | None = None

    def to_dict(self) -> dict[str, Any]:
        d: dict[str, Any] = {"level": self.level, "message": self.message}
        if self.file:
            d["file"] = self.file
        if self.line is not None:
            d["line"] = self.line
        if self.gate_id:
            d["gate_id"] = self.gate_id
        if self.context:
            d["context"] = self.context
        return d


# ---------------------------------------------------------------------------
# TOML line-number estimation
# ---------------------------------------------------------------------------

def _estimate_gate_lines(raw_text: str) -> dict[str, int]:
    """Map gate IDs to approximate TOML line numbers."""
    result: dict[str, int] = {}
    current_id: str | None = None
    for lineno, line in enumerate(raw_text.splitlines(), start=1):
        stripped = line.strip()
        if stripped.startswith("[[gate]]"):
            current_id = None
        elif current_id is None and stripped.startswith("id"):
            m = re.match(r'id\s*=\s*["\']([^"\']+)["\']', stripped)
            if m:
                current_id = m.group(1)
                result[current_id] = lineno
    return result


def _schema_version_line(raw_text: str) -> int | None:
    for lineno, line in enumerate(raw_text.splitlines(), start=1):
        stripped = line.strip()
        if "schema_version" in stripped:
            return lineno
    return None


# ---------------------------------------------------------------------------
# Parser
# ---------------------------------------------------------------------------

def parse_criteria_file(path: str) -> tuple[CriteriaFile, str]:
    """Parse a criteria TOML file and return a CriteriaFile plus raw text."""
    try:
        import tomllib
    except ModuleNotFoundError:
        print("ERROR: tomllib not available; requires Python 3.11+", file=sys.stderr)
        sys.exit(2)

    file_path = Path(path)
    if not file_path.exists():
        print(f"ERROR: file not found: {path}", file=sys.stderr)
        sys.exit(2)

    raw_text = file_path.read_text(encoding="utf-8")

    try:
        data = tomllib.loads(raw_text)
    except tomllib.TOMLDecodeError as exc:
        print(f"ERROR: TOML parse error in {path}: {exc}", file=sys.stderr)
        sys.exit(2)

    gate_lines = _estimate_gate_lines(raw_text)

    # Parse [meta] section
    raw_meta = data.get("meta", {})
    meta = Meta(
        schema_version=str(raw_meta.get("schema_version", "")),
        project=raw_meta.get("project", ""),
        version=raw_meta.get("version", ""),
        description=raw_meta.get("description", ""),
    )

    # Parse [[gate]] array
    gates: list[Gate] = []
    raw_gates = data.get("gate", [])
    if not isinstance(raw_gates, list):
        raw_gates = [raw_gates]

    for raw in raw_gates:
        gate_id = raw.get("id", "")
        line_hint = gate_lines.get(gate_id)
        gate = Gate(
            id=gate_id,
            title=raw.get("title", ""),
            description=raw.get("description", ""),
            required=raw.get("required", False),
            evidence_classes=raw.get("evidence_classes", []),
            platforms=raw.get("platforms", []),
            release_stage=raw.get("release_stage", ""),
            max_age_days=raw.get("max_age_days", 90),
            invalidated_by=raw.get("invalidated_by", []),
            depends_on=raw.get("depends_on", []),
            waiver_allowed=raw.get("waiver_allowed", False),
            command=raw.get("command"),
            workflow_job=raw.get("workflow_job"),
            features=raw.get("features", []),
            artifacts=raw.get("artifacts", []),
            triggers=raw.get("triggers", []),
            security_relevance=raw.get("security_relevance", False),
            doc_ref=raw.get("doc_ref"),
            waiver_authority=raw.get("waiver_authority"),
            line_hint=line_hint,
        )
        gates.append(gate)

    # Parse [platforms] section
    platforms: dict[str, PlatformInfo] = {}
    raw_platforms = data.get("platforms", {})
    if isinstance(raw_platforms, dict):
        for name, info in raw_platforms.items():
            if isinstance(info, dict):
                platforms[name] = PlatformInfo(
                    name=name,
                    runner=info.get("runner", ""),
                    targets=info.get("targets", []),
                )

    # Parse [languages] section
    languages: dict[str, LanguageInfo] = {}
    raw_languages = data.get("languages", {})
    if isinstance(raw_languages, dict):
        for name, info in raw_languages.items():
            if isinstance(info, dict):
                languages[name] = LanguageInfo(
                    name=name,
                    version=info.get("version", ""),
                    toolchain=info.get("toolchain", ""),
                    edition=info.get("edition", ""),
                    constraint=info.get("constraint", ""),
                )

    criteria = CriteriaFile(
        meta=meta,
        gates=gates,
        platforms=platforms,
        languages=languages,
        source_path=path,
    )
    return criteria, raw_text


# ---------------------------------------------------------------------------
# Validator
# ---------------------------------------------------------------------------

class CriteriaValidator:
    """Validates a parsed CriteriaFile against all rules."""

    def __init__(self, criteria: CriteriaFile) -> None:
        self.criteria = criteria
        self.diagnostics: list[Diagnostic] = []

    def _error(
        self,
        message: str,
        line: int | None = None,
        gate_id: str | None = None,
        context: str | None = None,
    ) -> None:
        self.diagnostics.append(Diagnostic(
            level="error",
            message=message,
            file=self.criteria.source_path,
            line=line,
            gate_id=gate_id,
            context=context,
        ))

    def _warning(
        self,
        message: str,
        line: int | None = None,
        gate_id: str | None = None,
    ) -> None:
        self.diagnostics.append(Diagnostic(
            level="warning",
            message=message,
            file=self.criteria.source_path,
            line=line,
            gate_id=gate_id,
        ))

    def validate(self) -> list[Diagnostic]:
        """Run all validation checks and return diagnostics."""
        self._check_schema_version()
        self._check_duplicate_ids()
        self._check_required_fields()
        self._check_unknown_fields()
        self._check_evidence_classes()
        self._check_release_stages()
        self._check_platforms()
        self._check_commands_nonempty()
        self._check_unknown_dependencies()
        self._check_no_dependency_cycles()
        self._check_config_only_execution_gates()
        return self.diagnostics

    # -- checks ------------------------------------------------------------

    def _check_schema_version(self) -> None:
        sv = self.criteria.meta.schema_version
        if sv != SCHEMA_VERSION:
            self._error(
                f"schema_version is \"{sv}\", expected \"{SCHEMA_VERSION}\"",
                line=_schema_version_line(
                    Path(self.criteria.source_path).read_text(encoding="utf-8")
                ) if Path(self.criteria.source_path).exists() else None,
            )

    def _check_duplicate_ids(self) -> None:
        seen: dict[str, int] = {}
        for gate in self.criteria.gates:
            if gate.id in seen:
                self._error(
                    f"duplicate gate id: '{gate.id}'",
                    line=gate.line_hint,
                    gate_id=gate.id,
                )
            else:
                seen[gate.id] = 1

    def _check_required_fields(self) -> None:
        # Fields where an empty string is truly missing.
        _STRING_FIELDS = {"id", "title", "description", "release_stage"}
        # Fields where an empty list is valid (explicitly "none").
        _LIST_FIELDS = {"depends_on", "features", "artifacts", "triggers",
                        "evidence_classes", "platforms", "invalidated_by"}
        for gate in self.criteria.gates:
            for fld in REQUIRED_GATE_FIELDS:
                val = getattr(gate, fld, None)
                if val is None:
                    self._error(
                        f"missing required field '{fld}'",
                        line=gate.line_hint,
                        gate_id=gate.id,
                    )
                elif fld in _STRING_FIELDS and isinstance(val, str) and not val:
                    self._error(
                        f"required field '{fld}' is empty",
                        line=gate.line_hint,
                        gate_id=gate.id,
                    )
            if not gate.id:
                self._error(
                    "gate has empty id",
                    line=gate.line_hint,
                )

    def _check_unknown_fields(self) -> None:
        for gate in self.criteria.gates:
            raw = gate.to_dict()
            for key in raw:
                if key not in ALL_KNOWN_GATE_FIELDS:
                    self._warning(
                        f"unknown gate field '{key}'",
                        line=gate.line_hint,
                        gate_id=gate.id,
                    )

    def _check_evidence_classes(self) -> None:
        for gate in self.criteria.gates:
            for ec in gate.evidence_classes:
                if ec not in VALID_EVIDENCE_CLASSES:
                    self._error(
                        f"invalid evidence_class '{ec}'",
                        line=gate.line_hint,
                        gate_id=gate.id,
                    )

    def _check_release_stages(self) -> None:
        for gate in self.criteria.gates:
            if gate.release_stage not in VALID_RELEASE_STAGES:
                self._error(
                    f"invalid release_stage '{gate.release_stage}'; "
                    f"valid values: {sorted(VALID_RELEASE_STAGES)}",
                    line=gate.line_hint,
                    gate_id=gate.id,
                )

    def _check_platforms(self) -> None:
        for gate in self.criteria.gates:
            for plat in gate.platforms:
                if plat not in VALID_PLATFORMS:
                    self._error(
                        f"invalid platform '{plat}'; "
                        f"valid values: {sorted(VALID_PLATFORMS)}",
                        line=gate.line_hint,
                        gate_id=gate.id,
                    )

    def _check_commands_nonempty(self) -> None:
        for gate in self.criteria.gates:
            if gate.required and gate.command is not None:
                if not gate.command.strip():
                    self._error(
                        "required gate has empty command",
                        line=gate.line_hint,
                        gate_id=gate.id,
                    )

    def _check_unknown_dependencies(self) -> None:
        known = self.criteria.gate_by_id()
        for gate in self.criteria.gates:
            for dep in gate.depends_on:
                if dep not in known:
                    self._error(
                        f"depends_on references unknown gate '{dep}'",
                        line=gate.line_hint,
                        gate_id=gate.id,
                    )

    def _check_no_dependency_cycles(self) -> None:
        """Detect cycles via Kahn's algorithm."""
        known = self.criteria.gate_by_id()
        in_degree: dict[str, int] = {gid: 0 for gid in known}
        adj: dict[str, list[str]] = {gid: [] for gid in known}

        for gate in self.criteria.gates:
            for dep in gate.depends_on:
                if dep in known:
                    adj[dep].append(gate.id)
                    in_degree[gate.id] += 1

        queue: deque[str] = deque()
        for gid, deg in in_degree.items():
            if deg == 0:
                queue.append(gid)

        sorted_count = 0
        while queue:
            node = queue.popleft()
            sorted_count += 1
            for neighbor in adj[node]:
                in_degree[neighbor] -= 1
                if in_degree[neighbor] == 0:
                    queue.append(neighbor)

        if sorted_count != len(known):
            cycle_nodes = [gid for gid, deg in in_degree.items() if deg > 0]
            self._error(
                f"dependency cycle detected involving gates: {cycle_nodes}",
            )

    def _check_config_only_execution_gates(self) -> None:
        for gate in self.criteria.gates:
            if gate.required:
                non_config = [ec for ec in gate.evidence_classes
                              if ec not in ("CONFIG", "config")]
                if not non_config and gate.evidence_classes:
                    self._warning(
                        "required gate has only CONFIG evidence class; "
                        "CONFIG alone cannot satisfy execution gates",
                        line=gate.line_hint,
                        gate_id=gate.id,
                    )


# ---------------------------------------------------------------------------
# Topological sort
# ---------------------------------------------------------------------------

def topological_sort(gates: list[Gate]) -> list[str]:
    """Return gate IDs in topological order (dependencies first).

    When multiple gates are ready at the same step, they are emitted in
    alphabetical order for deterministic output.
    """
    gate_map = {g.id: g for g in gates}
    in_degree: dict[str, int] = {g.id: 0 for g in gates}
    adj: dict[str, list[str]] = {g.id: [] for g in gates}

    for gate in gates:
        for dep in gate.depends_on:
            if dep in gate_map:
                adj[dep].append(gate.id)
                in_degree[gate.id] += 1

    queue: deque[str] = deque()
    for gid, deg in in_degree.items():
        if deg == 0:
            queue.append(gid)

    order: list[str] = []
    while queue:
        batch = sorted(queue)
        queue.clear()
        for node in batch:
            order.append(node)
            for neighbor in sorted(adj[node]):
                in_degree[neighbor] -= 1
                if in_degree[neighbor] == 0:
                    queue.append(neighbor)

    return order


# ---------------------------------------------------------------------------
# Evidence freshness and invalidation
# ---------------------------------------------------------------------------

def is_path_invalidated(changed_paths: list[str], patterns: list[str]) -> bool:
    """Check if any changed path matches any invalidation pattern."""
    for path in changed_paths:
        for pattern in patterns:
            if fnmatch.fnmatch(path, pattern):
                return True
    return False


def is_evidence_valid(
    gate: Gate,
    evidence: EvidenceRecord,
    candidate_sha: str,
    changed_paths: list[str],
) -> tuple[bool, list[str]]:
    """Check if evidence is valid for a gate given current state.

    Returns (is_valid, reasons) where reasons lists invalidity causes.
    """
    reasons: list[str] = []

    if gate.waiver_allowed:
        return True, ["waived"]

    if evidence.gate_id != gate.id:
        reasons.append(f"evidence gate_id '{evidence.gate_id}' != gate '{gate.id}'")

    if evidence.result not in ("passed", "skipped", "not-applicable"):
        reasons.append(f"evidence result is '{evidence.result}', not a passing state")

    if gate.max_age_days == 0:
        if evidence.commit_sha != candidate_sha:
            reasons.append(
                f"exact-SHA gate requires commit '{candidate_sha}', "
                f"evidence has '{evidence.commit_sha}'"
            )
    else:
        try:
            evidence_time = datetime.fromisoformat(evidence.end_time)
            now = datetime.now(timezone.utc)
            if evidence_time.tzinfo is None:
                evidence_time = evidence_time.replace(tzinfo=timezone.utc)
            age_days = (now - evidence_time).total_seconds() / 86400.0
            if age_days > gate.max_age_days:
                reasons.append(
                    f"evidence is {age_days:.1f} days old, "
                    f"max allowed is {gate.max_age_days}"
                )
        except (ValueError, TypeError):
            reasons.append(f"could not parse evidence end_time '{evidence.end_time}'")

    if gate.invalidated_by and is_path_invalidated(changed_paths, gate.invalidated_by):
        reasons.append(
            f"changed paths match invalidation patterns: {gate.invalidated_by}"
        )

    if gate.artifacts:
        if not evidence.artifact_ids:
            reasons.append("gate requires artifacts but evidence has none")
        else:
            missing = [a for a in gate.artifacts if a not in evidence.artifact_ids]
            if missing:
                reasons.append(f"missing required artifacts: {missing}")

    return len(reasons) == 0, reasons


# ---------------------------------------------------------------------------
# Subcommands
# ---------------------------------------------------------------------------

def cmd_validate(args: argparse.Namespace) -> int:
    """Validate a criteria file."""
    criteria, _ = parse_criteria_file(args.criteria)
    validator = CriteriaValidator(criteria)
    diagnostics = validator.validate()

    errors = [d for d in diagnostics if d.level == "error"]
    warnings = [d for d in diagnostics if d.level == "warning"]

    if args.format == "json":
        result = {
            "valid": len(errors) == 0,
            "errors": [d.to_dict() for d in errors],
            "warnings": [d.to_dict() for d in warnings],
            "gate_count": len(criteria.gates),
            "schema_version": criteria.meta.schema_version,
        }
        print(json.dumps(result, indent=2))
    else:
        for d in diagnostics:
            loc = ""
            if d.line is not None:
                loc = f":{d.line}"
            if d.gate_id:
                loc += f" (gate {d.gate_id})"
            prefix = "ERROR" if d.level == "error" else "WARN "
            print(f"{prefix} {d.message}{loc}", file=sys.stderr)

        if errors:
            print(f"\n{len(errors)} error(s), {len(warnings)} warning(s)",
                  file=sys.stderr)
        else:
            print(f"\nOK: {len(criteria.gates)} gate(s), "
                  f"{len(warnings)} warning(s)")

    return 1 if errors else 0


def cmd_list(args: argparse.Namespace) -> int:
    """List all gates."""
    criteria, _ = parse_criteria_file(args.criteria)

    if args.format == "json":
        gates_data = [g.to_dict() for g in criteria.gates]
        print(json.dumps(gates_data, indent=2))
    else:
        if not criteria.gates:
            print("No gates defined.")
            return 0
        stages: dict[str, list[Gate]] = {}
        for gate in criteria.gates:
            stages.setdefault(gate.release_stage, []).append(gate)

        for stage in _RELEASE_STAGES_LIST:
            gates = stages.get(stage, [])
            if not gates:
                continue
            print(f"\n## {stage}")
            for gate in gates:
                req = "required" if gate.required else "advisory"
                evid = ", ".join(gate.evidence_classes)
                print(f"  {gate.id:<40s} [{req:<8s}] evidence={evid}")

    return 0


def cmd_explain(args: argparse.Namespace) -> int:
    """Explain a single gate."""
    criteria, _ = parse_criteria_file(args.criteria)
    gate_map = criteria.gate_by_id()

    gate = gate_map.get(args.gate_id)
    if gate is None:
        print(f"ERROR: gate '{args.gate_id}' not found", file=sys.stderr)
        available = sorted(gate_map.keys())
        if available:
            print(f"Available gates: {', '.join(available)}", file=sys.stderr)
        return 1

    if args.format == "json":
        d = gate.to_dict()
        d["depends_on_details"] = []
        for dep_id in gate.depends_on:
            dep_gate = gate_map.get(dep_id)
            if dep_gate:
                d["depends_on_details"].append({
                    "id": dep_gate.id,
                    "title": dep_gate.title,
                    "required": dep_gate.required,
                })
        print(json.dumps(d, indent=2))
    else:
        lines = [
            f"Gate: {gate.id}",
            f"Title: {gate.title}",
            f"Description: {gate.description}",
            f"Required: {'yes' if gate.required else 'no'}",
            f"Evidence classes: {', '.join(gate.evidence_classes)}",
            f"Platforms: {', '.join(gate.platforms)}",
            f"Release stage: {gate.release_stage}",
            f"Max age (days): {gate.max_age_days}",
            f"Waiver allowed: {'yes' if gate.waiver_allowed else 'no'}",
        ]
        if gate.command:
            lines.append(f"Command: {gate.command}")
        if gate.workflow_job:
            lines.append(f"Workflow job: {gate.workflow_job}")
        if gate.features:
            lines.append(f"Features: {', '.join(gate.features)}")
        if gate.artifacts:
            lines.append(f"Artifacts: {', '.join(gate.artifacts)}")
        if gate.security_relevance:
            lines.append("Security relevant: yes")
        if gate.doc_ref:
            lines.append(f"Doc ref: {gate.doc_ref}")
        if gate.waiver_authority:
            lines.append(f"Waiver authority: {gate.waiver_authority}")
        if gate.depends_on:
            lines.append(f"Depends on: {', '.join(gate.depends_on)}")
        if gate.invalidated_by:
            lines.append("Invalidated by:")
            for pat in gate.invalidated_by:
                lines.append(f"  - {pat}")
        print("\n".join(lines))

    return 0


def cmd_graph(args: argparse.Namespace) -> int:
    """Show dependency graph in topological order."""
    criteria, _ = parse_criteria_file(args.criteria)
    gate_map = criteria.gate_by_id()
    order = topological_sort(criteria.gates)

    if args.format == "json":
        graph_data = []
        for gid in order:
            gate = gate_map[gid]
            graph_data.append({
                "id": gid,
                "depends_on": gate.depends_on,
                "required": gate.required,
            })
        print(json.dumps(graph_data, indent=2))
    else:
        if not order:
            print("No gates defined.")
            return 0

        print("Dependency graph (topological order):\n")
        max_id_len = max(len(gid) for gid in order)
        for idx, gid in enumerate(order, start=1):
            gate = gate_map[gid]
            deps = gate.depends_on
            req = "*" if gate.required else " "
            dep_str = f" -> {', '.join(deps)}" if deps else ""
            print(f"  {idx:2d}. {req} {gid:<{max_id_len}s}{dep_str}")

        print(f"\n  {len(order)} gate(s); * = required")

    return 0


def _load_evidence_bundle(
    evidence_path: str,
) -> dict[str, dict[str, Any]]:
    """Load evidence records from a directory or file and index by gate_id.

    Supports:
    - A directory containing per-gate JSON files (e.g. gates/*.json)
    - A single JSON file containing a list of records
    - A manifest.json with a "gates" array
    """
    path = Path(evidence_path)
    records: dict[str, dict[str, Any]] = {}

    if path.is_dir():
        # Look for a manifest.json first
        manifest = path / "manifest.json"
        if manifest.exists():
            try:
                data = json.loads(manifest.read_text(encoding="utf-8"))
                for gate_result in data.get("gates", []):
                    gid = gate_result.get("gate_id", "")
                    if gid:
                        records[gid] = gate_result
            except Exception:
                pass

        # Also load per-gate JSON files from gates/ subdirectory
        gates_dir = path / "gates"
        if gates_dir.is_dir():
            for f in sorted(gates_dir.glob("*.json")):
                try:
                    data = json.loads(f.read_text(encoding="utf-8"))
                    gid = data.get("gate_id", "")
                    if gid:
                        records[gid] = data
                except Exception:
                    pass

        # Also look for top-level JSON files
        for f in sorted(path.glob("*.json")):
            if f.name == "manifest.json":
                continue
            try:
                data = json.loads(f.read_text(encoding="utf-8"))
                if isinstance(data, list):
                    for item in data:
                        gid = item.get("gate_id", "")
                        if gid:
                            records[gid] = item
                elif isinstance(data, dict):
                    gid = data.get("gate_id", "")
                    if gid:
                        records[gid] = data
            except Exception:
                pass
    elif path.is_file():
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
            if isinstance(data, list):
                for item in data:
                    gid = item.get("gate_id", "")
                    if gid:
                        records[gid] = item
            elif isinstance(data, dict):
                # Could be a manifest
                for gate_result in data.get("gates", []):
                    gid = gate_result.get("gate_id", "")
                    if gid:
                        records[gid] = gate_result
                # Or a single record
                gid = data.get("gate_id", "")
                if gid:
                    records[gid] = data
        except Exception:
            pass

    return records


def _compute_gate_status(
    gate: Gate,
    evidence: dict[str, Any] | None,
    candidate_sha: str,
) -> tuple[str, str]:
    """Compute the checklist status for a gate given available evidence.

    Returns (status, evidence_ref) where status is one of:
    PENDING, PASSED, FAILED, SKIPPED, NOT-APPLICABLE, STALE, INVALIDATED,
    WAIVED, ERROR.
    """
    if evidence is None:
        return "PENDING", "TBD"

    result = evidence.get("result", "")
    commit_sha = evidence.get("commit_sha", "")
    dirty_tree = evidence.get("dirty_tree", False)
    end_time = evidence.get("end_time", "")

    # Check dirty-tree
    if dirty_tree:
        return "INVALIDATED", f"dirty-tree run"

    # Check SHA match
    if gate.max_age_days == 0 and commit_sha and candidate_sha:
        if commit_sha != candidate_sha:
            return "STALE", f"SHA mismatch: {commit_sha[:12]}"

    # Check freshness
    if end_time and gate.max_age_days > 0:
        try:
            evidence_time = datetime.fromisoformat(end_time)
            now = datetime.now(timezone.utc)
            if evidence_time.tzinfo is None:
                evidence_time = evidence_time.replace(tzinfo=timezone.utc)
            age_days = (now - evidence_time).total_seconds() / 86400.0
            if age_days > gate.max_age_days:
                return "STALE", f"{age_days:.0f}d old (max {gate.max_age_days})"
        except (ValueError, TypeError):
            return "ERROR", f"unparseable end_time"

    # Map result to status
    status_map = {
        "passed": "PASSED",
        "failed": "FAILED",
        "skipped": "SKIPPED",
        "not-applicable": "NOT-APPLICABLE",
        "error": "ERROR",
    }
    status = status_map.get(result, "ERROR")

    # Build evidence reference
    workflow_url = evidence.get("workflow_run_url", "")
    job_id = evidence.get("job_id", "")
    run_id = evidence.get("workflow_run_url", "")
    evidence_ref = f"{commit_sha[:12]}" if commit_sha else "TBD"
    if workflow_url:
        evidence_ref = f"[{commit_sha[:12]}]({workflow_url})"
    elif commit_sha:
        evidence_ref = commit_sha[:12]

    return status, evidence_ref


def cmd_generate_checklist(args: argparse.Namespace) -> int:
    """Generate a release checklist in Markdown."""
    criteria, _ = parse_criteria_file(args.criteria)
    stages: dict[str, list[Gate]] = {}
    for gate in criteria.gates:
        stages.setdefault(gate.release_stage, []).append(gate)

    # Load evidence if provided
    evidence_records: dict[str, dict[str, Any]] = {}
    candidate_sha = getattr(args, "sha", "") or ""
    if args.evidence:
        evidence_records = _load_evidence_bundle(args.evidence)
        # Try to extract candidate SHA from evidence if not provided
        if not candidate_sha:
            for gid, ev in evidence_records.items():
                sha = ev.get("commit_sha", "")
                if sha:
                    candidate_sha = sha
                    break

    lines: list[str] = []
    lines.append(
        "<!-- AUTO-GENERATED by scripts/release_criteria.py"
        " -- do not edit manually -->"
    )
    lines.append("")
    lines.append("# Release Checklist")
    lines.append("")
    lines.append(f"Schema version: {criteria.meta.schema_version}")
    lines.append(f"Total gates: {len(criteria.gates)}")
    if candidate_sha:
        lines.append(f"Candidate SHA: `{candidate_sha}`")
    if evidence_records:
        lines.append(f"Evidence records: {len(evidence_records)}")
    lines.append("")

    # Count statuses for summary
    status_counts: dict[str, int] = {}

    for stage in _RELEASE_STAGES_LIST:
        gates = stages.get(stage, [])
        if not gates:
            continue
        lines.append(f"## {stage.replace('-', ' ').title()}")
        lines.append("")
        lines.append(
            "| Gate ID | Title | Required | Evidence Class "
            "| Status | Evidence |"
        )
        lines.append(
            "|---------|-------|----------|----------------"
            "|--------|----------|"
        )
        for gate in sorted(gates, key=lambda g: g.id):
            req = "yes" if gate.required else "no"
            evid = ", ".join(gate.evidence_classes)
            ev_data = evidence_records.get(gate.id)
            status, evidence_ref = _compute_gate_status(
                gate, ev_data, candidate_sha,
            )
            status_counts[status] = status_counts.get(status, 0) + 1
            lines.append(
                f"| `{gate.id}` | {gate.title} | {req} | {evid} "
                f"| {status} | {evidence_ref} |"
            )
        lines.append("")

    # Summary
    if status_counts:
        lines.append("## Summary")
        lines.append("")
        for status in ["PASSED", "FAILED", "SKIPPED", "NOT-APPLICABLE",
                        "STALE", "INVALIDATED", "WAIVED", "ERROR", "PENDING"]:
            count = status_counts.get(status, 0)
            if count > 0:
                lines.append(f"- {status}: {count}")
        lines.append("")

    if criteria.platforms:
        lines.append("## Platform Support")
        lines.append("")
        lines.append("| Platform | Runner | Targets |")
        lines.append("|----------|--------|---------|")
        for name, info in criteria.platforms.items():
            targets = ", ".join(info.targets)
            lines.append(f"| {name} | {info.runner} | {targets} |")
        lines.append("")

    lines.append("---")
    lines.append("")
    lines.append(
        "*This checklist was generated from `release/criteria.toml`.*"
    )
    lines.append("*Do not edit by hand; regenerate with:*")
    lines.append("```sh")
    lines.append(
        "python scripts/release_criteria.py generate-checklist"
        " --criteria release/criteria.toml"
    )
    lines.append("```")
    lines.append("")

    generated = "\n".join(lines)

    if args.check:
        checklist_path = Path(args.checklist_output)
        if not checklist_path.exists():
            print(
                f"ERROR: checklist file not found: {checklist_path}",
                file=sys.stderr,
            )
            return 1
        existing = checklist_path.read_text(encoding="utf-8")
        if existing.rstrip("\n") != generated.rstrip("\n"):
            print(
                "ERROR: generated checklist does not match committed file; "
                "regenerate with: python scripts/release_criteria.py"
                " generate-checklist",
                file=sys.stderr,
            )
            return 1
        print("OK: checklist matches generated output")
        return 0

    output_path = Path(args.checklist_output)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(generated, encoding="utf-8")
    print(f"Written: {output_path}")
    return 0


# ---------------------------------------------------------------------------
# Evidence aggregation (Track D — fail-closed aggregation)
# ---------------------------------------------------------------------------

# Status precedence (lower index = higher severity, overrides in aggregation)
_STATUS_PRECEDENCE = [
    "MALFORMED",
    "CONFLICTING",
    "INVALIDATED",
    "STALE",
    "FAILED",
    "MISSING",
    "DEFERRED",
    "NOT-APPLICABLE",
    "WAIVED",
    "PASSED",
]


def _classify_evidence_record(
    gate: Gate,
    evidence: dict[str, Any],
    candidate_sha: str,
) -> str:
    """Classify a single evidence record into a checklist status.

    Returns one of: PASSED, FAILED, SKIPPED, NOT-APPLICABLE, STALE,
    INVALIDATED, WAIVED, ERROR, MALFORMED.
    """
    if not isinstance(evidence, dict):
        return "MALFORMED"

    required_fields = {"schema_version", "gate_id", "result", "commit_sha"}
    if not required_fields.issubset(evidence.keys()):
        return "MALFORMED"

    result = evidence.get("result", "")
    if result not in ("passed", "failed", "skipped", "not-applicable", "error"):
        return "MALFORMED"

    commit_sha = evidence.get("commit_sha", "")
    dirty_tree = evidence.get("dirty_tree", False)
    end_time = evidence.get("end_time", "")

    if dirty_tree:
        return "INVALIDATED"

    if gate.max_age_days == 0 and commit_sha and candidate_sha:
        if commit_sha != candidate_sha:
            return "STALE"

    if end_time and gate.max_age_days > 0:
        try:
            evidence_time = datetime.fromisoformat(end_time)
            now = datetime.now(timezone.utc)
            if evidence_time.tzinfo is None:
                evidence_time = evidence_time.replace(tzinfo=timezone.utc)
            age_days = (now - evidence_time).total_seconds() / 86400.0
            if age_days > gate.max_age_days:
                return "STALE"
        except (ValueError, TypeError):
            return "MALFORMED"

    status_map = {
        "passed": "PASSED",
        "failed": "FAILED",
        "skipped": "SKIPPED",
        "not-applicable": "NOT-APPLICABLE",
        "error": "FAILED",
    }
    return status_map.get(result, "MALFORMED")


def _aggregate_gate(
    gate: Gate,
    evidence_records: list[dict[str, Any]],
    candidate_sha: str,
) -> tuple[str, list[str]]:
    """Aggregate evidence for a single gate into a final status.

    Returns (final_status, reasons) where reasons explains the decision.
    A waiver can never hide MALFORMED or CONFLICTING evidence.
    """
    if not evidence_records:
        if gate.required:
            return "MISSING", ["no evidence record found for required gate"]
        return "NOT-APPLICABLE", ["gate not required and no evidence"]

    classifications = []
    for ev in evidence_records:
        cls = _classify_evidence_record(gate, ev, candidate_sha)
        classifications.append((cls, ev))

    non_trivial = {c for c, _ in classifications if c not in ("NOT-APPLICABLE", "SKIPPED")}
    if len(non_trivial) > 1:
        return "CONFLICTING", [
            f"multiple non-trivial statuses: {sorted(non_trivial)}"
        ]

    has_malformed = any(c == "MALFORMED" for c, _ in classifications)
    if has_malformed:
        return "MALFORMED", ["one or more evidence records are malformed"]

    has_invalidated = any(c == "INVALIDATED" for c, _ in classifications)
    if has_invalidated:
        return "INVALIDATED", ["evidence produced by dirty tree"]

    has_stale = any(c == "STALE" for c, _ in classifications)
    if has_stale:
        return "STALE", ["evidence is stale (SHA mismatch or age exceeded)"]

    for precedence in _STATUS_PRECEDENCE:
        if any(c == precedence for c, _ in classifications):
            reasons = []
            for c, ev in classifications:
                if c == precedence:
                    gate_id = ev.get("gate_id", "?")
                    reasons.append(f"gate {gate_id}: {c.lower()}")
            return precedence, reasons

    return "NOT-APPLICABLE", ["all records are not-applicable or skipped"]


def cmd_aggregate(args: argparse.Namespace) -> int:
    """Validate an evidence bundle against all criteria gates."""
    criteria, _ = parse_criteria_file(args.criteria)
    gate_map = criteria.gate_by_id()

    evidence_path = Path(args.evidence)
    if not evidence_path.exists():
        print(f"ERROR: evidence path not found: {args.evidence}", file=sys.stderr)
        return 2

    all_records: dict[str, list[dict[str, Any]]] = {}
    evidence_dir = evidence_path

    if evidence_path.is_dir():
        gates_dir = evidence_dir / "gates"
        if gates_dir.is_dir():
            for f in sorted(gates_dir.glob("*.json")):
                try:
                    data = json.loads(f.read_text(encoding="utf-8"))
                    gid = data.get("gate_id", "")
                    if gid:
                        all_records.setdefault(gid, []).append(data)
                except Exception:
                    pass

        for f in sorted(evidence_dir.glob("*.json")):
            if f.name == "manifest.json":
                continue
            try:
                data = json.loads(f.read_text(encoding="utf-8"))
                if isinstance(data, list):
                    for item in data:
                        gid = item.get("gate_id", "")
                        if gid:
                            all_records.setdefault(gid, []).append(item)
                elif isinstance(data, dict):
                    gid = data.get("gate_id", "")
                    if gid:
                        all_records.setdefault(gid, []).append(data)
            except Exception:
                pass

    candidate_sha = args.sha or ""

    gate_results: dict[str, tuple[str, list[str]]] = {}
    for gate_id, gate in gate_map.items():
        records = all_records.get(gate_id, [])
        status, reasons = _aggregate_gate(gate, records, candidate_sha)
        gate_results[gate_id] = (status, reasons)

    release_ready = True
    for gate_id, (status, _reasons) in gate_results.items():
        gate = gate_map[gate_id]
        if gate.required and status != "PASSED":
            release_ready = False

    if args.format == "json":
        output = {
            "release_ready": release_ready,
            "candidate_sha": candidate_sha,
            "gates": {
                gid: {"status": s, "reasons": r}
                for gid, (s, r) in gate_results.items()
            },
        }
        print(json.dumps(output, indent=2))
    else:
        for gate_id in sorted(gate_results.keys()):
            status, reasons = gate_results[gate_id]
            gate = gate_map[gate_id]
            req = "required" if gate.required else "optional"
            reasons_str = ""
            if reasons:
                reasons_str = f"  ({'; '.join(reasons)})"
            print(f"  [{status:<17s}] {gate_id:<40s} {req}{reasons_str}")

        print()
        if release_ready:
            print("RELEASE READY: all required gates have passing evidence.")
        else:
            failing = [
                gid for gid, (s, _) in gate_results.items()
                if gate_map[gid].required and s != "PASSED"
            ]
            print(
                f"NOT RELEASE READY: {len(failing)} required gate(s) "
                f"not satisfied: {', '.join(sorted(failing))}",
                file=sys.stderr,
            )

    return 0 if release_ready else 1


def cmd_check_evidence(args: argparse.Namespace) -> int:
    """Validate evidence against criteria gates."""
    criteria, _ = parse_criteria_file(args.criteria)
    gate_map = criteria.gate_by_id()

    evidence_path = Path(args.evidence)
    if not evidence_path.exists():
        print(f"ERROR: evidence file not found: {args.evidence}", file=sys.stderr)
        return 2

    raw = json.loads(evidence_path.read_text(encoding="utf-8"))
    if isinstance(raw, dict):
        raw = [raw]

    candidate_sha = args.sha
    changed_paths: list[str] = []
    if args.changed_paths:
        changed_paths = [p.strip() for p in args.changed_paths.split(",") if p.strip()]

    results: list[dict[str, Any]] = []
    all_valid = True

    for item in raw:
        evidence = EvidenceRecord(
            schema_version=item.get("schema_version", ""),
            gate_id=item.get("gate_id", ""),
            result=item.get("result", ""),
            evidence_class=item.get("evidence_class", ""),
            command=item.get("command", ""),
            exit_code=item.get("exit_code", 0),
            start_time=item.get("start_time", ""),
            end_time=item.get("end_time", ""),
            duration_secs=item.get("duration_secs", 0.0),
            commit_sha=item.get("commit_sha", ""),
            dirty_tree=item.get("dirty_tree", False),
            os=item.get("os", ""),
            arch=item.get("arch", ""),
            tool_versions=item.get("tool_versions", {}),
            features=item.get("features", []),
            log_path=item.get("log_path"),
            skip_reason=item.get("skip_reason"),
            target_triple=item.get("target_triple"),
            invalidation_info=item.get("invalidation_info"),
            workflow_run_url=item.get("workflow_run_url"),
            job_id=item.get("job_id"),
            runner_os=item.get("runner_os"),
            artifact_ids=item.get("artifact_ids", []),
        )

        gate = gate_map.get(evidence.gate_id)
        if gate is None:
            results.append({
                "gate_id": evidence.gate_id,
                "valid": False,
                "reasons": [f"unknown gate '{evidence.gate_id}'"],
            })
            all_valid = False
            continue

        valid, reasons = is_evidence_valid(gate, evidence, candidate_sha, changed_paths)
        results.append({
            "gate_id": evidence.gate_id,
            "valid": valid,
            "reasons": reasons,
        })
        if not valid:
            all_valid = False

    if args.format == "json":
        output = {
            "all_valid": all_valid,
            "candidate_sha": candidate_sha,
            "results": results,
        }
        print(json.dumps(output, indent=2))
    else:
        for r in results:
            status = "PASS" if r["valid"] else "FAIL"
            reasons_str = ""
            if r["reasons"]:
                reasons_str = f"  ({'; '.join(r['reasons'])})"
            print(f"  {status} {r['gate_id']}{reasons_str}")
        print()
        if all_valid:
            print("All evidence valid.")
        else:
            print("Some evidence is invalid.", file=sys.stderr)

    return 0 if all_valid else 1


def cmd_record_waiver(args: argparse.Namespace) -> int:
    """Record a waiver and output JSON to stdout."""
    now = datetime.now(timezone.utc).isoformat()
    waiver = WaiverRecord(
        gate_id=args.gate_id,
        candidate_sha=args.sha,
        approver=args.approver,
        date=now,
        rationale=args.rationale,
        risk_classification=args.risk,
        expiration=args.expiration,
        compensating_controls=[],
        disclosure=None,
    )
    print(json.dumps(waiver.to_dict(), indent=2))
    return 0


def cmd_validate_evidence(args: argparse.Namespace) -> int:
    """Validate an evidence JSON file against the schema."""
    evidence_path = Path(args.evidence)
    if not evidence_path.exists():
        print(f"ERROR: evidence file not found: {args.evidence}", file=sys.stderr)
        return 2

    raw = json.loads(evidence_path.read_text(encoding="utf-8"))
    if isinstance(raw, dict):
        raw = [raw]

    errors: list[str] = []
    required_fields = {
        "schema_version", "gate_id", "result", "evidence_class",
        "command", "exit_code", "start_time", "end_time",
        "duration_secs", "commit_sha", "dirty_tree", "os", "arch",
    }
    valid_results = {"passed", "failed", "skipped", "not-applicable", "error"}
    valid_classes = {"LOCAL", "GITHUB", "ARTIFACT", "HUMAN", "CONFIG"}

    for idx, item in enumerate(raw):
        prefix = f"record[{idx}]"
        for fld in required_fields:
            if fld not in item:
                errors.append(f"{prefix}: missing required field '{fld}'")

        if "result" in item and item["result"] not in valid_results:
            errors.append(
                f"{prefix}: invalid result '{item['result']}'; "
                f"valid: {sorted(valid_results)}"
            )

        if "evidence_class" in item and item["evidence_class"] not in valid_classes:
            errors.append(
                f"{prefix}: invalid evidence_class '{item['evidence_class']}'; "
                f"valid: {sorted(valid_classes)}"
            )

        if "exit_code" in item and not isinstance(item["exit_code"], int):
            errors.append(f"{prefix}: exit_code must be an integer")

        if "duration_secs" in item and not isinstance(
            item["duration_secs"], (int, float)
        ):
            errors.append(f"{prefix}: duration_secs must be a number")

        if "dirty_tree" in item and not isinstance(item["dirty_tree"], bool):
            errors.append(f"{prefix}: dirty_tree must be a boolean")

    if args.format == "json":
        output = {
            "valid": len(errors) == 0,
            "errors": errors,
            "record_count": len(raw),
        }
        print(json.dumps(output, indent=2))
    else:
        for err in errors:
            print(f"ERROR: {err}", file=sys.stderr)
        if errors:
            print(f"\n{len(errors)} error(s) in {len(raw)} record(s)",
                  file=sys.stderr)
        else:
            print(f"OK: {len(raw)} record(s) valid")

    return 1 if errors else 0


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="release_criteria",
        description="Release criteria validator and query CLI for eggserve.",
    )
    parser.add_argument(
        "--criteria",
        default="release/criteria.toml",
        help="Path to criteria.toml (default: release/criteria.toml)",
    )

    sub = parser.add_subparsers(dest="command", help="Available commands")

    # validate
    p_val = sub.add_parser(
        "validate", help="Validate a criteria file strictly",
    )
    p_val.add_argument(
        "criteria_file", help="Path to criteria TOML file",
    )
    p_val.add_argument(
        "--format",
        choices=["json", "text"],
        default="text",
        help="Output format (default: text)",
    )

    # list
    p_list = sub.add_parser("list", help="List all gates")
    p_list.add_argument(
        "--criteria",
        default="release/criteria.toml",
        help="Path to criteria TOML file",
    )
    p_list.add_argument(
        "--format",
        choices=["json", "text"],
        default="text",
        help="Output format (default: text)",
    )

    # explain
    p_explain = sub.add_parser(
        "explain", help="Show full details for one gate",
    )
    p_explain.add_argument("gate_id", help="Gate ID to explain")
    p_explain.add_argument(
        "--criteria",
        default="release/criteria.toml",
        help="Path to criteria TOML file",
    )
    p_explain.add_argument(
        "--format",
        choices=["json", "text"],
        default="text",
        help="Output format (default: text)",
    )

    # graph
    p_graph = sub.add_parser(
        "graph", help="Show dependency graph (topological order)",
    )
    p_graph.add_argument(
        "--criteria",
        default="release/criteria.toml",
        help="Path to criteria TOML file",
    )
    p_graph.add_argument(
        "--format",
        choices=["json", "text"],
        default="text",
        help="Output format (default: text)",
    )

    # generate-checklist
    p_gen = sub.add_parser(
        "generate-checklist",
        help="Generate a release checklist in Markdown",
    )
    p_gen.add_argument(
        "--criteria",
        default="release/criteria.toml",
        help="Path to criteria TOML file",
    )
    p_gen.add_argument(
        "--evidence",
        default=None,
        help="Path to evidence bundle (directory or JSON file) to merge into checklist",
    )
    p_gen.add_argument(
        "--sha",
        default="",
        help="Candidate commit SHA for freshness/validation checks",
    )
    p_gen.add_argument(
        "--check",
        action="store_true",
        help="Verify committed checklist matches generated output",
    )
    p_gen.add_argument(
        "--checklist-output",
        default="docs/release-checklist.md",
        help=(
            "Output path for generated checklist "
            "(default: docs/release-checklist.md)"
        ),
    )

    # check-evidence
    p_ce = sub.add_parser(
        "check-evidence",
        help="Validate evidence against criteria gates",
    )
    p_ce.add_argument(
        "--criteria",
        default="release/criteria.toml",
        help="Path to criteria TOML file",
    )
    p_ce.add_argument(
        "--evidence",
        required=True,
        help="Path to evidence JSON file",
    )
    p_ce.add_argument(
        "--sha",
        required=True,
        help="Candidate commit SHA",
    )
    p_ce.add_argument(
        "--changed-paths",
        default=None,
        help="Comma-separated list of changed paths",
    )
    p_ce.add_argument(
        "--format",
        choices=["json", "text"],
        default="text",
        help="Output format (default: text)",
    )

    # record-waiver
    p_rw = sub.add_parser(
        "record-waiver",
        help="Record a waiver (outputs JSON to stdout)",
    )
    p_rw.add_argument(
        "--criteria",
        default="release/criteria.toml",
        help="Path to criteria TOML file",
    )
    p_rw.add_argument(
        "--gate-id",
        required=True,
        help="Gate ID to waive",
    )
    p_rw.add_argument(
        "--sha",
        required=True,
        help="Candidate commit SHA",
    )
    p_rw.add_argument(
        "--approver",
        required=True,
        help="Name of the approver",
    )
    p_rw.add_argument(
        "--rationale",
        required=True,
        help="Rationale for the waiver",
    )
    p_rw.add_argument(
        "--risk",
        choices=["low", "medium", "high", "critical"],
        required=True,
        help="Risk classification",
    )
    p_rw.add_argument(
        "--expiration",
        required=True,
        help="Expiration date (ISO 8601)",
    )

    # validate-evidence
    p_ve = sub.add_parser(
        "validate-evidence",
        help="Validate an evidence JSON file against the schema",
    )
    p_ve.add_argument(
        "--evidence",
        required=True,
        help="Path to evidence JSON file",
    )
    p_ve.add_argument(
        "--format",
        choices=["json", "text"],
        default="text",
        help="Output format (default: text)",
    )

    # aggregate
    p_agg = sub.add_parser(
        "aggregate",
        help="Validate evidence bundle against all criteria gates",
    )
    p_agg.add_argument(
        "--criteria",
        default="release/criteria.toml",
        help="Path to criteria TOML file",
    )
    p_agg.add_argument(
        "--evidence",
        required=True,
        help="Path to evidence bundle directory",
    )
    p_agg.add_argument(
        "--sha",
        default="",
        help="Candidate commit SHA",
    )
    p_agg.add_argument(
        "--format",
        choices=["json", "text"],
        default="text",
        help="Output format (default: text)",
    )

    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()

    if args.command is None:
        parser.print_help()
        return 1

    handlers = {
        "validate": lambda: (setattr(args, "criteria", args.criteria_file),
                             cmd_validate(args))[1],
        "list": lambda: cmd_list(args),
        "explain": lambda: cmd_explain(args),
        "graph": lambda: cmd_graph(args),
        "generate-checklist": lambda: cmd_generate_checklist(args),
        "check-evidence": lambda: cmd_check_evidence(args),
        "record-waiver": lambda: cmd_record_waiver(args),
        "validate-evidence": lambda: cmd_validate_evidence(args),
        "aggregate": lambda: cmd_aggregate(args),
    }

    handler = handlers.get(args.command)
    if handler is None:
        parser.print_help()
        return 1

    return handler()


if __name__ == "__main__":
    sys.exit(main())

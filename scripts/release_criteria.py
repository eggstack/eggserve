#!/usr/bin/env python3
"""Release criteria validator and query CLI for eggserve.

Parses and validates ``release/criteria.toml``, the single source of truth for
every release gate.  Provides machine-readable JSON output and human-readable
text, dependency-graph analysis, and Markdown checklist generation.

Requires Python 3.11+ (``tomllib``).  No external dependencies.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import deque
from dataclasses import dataclass, field
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
        if self.security_relevance:
            d["security_relevance"] = self.security_relevance
        if self.doc_ref is not None:
            d["doc_ref"] = self.doc_ref
        if self.waiver_authority is not None:
            d["waiver_authority"] = self.waiver_authority
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
        _LIST_FIELDS = {"depends_on", "features", "artifacts",
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


def cmd_generate_checklist(args: argparse.Namespace) -> int:
    """Generate a release checklist in Markdown."""
    criteria, _ = parse_criteria_file(args.criteria)
    stages: dict[str, list[Gate]] = {}
    for gate in criteria.gates:
        stages.setdefault(gate.release_stage, []).append(gate)

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
    lines.append("")

    for stage in _RELEASE_STAGES_LIST:
        gates = stages.get(stage, [])
        if not gates:
            continue
        lines.append(f"## {stage.replace('-', ' ').title()}")
        lines.append("")
        lines.append(
            "| Gate ID | Title | Required | Evidence Class "
            "| Release Stage | Status | Evidence |"
        )
        lines.append(
            "|---------|-------|----------|----------------"
            "|---------------|--------|----------|"
        )
        for gate in sorted(gates, key=lambda g: g.id):
            req = "yes" if gate.required else "no"
            evid = ", ".join(gate.evidence_classes)
            status = "PENDING"
            evidence_ref = "TBD"
            lines.append(
                f"| `{gate.id}` | {gate.title} | {req} | {evid} "
                f"| {gate.release_stage} | {status} | {evidence_ref} |"
            )
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
        help="Path to evidence bundle (reserved for future use)",
    )
    p_gen.add_argument(
        "--check",
        action="store_true",
        help="Verify committed checklist matches generated output",
    )
    p_gen.add_argument(
        "--checklist-output",
        default="docs/release-checklist-generated.md",
        help=(
            "Output path for generated checklist "
            "(default: docs/release-checklist-generated.md)"
        ),
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
    }

    handler = handlers.get(args.command)
    if handler is None:
        parser.print_help()
        return 1

    return handler()


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Release safety tests — static analysis of CI/workflow/script safety."""

from __future__ import annotations

import re
import unittest
from pathlib import Path

SCRIPTS_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPTS_DIR.parent
WORKFLOWS = REPO_ROOT / ".github" / "workflows"

try:
    import yaml
    HAS_YAML = True
except ImportError:
    HAS_YAML = False


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _load_yaml(path: Path) -> dict:
    return yaml.safe_load(_read(path))


def _yaml_on(data: dict) -> dict:
    return data.get("on") or data.get(True) or {}


def find_repo_root() -> Path | None:
    p = Path(__file__).resolve().parent.parent
    for _ in range(10):
        if (p / "Cargo.toml").is_file():
            return p
        p = p.parent
    return None


class TestValidateScriptNeverPublishes(unittest.TestCase):
    def test_no_publish_commands(self):
        script = _read(SCRIPTS_DIR / "release-validate.sh")
        forbidden = [
            (r'\bcargo\s+publish\b(?!\s+--dry-run)', "cargo publish"),
            (r'\btwine\s+upload\b', "twine upload"),
            (r'\bmaturin\s+publish\b', "maturin publish"),
            (r'\bpypi\b', "pypi reference"),
        ]
        for pattern, label in forbidden:
            matches = re.findall(pattern, script, re.IGNORECASE)
            self.assertEqual(
                matches, [],
                f"release-validate.sh contains {label}: {matches}",
            )


class TestReleaseWorkflowDefaultsToDryRun(unittest.TestCase):
    def test_dry_run_defaults_true(self):
        if not HAS_YAML:
            self.skipTest("PyYAML not installed")
        data = _load_yaml(WORKFLOWS / "release.yml")
        dispatch = _yaml_on(data).get("workflow_dispatch", {})
        inputs = dispatch.get("inputs", {})
        dry_run = inputs.get("dry_run", {})
        self.assertEqual(dry_run.get("default"), "true")


class TestReleaseWorkflowRequiresEnvironment(unittest.TestCase):
    def test_publish_job_uses_environment(self):
        if not HAS_YAML:
            self.skipTest("PyYAML not installed")
        data = _load_yaml(WORKFLOWS / "release.yml")
        publish = data.get("jobs", {}).get("publish", {})
        has_environment = "environment" in publish
        has_if_condition = "if" in publish
        self.assertTrue(
            has_environment or has_if_condition,
            "publish job must use 'environment:' or 'if:' conditions",
        )


class TestReleaseWorkflowDoesNotCreatePublicRelease(unittest.TestCase):
    def test_no_unconditional_public_release(self):
        if not HAS_YAML:
            self.skipTest("PyYAML not installed")
        data = _load_yaml(WORKFLOWS / "release.yml")
        for job_name, job in data.get("jobs", {}).items():
            for i, step in enumerate(job.get("steps", [])):
                uses = step.get("uses", "")
                if "action-gh-release" in uses:
                    with_block = step.get("with", {})
                    draft = with_block.get("draft")
                    prerelease = with_block.get("prerelease")
                    self.assertNotEqual(
                        draft, False,
                        f"Job '{job_name}' step {i}: action-gh-release with draft: false",
                    )
                    self.assertNotEqual(
                        prerelease, False,
                        f"Job '{job_name}' step {i}: action-gh-release with prerelease: false",
                    )


class TestCargoPackagesDryRunOnly(unittest.TestCase):
    def test_only_dry_run_publish(self):
        script = _read(SCRIPTS_DIR / "verify-cargo-packages.sh")
        publish_lines = [
            line.strip()
            for line in script.splitlines()
            if re.search(r'\bcargo\s+publish\b', line)
        ]
        for line in publish_lines:
            self.assertIn(
                "--dry-run",
                line,
                f"verify-cargo-packages.sh has cargo publish without --dry-run: {line}",
            )


class TestEvidenceAggregateJobRequiresAllGates(unittest.TestCase):
    def test_evidence_aggregate_needs(self):
        if not HAS_YAML:
            self.skipTest("PyYAML not installed")
        data = _load_yaml(WORKFLOWS / "ci.yml")
        aggregate = data.get("jobs", {}).get("evidence-aggregate", {})
        needs = aggregate.get("needs", [])
        self.assertIsInstance(needs, list)
        self.assertGreater(len(needs), 0, "evidence-aggregate must list needs")
        expected_jobs = [
            "rust-check",
            "supply-chain",
            "wire-tests",
            "corpus-replay",
            "python-unit-tests",
            "cargo-package",
        ]
        for job in expected_jobs:
            self.assertIn(
                job, needs,
                f"evidence-aggregate missing dependency on '{job}'",
            )


class TestEvidenceUploadUsesAlways(unittest.TestCase):
    def test_all_gate_evidence_uploads_use_always(self):
        if not HAS_YAML:
            self.skipTest("PyYAML not installed")
        data = _load_yaml(WORKFLOWS / "ci.yml")
        for job_name, job in data.get("jobs", {}).items():
            if job_name == "evidence-aggregate":
                continue
            for i, step in enumerate(job.get("steps", [])):
                uses = step.get("uses", "")
                if "upload-artifact" in uses and "gate-evidence" in step.get("with", {}).get("name", ""):
                    condition = step.get("if", "")
                    self.assertEqual(
                        condition, "always()",
                        f"Job '{job_name}' step {i}: evidence upload must use if: always(), got: {condition!r}",
                    )


class TestArtifactsBuiltBeforePublication(unittest.TestCase):
    def test_publish_depends_on_builds(self):
        if not HAS_YAML:
            self.skipTest("PyYAML not installed")
        data = _load_yaml(WORKFLOWS / "release.yml")
        publish = data.get("jobs", {}).get("publish", {})
        needs = publish.get("needs", [])
        self.assertIsInstance(needs, list)
        self.assertIn("stage-release", needs, "publish must depend on stage-release")
        stage = data.get("jobs", {}).get("stage-release", {})
        stage_needs = stage.get("needs", [])
        self.assertIn("build-artifacts", stage_needs, "stage-release must depend on build-artifacts")
        self.assertIn("build-python", stage_needs, "stage-release must depend on build-python")


if __name__ == "__main__":
    unittest.main()

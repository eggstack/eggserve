#!/usr/bin/env python3
"""Unit tests for the contract consistency validator."""

from __future__ import annotations

import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPTS_DIR.parent

_spec = importlib.util.spec_from_file_location(
    "check_contract_consistency",
    SCRIPTS_DIR / "check-contract-consistency.py",
)
cc = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(cc)


def find_repo_root() -> Path | None:
    p = Path(__file__).resolve().parent.parent
    for _ in range(10):
        if (p / "Cargo.toml").is_file():
            return p
        p = p.parent
    return None


class TestCheckTlsClaims(unittest.TestCase):
    def test_pass(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / "docs").mkdir()
            (root / "docs" / "non-goals.md").write_text("# Non-goals\n\nSome text about scope.\n")
            (root / "README.md").write_text("# eggserve\n\nTLS support via docs/tls.md\n")
            self.assertEqual(cc.check_tls_claims(root), [])

    def test_fail_deferred(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / "docs").mkdir()
            (root / "docs" / "non-goals.md").write_text(
                "# Non-goals\n\nTLS is deferred until a later release.\n"
            )
            (root / "README.md").write_text("# eggserve\n\nSome text.\n")
            errors = cc.check_tls_claims(root)
            self.assertTrue(any("tls is deferred" in e for e in errors))


class TestCheckPythonVersionConsistency(unittest.TestCase):
    def test_pass(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            crates = root / "crates" / "eggserve-python"
            crates.mkdir(parents=True)
            (crates / "pyproject.toml").write_text(
                '[project]\nrequires-python = ">=3.14,<3.15"\n'
            )
            docs = root / "docs"
            docs.mkdir()
            (docs / "library-capability-matrix.md").write_text(
                "# Matrix\n\nPython: >=3.14,<3.15\n"
            )
            self.assertEqual(cc.check_python_version_consistency(root), [])


class TestCheckPackageVersionConsistency(unittest.TestCase):
    def test_pass(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            for name in ("eggserve-core", "eggserve-bin", "eggserve-python"):
                d = root / "crates" / name
                d.mkdir(parents=True)
                (d / "Cargo.toml").write_text('[package]\nversion = "0.1.0"\n')
            (root / "crates" / "eggserve-python" / "pyproject.toml").write_text(
                'version = "0.1.0"\n'
            )
            self.assertEqual(cc.check_package_version_consistency(root), [])


class TestCheckPlatformClaims(unittest.TestCase):
    def test_pass(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            docs = root / "docs"
            docs.mkdir()
            (docs / "library-capability-matrix.md").write_text(
                "# Matrix\n\nWindows: supported-functional\n"
            )
            (docs / "release-contract.md").write_text(
                "# Release Contract\n\n## Platforms\n\n| Platform | Status |\n| --- | --- |\n| Windows | parser-level |\n"
            )
            (docs / "non-goals.md").write_text(
                "# Non-goals\n\nReparse points are not handled on Windows.\n"
            )
            self.assertEqual(cc.check_platform_claims(root), [])


class TestCheckStableApiInventory(unittest.TestCase):
    def test_pass(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            pkg = root / "crates" / "eggserve-python" / "python" / "eggserve"
            pkg.mkdir(parents=True)
            (pkg / "__init__.py").write_text(
                '__all__ = ["__version__", "ResponsePlan"]\n'
            )
            (pkg / "server.py").write_text(
                '__all__ = ["ServeConfig", "StaticPolicy"]\n'
            )
            docs = root / "docs"
            docs.mkdir()
            (docs / "api-stability.md").write_text(
                "# API Stability\n\n"
                "### `eggserve.__init__`\n\n"
                "| Item | Tier |\n| --- | --- |\n"
                "| `__version__` | stable |\n"
                "| `ResponsePlan` | stable |\n\n"
                "### `eggserve.server`\n\n"
                "| Item | Tier |\n| --- | --- |\n"
                "| `ServeConfig` | stable |\n"
                "| `StaticPolicy` | stable |\n"
            )
            self.assertEqual(cc.check_stable_api_inventory(root), [])


class TestCheckReadmeLinks(unittest.TestCase):
    def test_pass(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / "docs").mkdir()
            (root / "docs" / "non-goals.md").write_text("# Non-goals\n")
            (root / "README.md").write_text(
                "# eggserve\n\nSee [non-goals](docs/non-goals.md).\n"
            )
            self.assertEqual(cc.check_readme_links(root), [])

    def test_fail_broken(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / "README.md").write_text(
                "# eggserve\n\nSee [missing](docs/nope.md).\n"
            )
            errors = cc.check_readme_links(root)
            self.assertTrue(any("does not exist" in e for e in errors))


class TestMissingFilesHandledGracefully(unittest.TestCase):
    def test_missing_non_goals(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / "README.md").write_text("# eggserve\n\nTLS support.\n")
            errors = cc.check_tls_claims(root)
            self.assertTrue(any("not found" in e for e in errors))

    def test_missing_readme(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / "docs").mkdir()
            (root / "docs" / "non-goals.md").write_text("# Non-goals\n")
            errors = cc.check_tls_claims(root)
            self.assertTrue(any("README.md not found" in e for e in errors))

    def test_missing_pyproject(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            errors = cc.check_python_version_consistency(root)
            self.assertTrue(any("not found" in e for e in errors))

    def test_missing_api_stability(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            pkg = root / "crates" / "eggserve-python" / "python" / "eggserve"
            pkg.mkdir(parents=True)
            (pkg / "__init__.py").write_text('__all__ = ["__version__"]\n')
            errors = cc.check_stable_api_inventory(root)
            self.assertTrue(any("not found" in e for e in errors))


class TestAllChecksPassOnCurrentRepo(unittest.TestCase):
    def test_all_checks(self):
        root = find_repo_root()
        if root is None:
            self.skipTest("Not running inside the eggserve repository")
        all_errors: list[str] = []
        for name, fn in [
            ("tls", cc.check_tls_claims),
            ("python_version", cc.check_python_version_consistency),
            ("package_version", cc.check_package_version_consistency),
            ("platform", cc.check_platform_claims),
            ("stable_api", cc.check_stable_api_inventory),
            ("readme_links", cc.check_readme_links),
        ]:
            errors = fn(root)
            for e in errors:
                all_errors.append(f"[{name}] {e}")
        self.assertEqual(all_errors, [], f"Contract consistency errors:\n" + "\n".join(all_errors))


class TestCheckTriggerPolicyConsistency(unittest.TestCase):
    def _make_criteria_toml(self, gates: list[str]) -> str:
        """Build a criteria.toml string with gates having the given triggers."""
        lines = [
            "[meta]",
            'schema_version = "1.0.0"',
            'project = "eggserve"',
            'version = "0.1.0"',
            'description = "test"',
            "",
        ]
        for gate_id, triggers in gates:
            lines.append("[[gate]]")
            lines.append(f'id = "{gate_id}"')
            lines.append(f'title = "{gate_id}"')
            lines.append(f'description = "test gate"')
            lines.append("required = true")
            lines.append('evidence_classes = ["ci-log"]')
            lines.append('command = "echo ok"')
            lines.append(f'workflow_job = "{gate_id.split(".")[0]}"')
            lines.append('platforms = ["linux"]')
            lines.append("max_age_days = 1")
            lines.append("invalidated_by = []")
            lines.append("depends_on = []")
            lines.append("waiver_allowed = false")
            lines.append("release_stage = \"preflight\"")
            lines.append(f"triggers = {triggers}")
            lines.append("")
        return "\n".join(lines)

    def _make_ci_yml(self, jobs: dict[str, dict]) -> str:
        """Build a ci.yml string with the given job definitions."""
        lines = ["name: CI", "", "on:", "  push:", "    branches: [main]", "  pull_request:", "    branches: [main]", "", "jobs:"]
        for job_name, job_val in jobs.items():
            lines.append(f"  {job_name}:")
            name = job_val.get("name", job_name)
            lines.append(f'    name: "{name}"')
            if "if" in job_val:
                lines.append(f'    if: {job_val["if"]}')
            lines.append("    runs-on: ubuntu-latest")
            lines.append("    steps:")
            lines.append("      - run: echo ok")
        return "\n".join(lines)

    def test_pass_consistent_policy(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            # supply-chain gates: triggers = ["push"], CI has if: push
            criteria = self._make_criteria_toml([
                ("supply-chain.audit", '["push"]'),
                ("supply-chain.deny", '["push"]'),
            ])
            (root / "release").mkdir(parents=True)
            (root / "release" / "criteria.toml").write_text(criteria)

            ci = self._make_ci_yml({
                "supply-chain": {
                    "name": "gate/supply-chain",
                    "if": "github.event_name == 'push'",
                },
            })
            (root / ".github" / "workflows").mkdir(parents=True)
            (root / ".github" / "workflows" / "ci.yml").write_text(ci)

            errors = cc.check_trigger_policy_consistency(root)
            self.assertEqual(errors, [])

    def test_fail_mismatched_policy(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            # Gate says PR+push but CI restricts to push only
            criteria = self._make_criteria_toml([
                ("rust.format", '["pull_request", "push"]'),
            ])
            (root / "release").mkdir(parents=True)
            (root / "release" / "criteria.toml").write_text(criteria)

            ci = self._make_ci_yml({
                "rust": {
                    "name": "gate/rust",
                    "if": "github.event_name == 'push'",
                },
            })
            (root / ".github" / "workflows").mkdir(parents=True)
            (root / ".github" / "workflows" / "ci.yml").write_text(ci)

            errors = cc.check_trigger_policy_consistency(root)
            self.assertTrue(len(errors) > 0)
            self.assertIn("pull_request", errors[0])

    def test_missing_triggers_field(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            # Gate without triggers field — should pass gracefully
            lines = [
                "[meta]",
                'schema_version = "1.0.0"',
                'project = "eggserve"',
                'version = "0.1.0"',
                'description = "test"',
                "",
                "[[gate]]",
                'id = "rust.format"',
                'title = "rust.format"',
                'description = "test gate"',
                "required = true",
                'evidence_classes = ["ci-log"]',
                'command = "echo ok"',
                'workflow_job = "rust"',
                'platforms = ["linux"]',
                "max_age_days = 1",
                "invalidated_by = []",
                "depends_on = []",
                "waiver_allowed = false",
                'release_stage = "preflight"',
                "",
            ]
            (root / "release").mkdir(parents=True)
            (root / "release" / "criteria.toml").write_text("\n".join(lines))

            ci = self._make_ci_yml({
                "rust": {
                    "name": "gate/rust",
                },
            })
            (root / ".github" / "workflows").mkdir(parents=True)
            (root / ".github" / "workflows" / "ci.yml").write_text(ci)

            errors = cc.check_trigger_policy_consistency(root)
            self.assertEqual(errors, [])


if __name__ == "__main__":
    unittest.main()

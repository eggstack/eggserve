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
            ("production_profiles", cc.check_production_profiles),
            ("non_goal_retention", cc.check_non_goal_retention),
            ("no_asgi_vocabulary", cc.check_no_asgi_vocabulary_in_stable_api),
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


class TestContractConsistency(unittest.TestCase):
    """Tests that fail if Plan 055 Track B invariants are violated."""

    @staticmethod
    def _load_criteria():
        import tomllib
        path = REPO_ROOT / "release" / "criteria.toml"
        with open(path, "rb") as f:
            return tomllib.load(f)

    @staticmethod
    def _load_ci_jobs():
        ci_text = (REPO_ROOT / ".github" / "workflows" / "ci.yml").read_text()
        try:
            import yaml
            data = yaml.safe_load(ci_text)
        except ImportError:
            data = cc._yaml_load_string(ci_text)
        jobs = {}
        for job_key, job_val in data.get("jobs", {}).items():
            display = job_val.get("name", job_key)
            jobs[job_key] = {"name": display, "if": job_val.get("if", "")}
        return jobs

    @staticmethod
    def _load_platforms(criteria):
        return criteria.get("platforms", {})

    def test_no_duplicate_evidence_filenames(self):
        criteria = self._load_criteria()
        seen: dict[str, str] = {}
        for gate in criteria.get("gate", []):
            gate_id = gate["id"]
            evidence_filename = gate_id.replace(".", "-") + ".json"
            self.assertIn(
                evidence_filename, seen,
                f"Evidence filename collision: gates '{seen[evidence_filename]}' and "
                f"'{gate_id}' both produce '{evidence_filename}'",
            ) if False else None
            if evidence_filename in seen:
                self.fail(
                    f"Evidence filename collision: gates '{seen[evidence_filename]}' and "
                    f"'{gate_id}' both produce '{evidence_filename}'"
                )
            seen[evidence_filename] = gate_id

    def test_platform_gates_mapped_to_correct_runner(self):
        criteria = self._load_criteria()
        ci_jobs = self._load_ci_jobs()
        platforms = self._load_platforms(criteria)

        runner_map = {p: p_val.get("runner", "") for p, p_val in platforms.items()}

        ci_full = self._load_ci_full()

        matrix_jobs: dict[str, list[str]] = {}
        static_jobs: dict[str, str] = {}
        for job_key, job_val in ci_full.get("jobs", {}).items():
            runs_on = job_val.get("runs-on", "")
            if isinstance(runs_on, str) and "${{" in runs_on:
                matrix = job_val.get("strategy", {}).get("matrix", {})
                os_list = matrix.get("os", [])
                matrix_jobs[job_key] = os_list
            elif isinstance(runs_on, str):
                static_jobs[job_key] = runs_on

        for gate in criteria.get("gate", []):
            gate_id = gate["id"]
            gate_platforms = gate.get("platforms", [])
            workflow_job = gate.get("workflow_job", "")
            if not gate_platforms or not workflow_job:
                continue

            release_only = {"validate", "stage-release", "publish", "build-artifacts", "build-python"}
            if workflow_job in release_only:
                continue

            if workflow_job in matrix_jobs:
                matrix_runners = matrix_jobs[workflow_job]
                for platform in gate_platforms:
                    expected_runner = runner_map.get(platform, "")
                    if expected_runner and expected_runner not in matrix_runners:
                        self.fail(
                            f"Gate '{gate_id}' has platform '{platform}' (runner={expected_runner}) "
                            f"but CI job '{workflow_job}' matrix only includes {matrix_runners}"
                        )
            elif workflow_job in static_jobs:
                actual_runner = static_jobs[workflow_job]
                for platform in gate_platforms:
                    expected_runner = runner_map.get(platform, "")
                    if expected_runner and expected_runner != actual_runner:
                        self.fail(
                            f"Gate '{gate_id}' has platform '{platform}' (expected runner={expected_runner}) "
                            f"but CI job '{workflow_job}' runs on {actual_runner}"
                        )
            else:
                self.fail(
                    f"Gate '{gate_id}' references workflow_job '{workflow_job}' "
                    f"which is not found in CI job definitions"
                )

    @staticmethod
    def _load_ci_full():
        ci_text = (REPO_ROOT / ".github" / "workflows" / "ci.yml").read_text()
        try:
            import yaml
            return yaml.safe_load(ci_text)
        except ImportError:
            return cc._yaml_load_string(ci_text)

    def test_required_gates_have_commands(self):
        criteria = self._load_criteria()
        for gate in criteria.get("gate", []):
            if gate.get("required", False):
                command = gate.get("command", "")
                self.assertTrue(
                    bool(command and command.strip()),
                    f"Required gate '{gate['id']}' has empty or missing command",
                )

    def test_gate_ids_follow_naming_convention(self):
        import re
        criteria = self._load_criteria()
        segment = r"[a-z][a-z0-9]*(-[a-z0-9]+)*"
        pattern = re.compile(
            rf"^{segment}(\.{segment}){{1,2}}$"
        )
        allowed_exceptions = {"check-generated"}
        for gate in criteria.get("gate", []):
            gate_id = gate["id"]
            if gate_id in allowed_exceptions:
                continue
            self.assertRegex(
                gate_id, pattern,
                f"Gate ID '{gate_id}' does not match convention: "
                f"must be category.specific or category.sub.specific "
                f"(1-2 dots, lowercase alphanumeric and hyphens)",
            )

    def test_no_gate_references_nonexistent_workflow(self):
        criteria = self._load_criteria()
        ci_jobs = self._load_ci_jobs()
        release_only = {"validate", "stage-release", "publish", "build-artifacts", "build-python"}

        all_ci_names: set[str] = set()
        for job_key, job_val in ci_jobs.items():
            all_ci_names.add(job_key)
            all_ci_names.add(job_val["name"])

        for gate in criteria.get("gate", []):
            workflow_job = gate.get("workflow_job", "")
            if not workflow_job:
                continue
            if workflow_job in release_only:
                continue
            self.assertIn(
                workflow_job, all_ci_names,
                f"Gate '{gate['id']}' references workflow_job '{workflow_job}' "
                f"which does not exist in .github/workflows/ci.yml. "
                f"Available jobs: {sorted(all_ci_names)}",
            )


class TestCheckProductionProfiles(unittest.TestCase):
    def test_pass_valid_profiles(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / "release").mkdir()
            (root / "release" / "criteria.toml").write_text(
                '[meta]\nschema_version = "1.0"\n\n'
                '[[gate]]\nid = "rust.test"\ntitle = "test"\n'
                'description = "test"\nrequired = true\n'
                'evidence_classes = ["ci-log"]\ncommand = "echo ok"\n'
                'workflow_job = "rust"\nplatforms = ["linux"]\n'
                'max_age_days = 1\ninvalidated_by = []\ndepends_on = []\n'
                'waiver_allowed = false\nrelease_stage = "preflight"\n'
            )
            (root / "release" / "support-profiles.toml").write_text(
                '[[profile]]\nprofile = "test-profile"\n'
                'status = "candidate"\nplatform = ["linux-x86_64"]\n'
                'filesystem = ["ext4"]\nnetwork_binding = "loopback"\n'
                'tls_termination = "none"\nhttp_version = "1.1"\n'
                'security_defaults = ["loopback-bind"]\n'
                'symlink_following_allowed = false\n'
                'directory_listing_hardened = false\n'
                'python_callbacks_in_profile = true\n'
                'required_gates = ["rust.test"]\n'
                'excluded_flags = []\nevidence_max_age = "30d"\n'
                'invalidated_by = []\nwaivers_allowed = true\n'
                'notes = "test"\n'
            )
            (root / "README.md").write_text(
                "# eggserve\n\nProduction profiles in release/support-profiles.toml\n"
            )
            (root / "docs").mkdir()
            (root / "docs" / "threat-model.md").write_text(
                "# Threat Model\n\nProduction profiles defined.\n"
            )
            errors = cc.check_production_profiles(root)
            self.assertEqual(errors, [])

    def test_fail_hardened_allows_symlinks(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / "release").mkdir()
            (root / "release" / "criteria.toml").write_text(
                '[meta]\nschema_version = "1.0"\n'
            )
            (root / "release" / "support-profiles.toml").write_text(
                '[[profile]]\nprofile = "bad"\n'
                'status = "supported-hardened"\nplatform = ["linux-x86_64"]\n'
                'filesystem = ["ext4"]\nnetwork_binding = "loopback"\n'
                'tls_termination = "none"\nhttp_version = "1.1"\n'
                'security_defaults = []\n'
                'symlink_following_allowed = true\n'
                'directory_listing_hardened = false\n'
                'python_callbacks_in_profile = false\n'
                'required_gates = []\nexcluded_flags = []\n'
                'evidence_max_age = "30d"\ninvalidated_by = []\n'
                'waivers_allowed = false\nnotes = "test"\n'
            )
            errors = cc.check_production_profiles(root)
            self.assertTrue(any("symlink" in e for e in errors))

    def test_fail_nonexistent_gate_reference(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / "release").mkdir()
            (root / "release" / "criteria.toml").write_text(
                '[meta]\nschema_version = "1.0"\n\n'
                '[[gate]]\nid = "rust.test"\ntitle = "test"\n'
                'description = "test"\nrequired = true\n'
                'evidence_classes = ["ci-log"]\ncommand = "echo ok"\n'
                'workflow_job = "rust"\nplatforms = ["linux"]\n'
                'max_age_days = 1\ninvalidated_by = []\ndepends_on = []\n'
                'waiver_allowed = false\nrelease_stage = "preflight"\n'
            )
            (root / "release" / "support-profiles.toml").write_text(
                '[[profile]]\nprofile = "test"\n'
                'status = "candidate"\nplatform = ["linux-x86_64"]\n'
                'filesystem = ["ext4"]\nnetwork_binding = "loopback"\n'
                'tls_termination = "none"\nhttp_version = "1.1"\n'
                'security_defaults = []\n'
                'symlink_following_allowed = false\n'
                'directory_listing_hardened = false\n'
                'python_callbacks_in_profile = false\n'
                'required_gates = ["nonexistent.gate"]\n'
                'excluded_flags = []\nevidence_max_age = "30d"\n'
                'invalidated_by = []\nwaivers_allowed = false\n'
                'notes = "test"\n'
            )
            errors = cc.check_production_profiles(root)
            self.assertTrue(any("nonexistent" in e for e in errors))


class TestCheckNonGoalRetention(unittest.TestCase):
    def test_pass_all_present(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / "docs").mkdir()
            (root / "docs" / "non-goals.md").write_text(
                "# Non-goals\n\n"
                "- No in-tree ASGI or WSGI adapter\n"
                "- No reverse proxying\n"
                "- No middleware stack\n"
                "- No framework routing\n"
                "- No automatic ACME\n"
                "- No HTTP/2\n"
                "- No upload/write support\n"
                "- Downstream projects may build these\n"
            )
            errors = cc.check_non_goal_retention(root)
            self.assertEqual(errors, [])

    def test_fail_missing_asgi(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / "docs").mkdir()
            (root / "docs" / "non-goals.md").write_text(
                "# Non-goals\n\nSome text.\n"
            )
            errors = cc.check_non_goal_retention(root)
            self.assertTrue(any("asgi" in e for e in errors))


class TestCheckNoAsgiVocabularyInStableApi(unittest.TestCase):
    def test_pass_clean(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / "docs").mkdir()
            (root / "docs" / "api-stability.md").write_text(
                "# API Stability\n\n"
                "ASGI/WSGI adapters are not included in stable API.\n"
                "Downstream projects may build them.\n"
            )
            errors = cc.check_no_asgi_vocabulary_in_stable_api(root)
            self.assertEqual(errors, [])

    def test_fail_asgi_in_stable(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / "docs").mkdir()
            (root / "docs" / "api-stability.md").write_text(
                "# API Stability\n\n"
                "### `eggserve.server`\n\n"
                "| Item | Tier |\n| --- | --- |\n"
                "| `AsgiHandler` | stable |\n"
            )
            errors = cc.check_no_asgi_vocabulary_in_stable_api(root)
            self.assertTrue(any("asgi" in e for e in errors))


if __name__ == "__main__":
    unittest.main()

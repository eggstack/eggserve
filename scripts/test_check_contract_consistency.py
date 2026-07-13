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


if __name__ == "__main__":
    unittest.main()

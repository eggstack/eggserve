"""Standalone packaging smoke test — CLI entry points.

Tests `python -m eggserve --help` and the installed `eggserve` console
script. The wheel links the native CLI implementation directly into the
PyO3 extension, so no separate bundled binary is required.

Must be run from an installed wheel (pip install eggserve), NOT from the
source tree. Uses only stdlib + eggserve.
"""

import shutil
import subprocess
import sys
import unittest


class TestCliHelp(unittest.TestCase):
    """python -m eggserve --help must exit 0 and print usage info."""

    def _run_help(self):
        return subprocess.run(
            [sys.executable, "-m", "eggserve", "--help"],
            capture_output=True,
            text=True,
            timeout=10,
        )

    def test_module_help_exits_zero(self):
        result = self._run_help()
        self.assertEqual(result.returncode, 0, f"stderr: {result.stderr}")

    def test_module_help_outputs_usage(self):
        result = self._run_help()
        output = result.stdout + result.stderr
        output_lower = output.lower()
        self.assertTrue(
            "usage" in output_lower or "eggserve" in output_lower or "--bind" in output_lower,
            f"Expected usage/help output, got: {output[:200]}",
        )

    def test_module_help_mentions_directory(self):
        result = self._run_help()
        output = result.stdout + result.stderr
        self.assertIn("--directory", output)

    def test_module_help_mentions_bind(self):
        result = self._run_help()
        output = result.stdout + result.stderr
        self.assertIn("--bind", output)

    def test_module_help_mentions_port(self):
        result = self._run_help()
        output = result.stdout + result.stderr
        self.assertIn("--port", output)


class TestConsoleScript(unittest.TestCase):
    """The installed ``eggserve`` console script must be discoverable and run."""

    def test_console_script_is_discoverable(self):
        cmd = shutil.which("eggserve")
        self.assertIsNotNone(cmd, "installed `eggserve` console script not on PATH")

    def test_console_script_runs(self):
        cmd = shutil.which("eggserve")
        if cmd is None:
            self.skipTest("installed `eggserve` console script not on PATH")
        result = subprocess.run(
            [cmd, "--help"],
            capture_output=True,
            text=True,
            timeout=10,
        )
        self.assertEqual(result.returncode, 0, f"stderr: {result.stderr}")
        output = result.stdout + result.stderr
        self.assertIn("eggserve", output.lower())

    def test_console_script_mentions_directory(self):
        cmd = shutil.which("eggserve")
        if cmd is None:
            self.skipTest("installed `eggserve` console script not on PATH")
        result = subprocess.run(
            [cmd, "--help"],
            capture_output=True,
            text=True,
            timeout=10,
        )
        self.assertIn("--directory", result.stdout + result.stderr)


class TestVersionConsistency(unittest.TestCase):
    """Version from Python module must be consistent."""

    def test_version_matches_package_metadata(self):
        import importlib.metadata
        import eggserve

        try:
            pkg_version = importlib.metadata.version("eggserve")
            self.assertEqual(
                eggserve.__version__,
                pkg_version,
                f"__version__ ({eggserve.__version__}) does not match "
                f"package metadata ({pkg_version})",
            )
        except importlib.metadata.PackageNotFoundError:
            pass


if __name__ == "__main__":
    unittest.main()

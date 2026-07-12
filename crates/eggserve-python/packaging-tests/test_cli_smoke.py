"""Standalone packaging smoke test — CLI and binary discovery.

Tests `python -m eggserve --help` and verifies the binary is discoverable.

Must be run from an installed wheel (pip install eggserve), NOT from the
source tree. Uses only stdlib + eggserve.
"""

import subprocess
import sys
import unittest


class TestCliHelp(unittest.TestCase):
    """python -m eggserve --help must exit 0 and print usage info.

    NOTE: This test requires the eggserve binary to be installed.
    If the binary is not found, the test is skipped.
    """

    def _run_help(self):
        return subprocess.run(
            [sys.executable, "-m", "eggserve", "--help"],
            capture_output=True,
            text=True,
            timeout=10,
        )

    def test_module_help_exits_zero_or_skipped(self):
        result = self._run_help()
        if result.returncode != 0 and "binary not found" in result.stderr.lower():
            self.skipTest("eggserve binary not found")
        self.assertEqual(result.returncode, 0, f"stderr: {result.stderr}")

    def test_module_help_outputs_usage(self):
        result = self._run_help()
        if result.returncode != 0 and "binary not found" in result.stderr.lower():
            self.skipTest("eggserve binary not found")
        output = result.stdout + result.stderr
        output_lower = output.lower()
        self.assertTrue(
            "usage" in output_lower or "eggserve" in output_lower or "--bind" in output_lower,
            f"Expected usage/help output, got: {output[:200]}",
        )

    def test_module_help_mentions_directory(self):
        result = self._run_help()
        if result.returncode != 0 and "binary not found" in result.stderr.lower():
            self.skipTest("eggserve binary not found")
        output = result.stdout + result.stderr
        self.assertIn("--directory", output)

    def test_module_help_mentions_bind(self):
        result = self._run_help()
        if result.returncode != 0 and "binary not found" in result.stderr.lower():
            self.skipTest("eggserve binary not found")
        output = result.stdout + result.stderr
        self.assertIn("--bind", output)

    def test_module_help_mentions_port(self):
        result = self._run_help()
        if result.returncode != 0 and "binary not found" in result.stderr.lower():
            self.skipTest("eggserve binary not found")
        output = result.stdout + result.stderr
        self.assertIn("--port", output)


class TestBinaryDiscovery(unittest.TestCase):
    """The eggserve binary must be discoverable by the _bin module.

    NOTE: The Python wheel does not bundle the binary. Binary discovery
    depends on the binary being installed separately (e.g., via cargo install
    or PATH). If the binary is not found, these tests are skipped.
    """

    def setUp(self):
        try:
            from eggserve._bin import _find_binary
            self._binary_path = _find_binary()
        except FileNotFoundError:
            self._binary_path = None

    def test_find_binary_succeeds_or_skipped(self):
        """Binary discovery succeeds if binary is installed, else skip."""
        if self._binary_path is None:
            self.skipTest("eggserve binary not found on PATH")
        self.assertIsInstance(self._binary_path, str)
        self.assertGreater(len(self._binary_path), 0)

    def test_binary_is_executable_or_skipped(self):
        """Binary must be executable if found."""
        if self._binary_path is None:
            self.skipTest("eggserve binary not found on PATH")
        import os
        self.assertTrue(os.path.isfile(self._binary_path))
        self.assertTrue(os.access(self._binary_path, os.X_OK))

    def test_binary_executes_with_help_or_skipped(self):
        """Binary must respond to --help if found."""
        if self._binary_path is None:
            self.skipTest("eggserve binary not found on PATH")
        result = subprocess.run(
            [self._binary_path, "--help"],
            capture_output=True,
            text=True,
            timeout=10,
        )
        self.assertEqual(result.returncode, 0, f"stderr: {result.stderr}")
        output = result.stdout + result.stderr
        self.assertTrue(
            "usage" in output.lower() or "eggserve" in output.lower(),
            f"Expected help output from binary, got: {output[:200]}",
        )


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

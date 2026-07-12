"""Standalone packaging smoke test — import and metadata validation.

Verifies that all public names from eggserve.__all__ are importable, version
metadata is valid, the native extension loads correctly, and no source-tree
modules shadow the installed package.

Must be run from an installed wheel (pip install eggserve), NOT from the
source tree. Uses only stdlib + eggserve.
"""

import importlib
import os
import sys
import unittest


class TestAllNamesImportable(unittest.TestCase):
    """Every name listed in eggserve.__all__ must be importable."""

    def test_all_names_are_importable(self):
        import eggserve

        missing = []
        for name in eggserve.__all__:
            if not hasattr(eggserve, name):
                missing.append(name)
        self.assertEqual(
            missing, [], f"These __all__ names are missing from the eggserve module: {missing}"
        )

    def test_all_names_are_not_none(self):
        import eggserve

        none_names = [n for n in eggserve.__all__ if getattr(eggserve, n) is None]
        self.assertEqual(
            none_names, [], f"These __all__ names resolved to None: {none_names}"
        )


class TestVersionMetadata(unittest.TestCase):
    """Version string must be valid semver-like and importable."""

    def test_version_exists(self):
        import eggserve
        self.assertTrue(hasattr(eggserve, "__version__"))

    def test_version_is_string(self):
        import eggserve
        self.assertIsInstance(eggserve.__version__, str)

    def test_version_is_nonempty(self):
        import eggserve
        self.assertGreater(len(eggserve.__version__), 0)

    def test_version_has_at_least_two_components(self):
        import eggserve
        parts = eggserve.__version__.split(".")
        self.assertGreaterEqual(len(parts), 2)

    def test_version_components_are_numeric(self):
        import eggserve
        parts = eggserve.__version__.split(".")
        for part in parts:
            self.assertTrue(
                part.isdigit(), f"Version component {part!r} is not numeric"
            )


class TestNativeExtension(unittest.TestCase):
    """The native Rust extension must be available in a wheel install."""

    def test_native_available_flag(self):
        import eggserve
        self.assertTrue(
            eggserve.NATIVE_AVAILABLE,
            "NATIVE_AVAILABLE is False — native extension failed to load",
        )

    def test_native_module_importable(self):
        mod = importlib.import_module("eggserve._native")
        self.assertIsNotNone(mod)

    def test_key_native_types_exist(self):
        import eggserve
        key_names = [
            "Server",
            "Response",
            "Request",
            "StaticResponder",
            "ServerSecureRoot",
            "HttpClient",
            "ClientConfig",
            "SecureRoot",
            "StaticPolicy",
            "PathPolicy",
            "EggserveError",
            "LifecycleError",
        ]
        for name in key_names:
            self.assertTrue(
                hasattr(eggserve, name),
                f"Expected native type {name} not found on eggserve module",
            )


class TestNoSourceTreeShadowing(unittest.TestCase):
    """Installed package must not be shadowed by a source-tree copy."""

    def test_eggserve_package_is_not_in_current_dir(self):
        eggserve_mod = importlib.import_module("eggserve")
        pkg_file = getattr(eggserve_mod, "__file__", None)
        self.assertIsNotNone(pkg_file, "eggserve.__file__ is None")
        pkg_dir = os.path.dirname(os.path.realpath(pkg_file))
        cwd = os.path.realpath(os.getcwd())
        self.assertNotEqual(
            pkg_dir,
            cwd,
            f"eggserve package resolves to CWD ({pkg_dir}), which suggests "
            "source-tree shadowing. Run from a different directory.",
        )

    def test_eggserve_not_on_cwd_sys_path(self):
        cwd = os.path.realpath(os.getcwd())
        for entry in sys.path:
            if os.path.realpath(entry) == cwd:
                # It's acceptable for '' (empty string) to be on sys.path,
                # but a crates/eggserve-python/python entry means source-tree
                # shadowing. We check for the python/ subdirectory specifically.
                pass
        crates_python = os.path.join(cwd, "crates", "eggserve-python", "python")
        for entry in sys.path:
            if os.path.realpath(entry) == os.path.realpath(crates_python):
                self.fail(
                    f"Source-tree path {crates_python} is on sys.path, "
                    "which may cause shadowing. Remove it before running."
                )

    def test_native_module_file_not_in_source_tree(self):
        mod = importlib.import_module("eggserve._native")
        native_file = getattr(mod, "__file__", None)
        self.assertIsNotNone(native_file)
        native_real = os.path.realpath(native_file)
        crates_dir = os.path.join(
            os.getcwd(), "crates", "eggserve-python", "python", "eggserve"
        )
        self.assertFalse(
            native_real.startswith(os.path.realpath(crates_dir)),
            f"Native extension ({native_real}) appears to be from the source tree "
            f"({crates_dir}). Run from a different directory.",
        )


if __name__ == "__main__":
    unittest.main()

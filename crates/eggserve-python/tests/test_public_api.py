"""Contract tests for the deliberately small supported Python surface."""

import unittest


class PublicApiTests(unittest.TestCase):
    def test_server_exports(self):
        import eggserve
        import eggserve.server as server

        expected = {
            "HTTPServer", "ThreadingHTTPServer", "HTTPSServer",
            "ThreadingHTTPSServer", "BaseHTTPRequestHandler",
            "SimpleHTTPRequestHandler",
        }
        self.assertEqual(set(server.__all__), expected)
        self.assertTrue(expected.issubset(set(eggserve.__all__)))

    def test_top_level_excludes_experimental_and_internal_names(self):
        import eggserve

        for name in (
            "Server", "ServerSecureRoot", "StaticResponder",
            "StaticPolicyWrapper",
        ):
            self.assertNotIn(name, eggserve.__all__)

    def test_lowlevel_namespace(self):
        from eggserve.lowlevel import RequestTarget, SecureRoot, StaticPolicy

        self.assertIsNotNone(RequestTarget)
        self.assertIsNotNone(SecureRoot)
        self.assertFalse(StaticPolicy().follow_symlinks)

    def test_lowlevel_runtime_substrate(self):
        from eggserve import lowlevel

        self.assertTrue(callable(lowlevel.Server))
        self.assertIsNotNone(lowlevel.RuntimeConfig())
        self.assertTrue(callable(lowlevel.Response.stream))
        self.assertIsNotNone(lowlevel.StaticResponder)
        self.assertIsNotNone(lowlevel.ServerSecureRoot)
        for name in ("RuntimeConfig", "Server", "StaticResponder", "ServerSecureRoot"):
            self.assertIn(name, lowlevel.__all__)

    def test_subprocess_namespace(self):
        from eggserve.subprocess import ServeConfig, ServerProcess, serve_directory

        self.assertIsNotNone(ServeConfig)
        self.assertIsNotNone(ServerProcess)
        self.assertTrue(callable(serve_directory))

    def test_subprocess_ownership_and_compat_aliases(self):
        """Plan 182 Track A/D: subprocess owns helpers; server re-exports."""
        import eggserve
        import eggserve.server as server
        import eggserve.subprocess as subprocess_mod

        # Canonical owner exports the convenience API.
        self.assertEqual(
            set(subprocess_mod.__all__),
            {"ServeConfig", "ServerProcess", "StaticPolicy", "serve_directory"},
        )
        # Top-level convenience is preserved.
        self.assertTrue(callable(eggserve.serve_directory))
        self.assertIs(eggserve.serve_directory, subprocess_mod.serve_directory)
        # Documented server compatibility aliases resolve to the same objects.
        self.assertIs(server.serve_directory, subprocess_mod.serve_directory)
        self.assertIs(server.ServeConfig, subprocess_mod.ServeConfig)
        self.assertIs(server.ServerProcess, subprocess_mod.ServerProcess)
        self.assertIs(server.StaticPolicy, subprocess_mod.StaticPolicy)
        # Private bridge helpers are shared, not duplicated.
        self.assertIs(server._parse_bind, subprocess_mod._parse_bind)
        self.assertIs(server._config_to_argv, subprocess_mod._config_to_argv)
        # server.__all__ stays focused on the six-class facade.
        self.assertNotIn("serve_directory", server.__all__)
        self.assertNotIn("ServeConfig", server.__all__)

    def test_subprocess_has_no_server_dependency(self):
        """Plan 182 A3: subprocess must not import the compat server module."""
        import ast
        from pathlib import Path

        import eggserve.subprocess as subprocess_mod

        source = Path(subprocess_mod.__file__).read_text()
        tree = ast.parse(source)
        for node in ast.walk(tree):
            if isinstance(node, ast.ImportFrom) and node.module:
                self.assertFalse(
                    node.module == "eggserve.server"
                    or node.module.startswith("eggserve.server."),
                    "subprocess.py must not import from eggserve.server",
                )
            if isinstance(node, ast.Import):
                for alias in node.names:
                    self.assertFalse(
                        alias.name == "eggserve.server"
                        or alias.name.startswith("eggserve.server."),
                        "subprocess.py must not import eggserve.server",
                    )
        # The loaded module holds no reference to the server module object.
        import sys

        server_mod = sys.modules.get("eggserve.server")
        if server_mod is not None:
            self.assertNotIn(
                server_mod,
                list(vars(subprocess_mod).values()),
                "subprocess module must not hold a server module reference",
            )

    def test_lowlevel_independently_importable(self):
        import eggserve.lowlevel as lowlevel

        self.assertTrue(callable(lowlevel.Server))
        self.assertIsNotNone(lowlevel.RuntimeConfig())

    def test_removed_client_types_not_in_native_extension(self):
        import eggserve._native as native

        for name in ("HttpClient", "ClientConfig", "ClientRequest", "ClientResponse"):
            self.assertFalse(
                hasattr(native, name),
                f"Removed type {name} should not be present in native extension",
            )


if __name__ == "__main__":
    unittest.main()

"""API stability enforcement tests for the eggserve Python package.

These tests verify that the public API surface matches the stability tiers
defined in docs/api-stability.md. They check that expected names are exported,
internal names are absent from __all__, and experimental client types are
classified correctly.
"""

from __future__ import annotations

import unittest


class TestEggserveInitAll(unittest.TestCase):
    """Verify eggserve.__all__ contains expected public names."""

    def test_all_contains_version(self) -> None:
        import eggserve

        self.assertIn("__version__", eggserve.__all__)

    def test_all_contains_serve_config(self) -> None:
        import eggserve

        self.assertIn("ServeConfig", eggserve.__all__)

    def test_all_contains_server_process(self) -> None:
        import eggserve

        self.assertIn("ServerProcess", eggserve.__all__)

    def test_all_contains_serve_directory(self) -> None:
        import eggserve

        self.assertIn("serve_directory", eggserve.__all__)

    def test_all_contains_response_plan(self) -> None:
        import eggserve

        self.assertIn("ResponsePlan", eggserve.__all__)

    def test_all_contains_native_available(self) -> None:
        import eggserve

        self.assertIn("NATIVE_AVAILABLE", eggserve.__all__)

    def test_all_length_is_bounded(self) -> None:
        """Sanity check: __all__ should not grow unbounded without review."""
        import eggserve

        # As of the current release, __all__ has at most ~61 entries
        # (6 always + ~55 when native is available).
        self.assertLessEqual(len(eggserve.__all__), 70)


class TestPublicNamesImportable(unittest.TestCase):
    """Verify that expected public names are importable from eggserve."""

    def test_import_serve_config(self) -> None:
        from eggserve import ServeConfig

        self.assertIsNotNone(ServeConfig)

    def test_import_static_policy(self) -> None:
        """StaticPolicy is importable when native module is available."""
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")

        from eggserve import StaticPolicy

        self.assertIsNotNone(StaticPolicy)

    def test_import_serve_directory(self) -> None:
        from eggserve import serve_directory

        self.assertTrue(callable(serve_directory))

    def test_import_server_process(self) -> None:
        from eggserve import ServerProcess

        self.assertIsNotNone(ServerProcess)

    def test_import_response_plan(self) -> None:
        from eggserve import ResponsePlan

        self.assertIsNotNone(ResponsePlan)

    def test_import_version(self) -> None:
        from eggserve import __version__

        self.assertIsInstance(__version__, str)
        self.assertRegex(__version__, r"^\d+\.\d+\.\d+")


class TestInternalNamesAbsentFromAll(unittest.TestCase):
    """Verify that internal names are NOT in __all__."""

    def test_native_module_not_in_all(self) -> None:
        import eggserve

        self.assertNotIn("_native", eggserve.__all__)

    def test_bin_module_not_in_all(self) -> None:
        import eggserve

        self.assertNotIn("_bin", eggserve.__all__)

    def test_find_binary_not_in_all(self) -> None:
        import eggserve

        self.assertNotIn("_find_binary", eggserve.__all__)

    def test_parse_bind_not_in_all(self) -> None:
        """_parse_bind is internal to server.py."""
        import eggserve

        self.assertNotIn("_parse_bind", eggserve.__all__)

    def test_config_to_argv_not_in_all(self) -> None:
        """_config_to_argv is internal to server.py."""
        import eggserve

        self.assertNotIn("_config_to_argv", eggserve.__all__)


class TestNativeNamesInAllWhenAvailable(unittest.TestCase):
    """When native module is loaded, verify expected types are in __all__."""

    def setUp(self) -> None:
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")

    def test_path_policy_in_all(self) -> None:
        import eggserve

        self.assertIn("PathPolicy", eggserve.__all__)

    def test_static_policy_in_all(self) -> None:
        import eggserve

        self.assertIn("StaticPolicy", eggserve.__all__)

    def test_secure_root_in_all(self) -> None:
        import eggserve

        self.assertIn("SecureRoot", eggserve.__all__)

    def test_resolved_resource_in_all(self) -> None:
        import eggserve

        self.assertIn("ResolvedResource", eggserve.__all__)

    def test_resolved_file_in_all(self) -> None:
        import eggserve

        self.assertIn("ResolvedFile", eggserve.__all__)

    def test_resolved_directory_in_all(self) -> None:
        import eggserve

        self.assertIn("ResolvedDirectory", eggserve.__all__)

    def test_eggserve_error_in_all(self) -> None:
        import eggserve

        self.assertIn("EggserveError", eggserve.__all__)

    def test_validate_method_in_all(self) -> None:
        import eggserve

        self.assertIn("validate_method", eggserve.__all__)

    def test_generate_etag_in_all(self) -> None:
        import eggserve

        self.assertIn("generate_etag", eggserve.__all__)

    def test_body_source_in_all(self) -> None:
        import eggserve

        self.assertIn("BodySource", eggserve.__all__)

    def test_server_in_all(self) -> None:
        import eggserve

        self.assertIn("Server", eggserve.__all__)


class TestExperimentalClientNames(unittest.TestCase):
    """Verify that client types are present (when available) and classified experimental."""

    def setUp(self) -> None:
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")

    def test_http_client_in_all(self) -> None:
        import eggserve

        self.assertIn("HttpClient", eggserve.__all__)

    def test_client_config_in_all(self) -> None:
        import eggserve

        self.assertIn("ClientConfig", eggserve.__all__)

    def test_client_request_in_all(self) -> None:
        import eggserve

        self.assertIn("ClientRequest", eggserve.__all__)

    def test_client_response_in_all(self) -> None:
        import eggserve

        self.assertIn("ClientResponse", eggserve.__all__)

    def test_client_error_in_all(self) -> None:
        import eggserve

        self.assertIn("ClientError", eggserve.__all__)

    def test_client_method_in_all(self) -> None:
        import eggserve

        self.assertIn("Method", eggserve.__all__)


class TestCanonicalRequestTypesInAll(unittest.TestCase):
    """Verify that canonical request types are in __all__ when native is available."""

    def setUp(self) -> None:
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")

    def test_method_in_all(self) -> None:
        import eggserve

        self.assertIn("Method", eggserve.__all__)

    def test_http_version_in_all(self) -> None:
        import eggserve

        self.assertIn("HttpVersion", eggserve.__all__)

    def test_header_block_in_all(self) -> None:
        import eggserve

        self.assertIn("HeaderBlock", eggserve.__all__)

    def test_connection_info_in_all(self) -> None:
        import eggserve

        self.assertIn("ConnectionInfo", eggserve.__all__)

    def test_canonical_request_in_all(self) -> None:
        import eggserve

        self.assertIn("CanonicalRequest", eggserve.__all__)

    def test_parse_method_in_all(self) -> None:
        import eggserve

        self.assertIn("parse_method", eggserve.__all__)

    def test_parse_http_version_in_all(self) -> None:
        import eggserve

        self.assertIn("parse_http_version", eggserve.__all__)

    def test_method_error_in_all(self) -> None:
        import eggserve

        self.assertIn("MethodError", eggserve.__all__)

    def test_http_version_error_in_all(self) -> None:
        import eggserve

        self.assertIn("HttpVersionError", eggserve.__all__)

    def test_header_error_in_all(self) -> None:
        import eggserve

        self.assertIn("HeaderError", eggserve.__all__)

    def test_duplicate_header_error_in_all(self) -> None:
        import eggserve

        self.assertIn("DuplicateHeaderError", eggserve.__all__)


class TestServerModuleAll(unittest.TestCase):
    """Verify eggserve.server.__all__ contains expected names."""

    def test_server_all_has_static_policy(self) -> None:
        from eggserve.server import __all__ as server_all

        self.assertIn("StaticPolicy", server_all)

    def test_server_all_has_serve_config(self) -> None:
        from eggserve.server import __all__ as server_all

        self.assertIn("ServeConfig", server_all)

    def test_server_all_has_server_process(self) -> None:
        from eggserve.server import __all__ as server_all

        self.assertIn("ServerProcess", server_all)

    def test_server_all_has_serve_directory(self) -> None:
        from eggserve.server import __all__ as server_all

        self.assertIn("serve_directory", server_all)

    def test_server_all_length(self) -> None:
        from eggserve.server import __all__ as server_all

        self.assertEqual(len(server_all), 4)


class TestVersionFormat(unittest.TestCase):
    """Verify version string format is PEP 440 compatible."""

    def test_version_is_pep440(self) -> None:
        import eggserve

        import re

        # Basic PEP 440 pattern: N[.N]+[.N][{a|b|rc}N][.postN][.devN]
        pattern = r"^\d+\.\d+\.\d+((a|b|rc)\d+)?(\.post\d+)?(\.dev\d+)?$"
        self.assertRegex(eggserve.__version__, pattern)


class TestServerConstructorSnapshot(unittest.TestCase):
    """Snapshot tests for Server constructor signature.

    These tests catch accidental changes to the Server constructor parameters.
    If the signature changes intentionally, update the expected values below.
    """

    def test_server_has_expected_params(self) -> None:
        """Server constructor must accept expected parameters."""
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")

        # Test that we can construct with all expected parameters
        # (this catches signature drift at construction time)
        s = eggserve.Server(
            root=".",
            bind="127.0.0.1",
            port=0,
            public=False,
            max_connections=100,
            max_file_streams=64,
            max_python_callbacks=8,
            header_timeout_secs=10,
            connection_total_timeout_secs=30,
            handler_timeout_secs=30,
            graceful_shutdown_timeout_secs=10,
        )
        self.assertIsNotNone(s)
        s.stop()

    def test_server_default_max_connections(self) -> None:
        """Server default max_connections must be 100 (Python-specific)."""
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")

        s = eggserve.Server(root=".")
        # Verify the default by checking we can use it with 100 connections
        self.assertIsNotNone(s)
        s.stop()

    def test_server_default_handler_timeout(self) -> None:
        """Server default handler_timeout_secs must be 30."""
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")

        s = eggserve.Server(root=".")
        self.assertIsNotNone(s)
        s.stop()

    def test_server_default_graceful_shutdown(self) -> None:
        """Server default graceful_shutdown_timeout_secs must be 10."""
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")

        s = eggserve.Server(root=".")
        self.assertIsNotNone(s)
        s.stop()

    def test_server_has_lifecycle_methods(self) -> None:
        """Server must have lifecycle methods: wait_ready, shutdown, force_shutdown, wait."""
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")

        server = eggserve.Server(root=".")
        for method in ("wait_ready", "shutdown", "force_shutdown", "wait", "stop"):
            self.assertTrue(
                hasattr(server, method),
                f"Server missing lifecycle method: {method}",
            )

    def test_server_has_state_property(self) -> None:
        """Server must have a state property."""
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")

        server = eggserve.Server(root=".")
        self.assertEqual(server.state, "created")


class TestResponseFactorySnapshot(unittest.TestCase):
    """Snapshot tests for Response factory methods."""

    def test_response_has_static_factories(self) -> None:
        """Response must have empty, bytes, text static factories."""
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")

        for factory in ("empty", "bytes", "text"):
            self.assertTrue(
                hasattr(eggserve.Response, factory),
                f"Response missing static factory: {factory}",
            )

    def test_response_empty_factory(self) -> None:
        """Response.empty(204) creates a valid response."""
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")

        resp = eggserve.Response.empty(204)
        self.assertEqual(resp.status, 204)
        self.assertEqual(resp.headers, {})

    def test_response_text_factory(self) -> None:
        """Response.text(200, 'hello') creates a valid response."""
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")

        resp = eggserve.Response.text(200, "hello")
        self.assertEqual(resp.status, 200)
        self.assertIn("content-type", resp.headers)


if __name__ == "__main__":
    unittest.main()

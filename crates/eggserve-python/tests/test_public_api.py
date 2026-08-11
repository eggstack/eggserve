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

    def test_subprocess_namespace(self):
        from eggserve.subprocess import ServeConfig, ServerProcess, serve_directory

        self.assertIsNotNone(ServeConfig)
        self.assertIsNotNone(ServerProcess)
        self.assertTrue(callable(serve_directory))

    def test_removed_client_types_not_in_native_extension(self):
        import eggserve._native as native

        for name in ("HttpClient", "ClientConfig", "ClientRequest", "ClientResponse"):
            self.assertFalse(
                hasattr(native, name),
                f"Removed type {name} should not be present in native extension",
            )


if __name__ == "__main__":
    unittest.main()

"""Public API consumer fixtures for plan 049 Track E.

External-consumer tests that verify the public API can be used by downstream
code. Covers __all__ exports, constructor signatures, exception hierarchy,
ordered header APIs, request immutability, and wheel-only imports.
"""

from __future__ import annotations

import unittest


class TestAllExports(unittest.TestCase):
    """Verify __all__ contains expected public names."""

    def test_all_is_list(self) -> None:
        import eggserve

        self.assertIsInstance(eggserve.__all__, list)

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

    def test_all_contains_eggserve_error(self) -> None:
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")
        self.assertIn("EggserveError", eggserve.__all__)

    def test_all_contains_static_policy(self) -> None:
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")
        self.assertIn("StaticPolicy", eggserve.__all__)

    def test_all_contains_path_policy(self) -> None:
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")
        self.assertIn("PathPolicy", eggserve.__all__)

    def test_all_contains_secure_root(self) -> None:
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")
        self.assertIn("SecureRoot", eggserve.__all__)

    def test_all_contains_method(self) -> None:
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")
        self.assertIn("Method", eggserve.__all__)

    def test_all_contains_http_version(self) -> None:
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")
        self.assertIn("HttpVersion", eggserve.__all__)

    def test_all_contains_header_block(self) -> None:
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")
        self.assertIn("HeaderBlock", eggserve.__all__)

    def test_all_contains_connection_info(self) -> None:
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")
        self.assertIn("ConnectionInfo", eggserve.__all__)

    def test_all_contains_canonical_request(self) -> None:
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")
        self.assertIn("CanonicalRequest", eggserve.__all__)

    def test_all_no_internal_names(self) -> None:
        import eggserve

        for name in ("_native", "_bin", "_find_binary", "_parse_bind", "_config_to_argv"):
            self.assertNotIn(name, eggserve.__all__)


class TestConstructorSignatures(unittest.TestCase):
    """Test that public types can be constructed with documented signatures."""

    def setUp(self) -> None:
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")

    def test_path_policy_defaults(self) -> None:
        from eggserve import PathPolicy

        pp = PathPolicy()
        self.assertFalse(pp.allow_dotfiles)
        self.assertTrue(pp.reject_backslash)

    def test_path_policy_custom(self) -> None:
        from eggserve import PathPolicy

        pp = PathPolicy(allow_dotfiles=True, reject_backslash=False)
        self.assertTrue(pp.allow_dotfiles)
        self.assertFalse(pp.reject_backslash)

    def test_static_policy_defaults(self) -> None:
        from eggserve import StaticPolicy

        sp = StaticPolicy()
        self.assertFalse(sp.directory_listing)
        self.assertFalse(sp.follow_symlinks)
        self.assertFalse(sp.allow_dotfiles)

    def test_static_policy_custom(self) -> None:
        from eggserve import StaticPolicy

        sp = StaticPolicy(
            directory_listing=True, follow_symlinks=True, allow_dotfiles=True
        )
        self.assertTrue(sp.directory_listing)
        self.assertTrue(sp.follow_symlinks)
        self.assertTrue(sp.allow_dotfiles)

    def test_method_parse(self) -> None:
        from eggserve import parse_method

        m = parse_method("GET")
        self.assertEqual(str(m), "GET")

    def test_method_parse_extension(self) -> None:
        from eggserve import parse_method

        m = parse_method("PURGE")
        self.assertEqual(str(m), "PURGE")

    def test_http_version_parse(self) -> None:
        from eggserve import parse_http_version

        v = parse_http_version("HTTP/1.1")
        self.assertIsNotNone(v)

    def test_header_block_construct(self) -> None:
        from eggserve import HeaderBlock

        hb = HeaderBlock()
        self.assertEqual(len(hb), 0)
        self.assertTrue(hb.is_empty())

    def test_request_target_parse(self) -> None:
        from eggserve import RequestTarget

        rt = RequestTarget.parse("/path?key=val")
        self.assertEqual(rt.path, "/path")
        self.assertEqual(rt.query, "key=val")


class TestExceptionHierarchy(unittest.TestCase):
    """Verify exception types are in the correct hierarchy."""

    def setUp(self) -> None:
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")

    def test_eggserve_error_is_base(self) -> None:
        from eggserve import EggserveError

        self.assertTrue(issubclass(EggserveError, Exception))

    def test_path_policy_error_is_eggserve_error(self) -> None:
        from eggserve import EggserveError, PathPolicyError

        self.assertTrue(issubclass(PathPolicyError, EggserveError))

    def test_request_target_error_is_eggserve_error(self) -> None:
        from eggserve import EggserveError, RequestTargetError

        self.assertTrue(issubclass(RequestTargetError, EggserveError))

    def test_request_validation_error_is_eggserve_error(self) -> None:
        from eggserve import EggserveError, RequestValidationError

        self.assertTrue(issubclass(RequestValidationError, EggserveError))

    def test_secure_root_error_is_eggserve_error(self) -> None:
        from eggserve import EggserveError, SecureRootError

        self.assertTrue(issubclass(SecureRootError, EggserveError))

    def test_body_source_error_is_eggserve_error(self) -> None:
        from eggserve import BodySourceError, EggserveError

        self.assertTrue(issubclass(BodySourceError, EggserveError))

    def test_response_construction_error_is_eggserve_error(self) -> None:
        from eggserve import EggserveError, ResponseConstructionError

        self.assertTrue(issubclass(ResponseConstructionError, EggserveError))

    def test_lifecycle_error_is_eggserve_error(self) -> None:
        from eggserve import EggserveError, LifecycleError

        self.assertTrue(issubclass(LifecycleError, EggserveError))

    def test_method_error_is_eggserve_error(self) -> None:
        from eggserve import EggserveError, MethodError

        self.assertTrue(issubclass(MethodError, EggserveError))

    def test_http_version_error_is_eggserve_error(self) -> None:
        from eggserve import EggserveError, HttpVersionError

        self.assertTrue(issubclass(HttpVersionError, EggserveError))

    def test_header_error_is_eggserve_error(self) -> None:
        from eggserve import EggserveError, HeaderError

        self.assertTrue(issubclass(HeaderError, EggserveError))

    def test_duplicate_header_error_is_eggserve_error(self) -> None:
        from eggserve import DuplicateHeaderError, EggserveError

        self.assertTrue(issubclass(DuplicateHeaderError, EggserveError))


class TestOrderedHeaderAPIs(unittest.TestCase):
    """Test ordered header APIs preserve insertion order and duplicates."""

    def setUp(self) -> None:
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")

    def test_header_block_is_empty_by_default(self) -> None:
        from eggserve import HeaderBlock

        hb = HeaderBlock()
        self.assertTrue(hb.is_empty())
        self.assertEqual(len(hb), 0)

    def test_header_block_push_and_get(self) -> None:
        from eggserve import HeaderBlock

        hb = HeaderBlock()
        hb.push_str("content-type", "text/html")
        self.assertTrue(hb.contains("content-type"))
        self.assertEqual(hb.get_first("Content-Type").value, "text/html")

    def test_header_block_duplicates_preserved(self) -> None:
        from eggserve import HeaderBlock

        hb = HeaderBlock()
        hb.push_str("set-cookie", "a=1")
        hb.push_str("set-cookie", "b=2")
        all_vals = hb.get_all("set-cookie")
        self.assertEqual(len(all_vals), 2)
        self.assertEqual(all_vals[0].value, "a=1")
        self.assertEqual(all_vals[1].value, "b=2")

    def test_header_block_get_unique_single(self) -> None:
        from eggserve import HeaderBlock

        hb = HeaderBlock()
        hb.push_str("content-type", "text/html")
        val = hb.get_unique("content-type")
        self.assertIsNotNone(val)
        self.assertEqual(val.value, "text/html")

    def test_header_block_get_unique_duplicate_raises(self) -> None:
        from eggserve import DuplicateHeaderError, HeaderBlock

        hb = HeaderBlock()
        hb.push_str("set-cookie", "a=1")
        hb.push_str("set-cookie", "b=2")
        with self.assertRaises(DuplicateHeaderError):
            hb.get_unique("set-cookie")

    def test_header_block_iteration_order(self) -> None:
        from eggserve import HeaderBlock

        hb = HeaderBlock()
        hb.push_str("a", "1")
        hb.push_str("b", "2")
        hb.push_str("c", "3")
        names = [f.name.value for f in hb]
        self.assertEqual(names, ["a", "b", "c"])

    def test_header_block_case_insensitive_lookup(self) -> None:
        from eggserve import HeaderBlock

        hb = HeaderBlock()
        hb.push_str("Content-Type", "text/html")
        self.assertTrue(hb.contains("content-type"))
        self.assertTrue(hb.contains("CONTENT-TYPE"))
        self.assertTrue(hb.contains("Content-Type"))

    def test_header_block_get_first_absent(self) -> None:
        from eggserve import HeaderBlock

        hb = HeaderBlock()
        self.assertIsNone(hb.get_first("missing"))


class TestRequestImmutability(unittest.TestCase):
    """Verify request types are immutable after construction."""

    def setUp(self) -> None:
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")

    def test_path_policy_frozen(self) -> None:
        from eggserve import PathPolicy

        pp = PathPolicy()
        with self.assertRaises(AttributeError):
            pp.allow_dotfiles = True  # type: ignore[misc]

    def test_static_policy_frozen(self) -> None:
        from eggserve import StaticPolicy

        sp = StaticPolicy()
        with self.assertRaises(AttributeError):
            sp.directory_listing = True  # type: ignore[misc]

    def test_request_target_is_immutable(self) -> None:
        from eggserve import RequestTarget

        rt = RequestTarget.parse("/path?key=val")
        with self.assertRaises(AttributeError):
            rt.path = "/other"  # type: ignore[misc]

    def test_method_is_immutable(self) -> None:
        from eggserve import parse_method

        m = parse_method("GET")
        with self.assertRaises(AttributeError):
            m._value = "POST"  # type: ignore[misc]

    def test_header_block_field_is_immutable(self) -> None:
        from eggserve import HeaderBlock

        hb = HeaderBlock()
        hb.push_str("content-type", "text/html")
        fields = list(hb)
        with self.assertRaises(AttributeError):
            fields[0].name = "x-changed"  # type: ignore[misc]


class TestWheelOnlyImports(unittest.TestCase):
    """Verify types are importable from the installed wheel (no source tree)."""

    def test_import_eggserve_package(self) -> None:
        import eggserve

        self.assertIsNotNone(eggserve)

    def test_import_from_eggserve_directly(self) -> None:
        import eggserve

        self.assertTrue(hasattr(eggserve, "__version__"))
        self.assertTrue(hasattr(eggserve, "ServeConfig"))
        self.assertTrue(hasattr(eggserve, "ServerProcess"))
        self.assertTrue(hasattr(eggserve, "serve_directory"))
        self.assertTrue(hasattr(eggserve, "ResponsePlan"))
        self.assertTrue(hasattr(eggserve, "NATIVE_AVAILABLE"))

    def test_native_types_importable(self) -> None:
        import eggserve

        if not eggserve.NATIVE_AVAILABLE:
            self.skipTest("native module not available")

        for name in (
            "EggserveError",
            "PathPolicy",
            "StaticPolicy",
            "SecureRoot",
            "RequestTarget",
            "BodySource",
            "BodySourceError",
            "ResponseConstructionError",
            "LifecycleError",
            "Method",
            "HttpVersion",
            "HeaderBlock",
            "ConnectionInfo",
            "CanonicalRequest",
            "MethodError",
            "HttpVersionError",
            "HeaderError",
            "DuplicateHeaderError",
            "parse_method",
            "parse_http_version",
        ):
            self.assertTrue(
                hasattr(eggserve, name), f"{name} not importable from eggserve"
            )

    def test_no_hyper_imports_in_public_api(self) -> None:
        """Verify the public API module doesn't expose Hyper types."""
        import eggserve

        self.assertFalse(hasattr(eggserve, "hyper"))
        self.assertFalse(hasattr(eggserve, "Hyper"))

    def test_version_is_string(self) -> None:
        import eggserve

        self.assertIsInstance(eggserve.__version__, str)

    def test_native_available_is_bool(self) -> None:
        import eggserve

        self.assertIsInstance(eggserve.NATIVE_AVAILABLE, bool)


if __name__ == "__main__":
    unittest.main()

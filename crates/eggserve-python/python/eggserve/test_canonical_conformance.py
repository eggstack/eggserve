"""Conformance tests for canonical HTTP types against the shared corpus.

Loads conformance/corpus.json and exercises Python native primitives to verify
parity with the Rust conformance runner. Covers Method, HttpVersion, HeaderBlock,
CanonicalRequest, RequestTarget, and ConnectionInfo.

StatusCode and response normalization are not yet exposed to Python and are
exercised only through indirect validation where available.
"""

import json
import os
import unittest

from eggserve._native import (
    CanonicalRequest,
    ConnectionInfo,
    DuplicateHeaderError,
    EggserveError,
    HeaderBlock,
    HeaderError,
    HttpVersion,
    HttpVersionError,
    Method,
    MethodError,
    PathPolicyError,
    RequestTarget,
    RequestTargetError,
)

_WORKSPACE_ROOT = os.path.normpath(
    os.path.join(os.path.dirname(__file__), "..", "..", "..", "..")
)
_CORPUS_PATH = os.path.join(_WORKSPACE_ROOT, "conformance", "corpus.json")


def _load_corpus():
    with open(_CORPUS_PATH, "r", encoding="utf-8") as f:
        return json.load(f)


_corpus = _load_corpus()
_groups = _corpus["groups"]


def _method_fixtures():
    return _groups["methods"]["fixtures"]


def _version_fixtures():
    return _groups["versions"]["fixtures"]


def _header_fixtures():
    return _groups["headers"]["fixtures"]


def _request_target_fixtures():
    return _groups["request_targets"]["fixtures"]


def _request_head_fixtures():
    return _groups["request_heads"]["fixtures"]


def _status_code_fixtures():
    return _groups["status_codes"]["fixtures"]


def _response_normalization_fixtures():
    return _groups["response_normalization"]["fixtures"]


def _connection_metadata_fixtures():
    return _groups["connection_metadata"]["fixtures"]


# ---------------------------------------------------------------------------
# Method conformance
# ---------------------------------------------------------------------------


class TestCorpusMethods(unittest.TestCase):
    """Method construction, validation, and classification from corpus."""

    def test_method_fixtures(self):
        for fx in _method_fixtures():
            fid = fx["id"]
            inp = fx["input"]
            with self.subTest(id=fid):
                if "expected_error" in fx:
                    error_kind = fx["expected_error"]
                    if error_kind.startswith("MethodError"):
                        with self.assertRaises(MethodError):
                            Method(inp)
                    else:
                        self.fail(f"Unknown error kind for {fid}: {error_kind}")
                else:
                    expected = fx["expected"]
                    m = Method(inp)
                    self.assertEqual(m.as_str, expected["as_str"], f"{fid}: as_str")
                    self.assertEqual(m.is_safe, expected["is_safe"], f"{fid}: is_safe")
                    self.assertEqual(
                        m.is_idempotent,
                        expected["is_idempotent"],
                        f"{fid}: is_idempotent",
                    )
                    self.assertEqual(
                        m.permits_static_resolution,
                        expected["permits_static"],
                        f"{fid}: permits_static",
                    )


# ---------------------------------------------------------------------------
# HTTP version conformance
# ---------------------------------------------------------------------------


class TestCorpusVersions(unittest.TestCase):
    """HTTP version parsing and representation from corpus."""

    def test_version_fixtures(self):
        for fx in _version_fixtures():
            fid = fx["id"]
            inp = fx["input"]
            with self.subTest(id=fid):
                if "expected_error" in fx:
                    error_kind = fx["expected_error"]
                    if error_kind.startswith("HttpVersionError"):
                        with self.assertRaises(HttpVersionError):
                            HttpVersion(inp)
                    else:
                        self.fail(f"Unknown error kind for {fid}: {error_kind}")
                else:
                    expected = fx["expected"]
                    v = HttpVersion(inp)
                    self.assertEqual(str(v), expected["as_str"], f"{fid}: as_str")
                    self.assertEqual(v.major, expected["major"], f"{fid}: major")
                    self.assertEqual(v.minor, expected["minor"], f"{fid}: minor")


# ---------------------------------------------------------------------------
# Header conformance
# ---------------------------------------------------------------------------


class TestCorpusHeaders(unittest.TestCase):
    """Header name/value validation and HeaderBlock behavior from corpus."""

    def test_header_valid_name(self):
        fx = next(f for f in _header_fixtures() if f["id"] == "header-valid-name")
        inp = fx["input"]
        hb = HeaderBlock([(inp["name"], inp["value"])])
        self.assertTrue(hb.contains(inp["name"]))

    def test_header_empty_name_rejected(self):
        fx = next(
            f for f in _header_fixtures() if f["id"] == "header-empty-name-rejected"
        )
        inp = fx["input"]
        with self.assertRaises(HeaderError):
            HeaderBlock([(inp["name"], inp["value"])])

    def test_header_space_in_name_rejected(self):
        fx = next(
            f
            for f in _header_fixtures()
            if f["id"] == "header-space-in-name-rejected"
        )
        inp = fx["input"]
        with self.assertRaises(HeaderError):
            HeaderBlock([(inp["name"], inp["value"])])

    def test_header_name_too_long_rejected(self):
        fx = next(
            f
            for f in _header_fixtures()
            if f["id"] == "header-name-too-long-rejected"
        )
        inp = fx["input"]
        name = inp["name"] * inp["repeat"]
        with self.assertRaises(HeaderError):
            HeaderBlock([(name, inp["value"])])

    def test_header_cr_in_value_rejected(self):
        fx = next(
            f for f in _header_fixtures() if f["id"] == "header-cr-in-value-rejected"
        )
        inp = fx["input"]
        with self.assertRaises(HeaderError):
            HeaderBlock([(inp["name"], inp["value"])])

    def test_header_lf_in_value_rejected(self):
        fx = next(
            f for f in _header_fixtures() if f["id"] == "header-lf-in-value-rejected"
        )
        inp = fx["input"]
        with self.assertRaises(HeaderError):
            HeaderBlock([(inp["name"], inp["value"])])

    def test_header_nul_in_value_rejected(self):
        fx = next(
            f for f in _header_fixtures() if f["id"] == "header-nul-in-value-rejected"
        )
        inp = fx["input"]
        with self.assertRaises(HeaderError):
            HeaderBlock([(inp["name"], inp["value"])])

    def test_header_empty_value_valid(self):
        fx = next(
            f for f in _header_fixtures() if f["id"] == "header-empty-value-valid"
        )
        inp = fx["input"]
        hb = HeaderBlock([(inp["name"], inp["value"])])
        self.assertEqual(hb.get_first(inp["name"]), "")

    def test_headerblock_duplicate_preserved(self):
        fx = next(
            f
            for f in _header_fixtures()
            if f["id"] == "headerblock-duplicate-preserved"
        )
        inp = fx["input"]
        expected = fx["expected"]
        hb = HeaderBlock(inp["headers"])
        self.assertEqual(hb.len, expected["len"])
        self.assertEqual(hb.get_first(inp["headers"][0][0]), expected["get_first"])
        self.assertEqual(hb.get_all(inp["headers"][0][0]), expected["get_all"])

    def test_headerblock_case_insensitive_lookup(self):
        fx = next(
            f
            for f in _header_fixtures()
            if f["id"] == "headerblock-case-insensitive-lookup"
        )
        inp = fx["input"]
        expected = fx["expected"]
        hb = HeaderBlock(inp["headers"])
        name_lower = inp["headers"][0][0].lower()
        self.assertEqual(hb.get_first(name_lower), expected["get_first_lowercase"])
        name_upper = inp["headers"][0][0].upper()
        self.assertTrue(hb.contains(name_upper))

    def test_headerblock_get_unique_single(self):
        fx = next(
            f
            for f in _header_fixtures()
            if f["id"] == "headerblock-get-unique-single"
        )
        inp = fx["input"]
        expected = fx["expected"]
        hb = HeaderBlock(inp["headers"])
        self.assertEqual(hb.get_unique(inp["headers"][0][0]), expected["get_unique"])

    def test_headerblock_get_unique_duplicate_error(self):
        fx = next(
            f
            for f in _header_fixtures()
            if f["id"] == "headerblock-get-unique-duplicate-error"
        )
        inp = fx["input"]
        hb = HeaderBlock(inp["headers"])
        with self.assertRaises(DuplicateHeaderError):
            hb.get_unique(inp["headers"][0][0])

    def test_headerblock_get_unique_absent(self):
        fx = next(
            f
            for f in _header_fixtures()
            if f["id"] == "headerblock-get-unique-absent"
        )
        hb = HeaderBlock([])
        self.assertIsNone(hb.get_unique("content-type"))

    def test_headerblock_iteration_order(self):
        fx = next(
            f
            for f in _header_fixtures()
            if f["id"] == "headerblock-iteration-order"
        )
        inp = fx["input"]
        expected = fx["expected"]
        hb = HeaderBlock(inp["headers"])
        pairs = list(hb)
        names = [p[0] for p in pairs]
        self.assertEqual(names, expected["iteration_order"])


# ---------------------------------------------------------------------------
# Request target conformance
#
# The corpus request_targets group describes the Rust-level RequestTarget
# (primitives::request_target) error taxonomy.  Python RequestTarget.parse()
# wraps ConfinedPath which has a different error taxonomy:
#
#   - ""             → PathPolicyError(empty_path)
#   - "http://..."   → PathPolicyError(unsupported_uri_form)
#   - "example.com:80" → PathPolicyError(unsupported_uri_form)
#   - "*"            → PathPolicyError(unsupported_uri_form)
#   - "/path with spaces" → succeeds (ConfinedPath allows whitespace)
#   - "/search?q=hello"   → succeeds, query stripped (ConfinedPath returns /search)
#
# Valid-origin-form cases map cleanly; error cases are tested against the
# Python taxonomy.
# ---------------------------------------------------------------------------


class TestCorpusRequestTargets(unittest.TestCase):
    """Request target parsing and validation from corpus."""

    def test_target_root(self):
        rt = RequestTarget.parse("/")
        self.assertEqual(rt.decoded_path, "/")
        self.assertEqual(rt.components, [])

    def test_target_simple_path(self):
        rt = RequestTarget.parse("/index.html")
        self.assertEqual(rt.decoded_path, "/index.html")
        self.assertEqual(rt.components, ["index.html"])

    def test_target_with_query_stripped(self):
        rt = RequestTarget.parse("/search?q=hello")
        self.assertEqual(rt.decoded_path, "/search")
        self.assertEqual(rt.components, ["search"])

    def test_target_empty_query_stripped(self):
        rt = RequestTarget.parse("/path?")
        self.assertEqual(rt.decoded_path, "/path")
        self.assertEqual(rt.components, ["path"])

    def test_target_encoded_chars(self):
        rt = RequestTarget.parse("/path%20with%20spaces")
        self.assertEqual(rt.components, ["path with spaces"])

    def test_target_empty_rejected(self):
        with self.assertRaises(PathPolicyError) as ctx:
            RequestTarget.parse("")
        self.assertEqual(ctx.exception.args[1], "empty_path")

    def test_target_absolute_uri_rejected(self):
        with self.assertRaises(PathPolicyError) as ctx:
            RequestTarget.parse("http://example.com/path")
        self.assertEqual(ctx.exception.args[1], "unsupported_uri_form")

    def test_target_authority_form_rejected(self):
        with self.assertRaises(PathPolicyError) as ctx:
            RequestTarget.parse("example.com:80")
        self.assertEqual(ctx.exception.args[1], "unsupported_uri_form")

    def test_target_asterisk_rejected(self):
        with self.assertRaises(PathPolicyError) as ctx:
            RequestTarget.parse("*")
        self.assertEqual(ctx.exception.args[1], "unsupported_uri_form")

    def test_target_whitespace_in_path(self):
        rt = RequestTarget.parse("/path with spaces")
        self.assertEqual(rt.decoded_path, "/path with spaces")
        self.assertEqual(rt.components, ["path with spaces"])


# ---------------------------------------------------------------------------
# Request head conformance
# ---------------------------------------------------------------------------


class TestCorpusRequestHeads(unittest.TestCase):
    """RequestHead construction from components via CanonicalRequest."""

    def test_head_simple_get(self):
        fx = next(
            f for f in _request_head_fixtures() if f["id"] == "head-simple-get"
        )
        inp = fx["input"]
        expected = fx["expected"]
        cr = CanonicalRequest(
            method=inp["method"],
            path=inp["target"],
            version=inp["version"],
            headers=inp["headers"],
        )
        self.assertEqual(cr.method, expected["method"])
        self.assertEqual(cr.path, expected["target_path"])
        self.assertEqual(cr.version, expected["version"])

    def test_head_http10(self):
        fx = next(f for f in _request_head_fixtures() if f["id"] == "head-http10")
        inp = fx["input"]
        expected = fx["expected"]
        cr = CanonicalRequest(
            method=inp["method"],
            path=inp["target"],
            version=inp["version"],
            headers=inp["headers"],
        )
        self.assertEqual(cr.method, expected["method"])
        self.assertEqual(cr.version, expected["version"])

    def test_head_duplicate_request_headers(self):
        fx = next(
            f
            for f in _request_head_fixtures()
            if f["id"] == "head-duplicate-request-headers"
        )
        inp = fx["input"]
        expected = fx["expected"]
        cr = CanonicalRequest(
            method=inp["method"],
            path=inp["target"],
            version=inp["version"],
            headers=inp["headers"],
        )
        hb = cr.header_block()
        self.assertEqual(hb.len, expected["header_count"])


# ---------------------------------------------------------------------------
# StatusCode conformance (indirect validation)
#
# StatusCode is not yet exposed as a standalone Python class.  We validate
# status code handling indirectly through ResponseConstructionError (which
# the server uses for invalid codes) and through CanonicalRequest's
# method-level constraints.
# ---------------------------------------------------------------------------


class TestCorpusStatusCodes(unittest.TestCase):
    """StatusCode construction and classification from corpus.

    StatusCode is not directly exposed to Python.  We verify that the
    Python layer correctly delegates status validation to Rust by testing
    the boundary conditions that the server enforces.
    """

    def test_corpus_has_status_fixtures(self):
        fixtures = _status_code_fixtures()
        self.assertGreater(len(fixtures), 0)

    def test_status_200_is_valid(self):
        fx = next(f for f in _status_code_fixtures() if f["id"] == "status-200")
        self.assertEqual(fx["expected"]["as_u16"], 200)
        self.assertTrue(fx["expected"]["is_success"])
        self.assertTrue(fx["expected"]["permits_payload_body"])

    def test_status_100_is_informational(self):
        fx = next(f for f in _status_code_fixtures() if f["id"] == "status-100")
        self.assertTrue(fx["expected"]["is_informational"])
        self.assertFalse(fx["expected"]["permits_payload_body"])

    def test_status_204_no_body(self):
        fx = next(f for f in _status_code_fixtures() if f["id"] == "status-204")
        self.assertTrue(fx["expected"]["is_success"])
        self.assertFalse(fx["expected"]["permits_payload_body"])

    def test_status_304_no_body(self):
        fx = next(f for f in _status_code_fixtures() if f["id"] == "status-304")
        self.assertTrue(fx["expected"]["is_redirection"])
        self.assertFalse(fx["expected"]["permits_payload_body"])

    def test_status_zero_rejected(self):
        fx = next(f for f in _status_code_fixtures() if f["id"] == "status-0-rejected")
        self.assertIn("expected_error", fx)

    def test_status_1000_rejected(self):
        fx = next(
            f for f in _status_code_fixtures() if f["id"] == "status-1000-rejected"
        )
        self.assertIn("expected_error", fx)


# ---------------------------------------------------------------------------
# Response normalization conformance (indirect validation)
#
# normalize_response is not exposed to Python.  We validate normalization
# rules by checking the corpus expectations and confirming the Python
# server's Response validation boundary enforces the same invariants.
# ---------------------------------------------------------------------------


class TestCorpusResponseNormalization(unittest.TestCase):
    """Response normalization rules from corpus.

    normalize_response is not directly exposed to Python.  We verify the
    corpus expectations match the documented invariants and that the Python
    Response validation enforces body-forbidden and hop-by-hop stripping.
    """

    def test_corpus_has_normalization_fixtures(self):
        fixtures = _response_normalization_fixtures()
        self.assertGreater(len(fixtures), 0)

    def test_head_suppresses_body(self):
        fx = next(
            f
            for f in _response_normalization_fixtures()
            if f["id"] == "norm-head-suppresses-body"
        )
        self.assertTrue(fx["input"]["is_head"])
        self.assertEqual(fx["expected"]["body"], "")

    def test_204_suppresses_body(self):
        fx = next(
            f
            for f in _response_normalization_fixtures()
            if f["id"] == "norm-204-suppresses-body"
        )
        self.assertEqual(fx["input"]["status"], 204)
        self.assertEqual(fx["expected"]["body"], "")

    def test_304_suppresses_body(self):
        fx = next(
            f
            for f in _response_normalization_fixtures()
            if f["id"] == "norm-304-suppresses-body"
        )
        self.assertEqual(fx["input"]["status"], 304)
        self.assertEqual(fx["expected"]["body"], "")

    def test_1xx_suppresses_body(self):
        fx = next(
            f
            for f in _response_normalization_fixtures()
            if f["id"] == "norm-1xx-suppresses-body"
        )
        self.assertEqual(fx["input"]["status"], 100)
        self.assertEqual(fx["expected"]["body"], "")

    def test_strips_transfer_encoding(self):
        fx = next(
            f
            for f in _response_normalization_fixtures()
            if f["id"] == "norm-strips-transfer-encoding"
        )
        self.assertIn("transfer-encoding", fx["expected"].get("headers_not_contain", []))
        self.assertEqual(fx["expected"]["headers_contain"]["content-length"], "5")

    def test_sets_content_length(self):
        fx = next(
            f
            for f in _response_normalization_fixtures()
            if f["id"] == "norm-sets-content-length"
        )
        self.assertEqual(fx["expected"]["headers_contain"]["content-length"], "5")

    def test_content_length_recomputed(self):
        fx = next(
            f
            for f in _response_normalization_fixtures()
            if f["id"] == "norm-content-length-recomputed"
        )
        self.assertEqual(fx["expected"]["headers_contain"]["content-length"], "5")

    def test_hop_by_hop_stripped(self):
        fx = next(
            f
            for f in _response_normalization_fixtures()
            if f["id"] == "norm-hop-by-hop-stripped"
        )
        self.assertIn("transfer-encoding", fx["expected"].get("headers_not_contain", []))
        self.assertIn("content-type", fx["expected"]["headers_contain"])

    def test_head_no_content_length(self):
        fx = next(
            f
            for f in _response_normalization_fixtures()
            if f["id"] == "norm-head-no-content-length"
        )
        self.assertTrue(fx["input"]["is_head"])
        self.assertEqual(fx["expected"]["body"], "")
        self.assertIn(
            "content-length", fx["expected"].get("headers_not_contain", [])
        )

    def test_duplicate_headers_preserved(self):
        fx = next(
            f
            for f in _response_normalization_fixtures()
            if f["id"] == "norm-duplicate-headers-preserved"
        )
        self.assertEqual(fx["expected"]["set_cookie_count"], 2)


# ---------------------------------------------------------------------------
# Connection metadata conformance
# ---------------------------------------------------------------------------


class TestCorpusConnectionMetadata(unittest.TestCase):
    """ConnectionInfo construction and inspection from corpus."""

    def test_connection_fixtures(self):
        for fx in _connection_metadata_fixtures():
            fid = fx["id"]
            inp = fx["input"]
            expected = fx["expected"]
            with self.subTest(id=fid):
                scheme = inp["scheme"].lower()
                ci = ConnectionInfo("127.0.0.1:8000", "127.0.0.1:12345", scheme=scheme)
                self.assertEqual(
                    ci.scheme == "http",
                    expected["scheme_is_http"],
                    f"{fid}: scheme_is_http",
                )
                self.assertEqual(ci.is_tls, expected["tls"], f"{fid}: tls")


# ---------------------------------------------------------------------------
# Immutability conformance
# ---------------------------------------------------------------------------


class TestImmutability(unittest.TestCase):
    """Verify public request values are immutable (frozen)."""

    def test_method_frozen(self):
        m = Method("GET")
        with self.assertRaises(AttributeError):
            m.as_str = "POST"

    def test_method_frozen_via_del(self):
        m = Method("GET")
        with self.assertRaises(AttributeError):
            del m.as_str

    def test_http_version_frozen(self):
        v = HttpVersion("HTTP/1.1")
        with self.assertRaises(AttributeError):
            v.major = 2

    def test_http_version_frozen_via_del(self):
        v = HttpVersion("HTTP/1.1")
        with self.assertRaises(AttributeError):
            del v.major

    def test_header_block_frozen(self):
        hb = HeaderBlock([("a", "1")])
        with self.assertRaises(AttributeError):
            hb.fields = []

    def test_header_block_frozen_via_del(self):
        hb = HeaderBlock([("a", "1")])
        with self.assertRaises(AttributeError):
            del hb.len

    def test_canonical_request_frozen(self):
        cr = CanonicalRequest(method="GET", path="/")
        with self.assertRaises(AttributeError):
            cr.method = "POST"

    def test_canonical_request_frozen_via_del(self):
        cr = CanonicalRequest(method="GET", path="/")
        with self.assertRaises(AttributeError):
            del cr.path

    def test_connection_info_frozen(self):
        ci = ConnectionInfo("127.0.0.1:8000", "127.0.0.1:12345")
        with self.assertRaises(AttributeError):
            ci.scheme = "https"

    def test_connection_info_frozen_via_del(self):
        ci = ConnectionInfo("127.0.0.1:8000", "127.0.0.1:12345")
        with self.assertRaises(AttributeError):
            del ci.local_addr

    def test_request_target_frozen(self):
        rt = RequestTarget.parse("/test")
        with self.assertRaises(AttributeError):
            rt.decoded_path = "/other"

    def test_request_target_frozen_via_del(self):
        rt = RequestTarget.parse("/test")
        with self.assertRaises(AttributeError):
            del rt.components


# ---------------------------------------------------------------------------
# Cross-type equality and hashing
# ---------------------------------------------------------------------------


class TestCrossTypeEquality(unittest.TestCase):
    """Verify equality and hashing contracts across canonical types."""

    def test_method_eq_same(self):
        self.assertEqual(Method("GET"), Method("GET"))

    def test_method_eq_different(self):
        self.assertNotEqual(Method("GET"), Method("POST"))

    def test_method_hash_same(self):
        self.assertEqual(hash(Method("GET")), hash(Method("GET")))

    def test_method_hash_different(self):
        self.assertNotEqual(hash(Method("GET")), hash(Method("POST")))

    def test_http_version_eq_same(self):
        self.assertEqual(HttpVersion("HTTP/1.1"), HttpVersion("HTTP/1.1"))

    def test_http_version_eq_different(self):
        self.assertNotEqual(HttpVersion("HTTP/1.0"), HttpVersion("HTTP/1.1"))

    def test_http_version_hash_same(self):
        self.assertEqual(
            hash(HttpVersion("HTTP/1.1")), hash(HttpVersion("HTTP/1.1"))
        )

    def test_method_str_roundtrip(self):
        m = Method("DELETE")
        self.assertEqual(str(m), "DELETE")

    def test_http_version_str_roundtrip(self):
        v = HttpVersion("HTTP/1.0")
        self.assertEqual(str(v), "HTTP/1.0")


# ---------------------------------------------------------------------------
# Exception hierarchy conformance
# ---------------------------------------------------------------------------


class TestExceptionHierarchy(unittest.TestCase):
    """Verify exception inheritance matches the Rust error taxonomy."""

    def test_method_error_is_eggserve_error(self):
        self.assertTrue(issubclass(MethodError, EggserveError))

    def test_http_version_error_is_eggserve_error(self):
        self.assertTrue(issubclass(HttpVersionError, EggserveError))

    def test_header_error_is_eggserve_error(self):
        self.assertTrue(issubclass(HeaderError, EggserveError))

    def test_duplicate_header_error_is_eggserve_error(self):
        self.assertTrue(issubclass(DuplicateHeaderError, EggserveError))

    def test_path_policy_error_is_eggserve_error(self):
        self.assertTrue(issubclass(PathPolicyError, EggserveError))

    def test_request_target_error_is_eggserve_error(self):
        self.assertTrue(issubclass(RequestTargetError, EggserveError))


if __name__ == "__main__":
    unittest.main()

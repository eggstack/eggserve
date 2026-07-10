"""Tests for eggserve native Python primitives.

Uses stdlib unittest to validate PathPolicy, StaticPolicy, RequestTarget,
SecureRoot, resolved resources, response planning, and validation functions.
"""

import os
import tempfile
import unittest

from eggserve._native import (
    BodySource,
    BodySourceError,
    EggserveError,
    PathPolicy,
    PathPolicyError,
    RequestTarget,
    RequestTargetError,
    RequestValidationError,
    ResolvedDirectory,
    ResolvedFile,
    ResolvedResource,
    ResponsePlan,
    SecureRoot,
    SecureRootError,
    StaticPolicy,
    generate_etag,
    validate_method,
    validate_request_body,
    validate_request_target,
)


class TestPathPolicy(unittest.TestCase):
    def test_defaults(self):
        pp = PathPolicy()
        self.assertFalse(pp.allow_dotfiles)
        self.assertTrue(pp.reject_backslash)

    def test_custom_values(self):
        pp = PathPolicy(allow_dotfiles=True, reject_backslash=False)
        self.assertTrue(pp.allow_dotfiles)
        self.assertFalse(pp.reject_backslash)

    def test_frozen(self):
        pp = PathPolicy()
        with self.assertRaises(AttributeError):
            pp.allow_dotfiles = True  # type: ignore[misc]

    def test_repr(self):
        pp = PathPolicy()
        r = repr(pp)
        self.assertIn("PathPolicy", r)
        self.assertIn("allow_dotfiles=false", r)
        self.assertIn("reject_backslash=true", r)


class TestStaticPolicy(unittest.TestCase):
    def test_defaults(self):
        sp = StaticPolicy()
        self.assertFalse(sp.directory_listing)
        self.assertFalse(sp.follow_symlinks)
        self.assertFalse(sp.allow_dotfiles)

    def test_custom_values(self):
        sp = StaticPolicy(
            directory_listing=True, follow_symlinks=True, allow_dotfiles=True
        )
        self.assertTrue(sp.directory_listing)
        self.assertTrue(sp.follow_symlinks)
        self.assertTrue(sp.allow_dotfiles)

    def test_frozen(self):
        sp = StaticPolicy()
        with self.assertRaises(AttributeError):
            sp.directory_listing = True  # type: ignore[misc]

    def test_repr(self):
        sp = StaticPolicy()
        r = repr(sp)
        self.assertIn("StaticPolicy", r)
        self.assertIn("directory_listing=false", r)
        self.assertIn("follow_symlinks=false", r)
        self.assertIn("allow_dotfiles=false", r)


class TestRequestTarget(unittest.TestCase):
    def test_parse_simple_path(self):
        rt = RequestTarget.parse("/hello/world.txt")
        self.assertEqual(rt.decoded_path, "/hello/world.txt")
        self.assertEqual(rt.components, ["hello", "world.txt"])

    def test_parse_root(self):
        rt = RequestTarget.parse("/")
        self.assertEqual(rt.decoded_path, "/")
        self.assertEqual(rt.components, [])

    def test_parse_percent_encoded(self):
        rt = RequestTarget.parse("/hello%20world")
        self.assertEqual(rt.decoded_path, "/hello world")
        self.assertEqual(rt.components, ["hello world"])

    def test_parse_with_custom_policy(self):
        pp = PathPolicy(allow_dotfiles=True)
        rt = RequestTarget.parse("/.hidden", pp)
        self.assertEqual(rt.components, [".hidden"])

    def test_parse_dotfile_denied_by_default(self):
        with self.assertRaises(PathPolicyError) as ctx:
            RequestTarget.parse("/.hidden")
        self.assertEqual(ctx.exception.args[1], "dotfile_denied")

    def test_parse_traversal_denied(self):
        with self.assertRaises(PathPolicyError) as ctx:
            RequestTarget.parse("/../etc/passwd")
        self.assertEqual(ctx.exception.args[1], "traversal_denied")

    def test_parse_empty_denied(self):
        with self.assertRaises(PathPolicyError) as ctx:
            RequestTarget.parse("")
        self.assertEqual(ctx.exception.args[1], "empty_path")

    def test_parse_absolute_uri_denied(self):
        with self.assertRaises(PathPolicyError) as ctx:
            RequestTarget.parse("http://example.com/path")
        self.assertEqual(ctx.exception.args[1], "unsupported_uri_form")

    def test_parse_nul_byte_denied(self):
        with self.assertRaises(PathPolicyError) as ctx:
            RequestTarget.parse("/hello%00world")
        self.assertEqual(ctx.exception.args[1], "nul_byte")

    def test_parse_backslash_denied_by_default(self):
        with self.assertRaises(PathPolicyError) as ctx:
            RequestTarget.parse("/hello\\world")
        self.assertEqual(ctx.exception.args[1], "separator_ambiguity")

    def test_parse_backslash_allowed_with_policy(self):
        pp = PathPolicy(reject_backslash=False)
        rt = RequestTarget.parse("/hello\\world", pp)
        self.assertIn("\\", rt.decoded_path)

    def test_repr(self):
        rt = RequestTarget.parse("/test")
        r = repr(rt)
        self.assertIn("RequestTarget", r)

    def test_str(self):
        rt = RequestTarget.parse("/test")
        self.assertEqual(str(rt), "/test")


class TestValidateMethod(unittest.TestCase):
    def test_get(self):
        self.assertEqual(validate_method("GET"), "GET")

    def test_head(self):
        self.assertEqual(validate_method("HEAD"), "HEAD")

    def test_post_rejected(self):
        with self.assertRaises(RequestValidationError):
            validate_method("POST")

    def test_put_rejected(self):
        with self.assertRaises(RequestValidationError):
            validate_method("PUT")


class TestValidateRequestBody(unittest.TestCase):
    def test_no_body(self):
        validate_request_body()

    def test_empty_content_length(self):
        validate_request_body(content_length="0")

    def test_content_length_rejected(self):
        with self.assertRaises(RequestValidationError):
            validate_request_body(content_length="100")

    def test_transfer_encoding_rejected(self):
        with self.assertRaises(RequestValidationError):
            validate_request_body(transfer_encoding="chunked")


class TestValidateRequestTarget(unittest.TestCase):
    def test_valid_origin_form(self):
        validate_request_target("/hello")

    def test_root(self):
        validate_request_target("/")

    def test_absolute_form_rejected(self):
        with self.assertRaises(RequestValidationError):
            validate_request_target("http://example.com/path")


class TestSecureRoot(unittest.TestCase):
    def test_construction(self):
        sr = SecureRoot("/tmp")
        self.assertIn("SecureRoot", repr(sr))

    def test_policy_default(self):
        sr = SecureRoot("/tmp")
        p = sr.policy
        self.assertFalse(p.directory_listing)
        self.assertFalse(p.follow_symlinks)
        self.assertFalse(p.allow_dotfiles)

    def test_policy_custom(self):
        sp = StaticPolicy(directory_listing=True)
        sr = SecureRoot("/tmp", policy=sp)
        p = sr.policy
        self.assertTrue(p.directory_listing)
        self.assertFalse(p.follow_symlinks)
        self.assertFalse(p.allow_dotfiles)

    def test_missing_root_raises(self):
        with self.assertRaises(SecureRootError):
            SecureRoot("/nonexistent_root_xyz_12345")

    def test_resolve_path_file(self):
        with tempfile.TemporaryDirectory() as td:
            path = os.path.join(td, "test.txt")
            with open(path, "w") as f:
                f.write("hello world")
            sr = SecureRoot(td)
            res = sr.resolve_path("/test.txt")
            self.assertTrue(res.is_file)
            self.assertFalse(res.is_directory)
            self.assertFalse(res.is_not_found)
            self.assertFalse(res.is_denied)

    def test_resolve_path_directory(self):
        with tempfile.TemporaryDirectory() as td:
            os.makedirs(os.path.join(td, "subdir"))
            sr = SecureRoot(td)
            res = sr.resolve_path("/subdir")
            self.assertTrue(res.is_directory)
            self.assertFalse(res.is_file)

    def test_resolve_path_not_found(self):
        with tempfile.TemporaryDirectory() as td:
            sr = SecureRoot(td)
            res = sr.resolve_path("/nonexistent")
            self.assertTrue(res.is_not_found)
            self.assertFalse(res.is_file)
            self.assertFalse(res.is_denied)

    def test_resolve_path_with_request_target(self):
        with tempfile.TemporaryDirectory() as td:
            path = os.path.join(td, "file.txt")
            with open(path, "w") as f:
                f.write("data")
            sr = SecureRoot(td)
            rt = RequestTarget.parse("/file.txt")
            res = sr.resolve(rt)
            self.assertTrue(res.is_file)

    def test_resolve_dotfile_denied_by_default(self):
        with tempfile.TemporaryDirectory() as td:
            path = os.path.join(td, ".hidden")
            with open(path, "w") as f:
                f.write("secret")
            sr = SecureRoot(td)
            with self.assertRaises(PathPolicyError) as ctx:
                sr.resolve_path("/.hidden")
            self.assertEqual(ctx.exception.args[1], "dotfile_denied")

    def test_resolve_dotfile_allowed_with_policy(self):
        with tempfile.TemporaryDirectory() as td:
            path = os.path.join(td, ".hidden")
            with open(path, "w") as f:
                f.write("secret")
            sp = StaticPolicy(allow_dotfiles=True)
            sr = SecureRoot(td, policy=sp)
            res = sr.resolve_path("/.hidden")
            self.assertTrue(res.is_file)

    def test_resolve_traversal_denied(self):
        with tempfile.TemporaryDirectory() as td:
            sr = SecureRoot(td)
            with self.assertRaises(PathPolicyError):
                sr.resolve_path("/../etc/passwd")


class TestResolvedResource(unittest.TestCase):
    def test_file_kind(self):
        with tempfile.TemporaryDirectory() as td:
            with open(os.path.join(td, "f.txt"), "w") as f:
                f.write("x")
            sr = SecureRoot(td)
            res = sr.resolve_path("/f.txt")
            self.assertEqual(res.kind, "file")

    def test_directory_kind(self):
        with tempfile.TemporaryDirectory() as td:
            sr = SecureRoot(td)
            res = sr.resolve_path("/")
            self.assertEqual(res.kind, "directory")

    def test_not_found_kind(self):
        with tempfile.TemporaryDirectory() as td:
            sr = SecureRoot(td)
            res = sr.resolve_path("/nope")
            self.assertEqual(res.kind, "not_found")

    def test_denied_kind(self):
        with tempfile.TemporaryDirectory() as td:
            target = os.path.join(td, "target.txt")
            link = os.path.join(td, "link.txt")
            with open(target, "w") as f:
                f.write("x")
            os.symlink(target, link)
            sr = SecureRoot(td)
            res = sr.resolve_path("/link.txt")
            self.assertEqual(res.kind, "denied")

    def test_file_accessor_on_file(self):
        with tempfile.TemporaryDirectory() as td:
            with open(os.path.join(td, "f.txt"), "w") as f:
                f.write("hello")
            sr = SecureRoot(td)
            res = sr.resolve_path("/f.txt")
            f = res.file
            self.assertIsInstance(f, ResolvedFile)
            self.assertEqual(f.length, 5)

    def test_file_accessor_on_directory_raises(self):
        with tempfile.TemporaryDirectory() as td:
            sr = SecureRoot(td)
            res = sr.resolve_path("/")
            with self.assertRaises(EggserveError):
                _ = res.file

    def test_directory_accessor_on_directory(self):
        with tempfile.TemporaryDirectory() as td:
            sr = SecureRoot(td)
            res = sr.resolve_path("/")
            d = res.directory
            self.assertIsInstance(d, ResolvedDirectory)

    def test_directory_accessor_on_file_raises(self):
        with tempfile.TemporaryDirectory() as td:
            with open(os.path.join(td, "f.txt"), "w") as f:
                f.write("x")
            sr = SecureRoot(td)
            res = sr.resolve_path("/f.txt")
            with self.assertRaises(EggserveError):
                _ = res.directory

    def test_denied_reason_on_denied(self):
        with tempfile.TemporaryDirectory() as td:
            target = os.path.join(td, "target.txt")
            link = os.path.join(td, "link.txt")
            with open(target, "w") as f:
                f.write("x")
            os.symlink(target, link)
            sr = SecureRoot(td)
            res = sr.resolve_path("/link.txt")
            msg, code = res.denied_reason
            self.assertEqual(code, "symlink_denied")
            self.assertIn("symlink", msg)

    def test_denied_reason_on_not_denied_raises(self):
        with tempfile.TemporaryDirectory() as td:
            sr = SecureRoot(td)
            res = sr.resolve_path("/nonexistent")
            with self.assertRaises(EggserveError):
                _ = res.denied_reason

    def test_repr(self):
        with tempfile.TemporaryDirectory() as td:
            with open(os.path.join(td, "f.txt"), "w") as f:
                f.write("x")
            sr = SecureRoot(td)
            res = sr.resolve_path("/f.txt")
            r = repr(res)
            self.assertIn("ResolvedResource", r)
            self.assertIn("file", r)

    def test_frozen(self):
        with tempfile.TemporaryDirectory() as td:
            with open(os.path.join(td, "f.txt"), "w") as f:
                f.write("x")
            sr = SecureRoot(td)
            res = sr.resolve_path("/f.txt")
            with self.assertRaises(AttributeError):
                res.kind = "directory"  # type: ignore[misc]


class TestResolvedFile(unittest.TestCase):
    def _make_file(self, content="hello world"):
        self._td = tempfile.mkdtemp()
        path = os.path.join(self._td, "test.txt")
        with open(path, "w") as f:
            f.write(content)
        sr = SecureRoot(self._td)
        res = sr.resolve_path("/test.txt")
        return res.file

    def tearDown(self):
        import shutil

        shutil.rmtree(self._td, ignore_errors=True)

    def test_length(self):
        f = self._make_file("hello world")
        self.assertEqual(f.length, 11)

    def test_content_type(self):
        f = self._make_file()
        self.assertIn("text/plain", f.content_type)

    def test_modified_is_float(self):
        f = self._make_file()
        m = f.modified
        self.assertIsInstance(m, float)
        self.assertGreater(m, 0)

    def test_safe_relative_components(self):
        f = self._make_file()
        comps = f.safe_relative_components
        self.assertEqual(comps, ["test.txt"])

    def test_plan_response_get(self):
        f = self._make_file()
        plan = f.plan_response("GET")
        self.assertEqual(plan.status, 200)
        self.assertEqual(plan.body_kind, "file_full")
        header_names = [h[0] for h in plan.headers]
        self.assertIn("content-length", header_names)
        self.assertIn("content-type", header_names)
        self.assertIn("etag", header_names)

    def test_plan_response_head(self):
        f = self._make_file()
        plan = f.plan_response("HEAD")
        self.assertEqual(plan.status, 200)
        self.assertEqual(plan.body_kind, "empty")

    def test_plan_conditional_304(self):
        f = self._make_file()
        etag = generate_etag(f)
        self.assertIsNotNone(etag)
        plan = f.plan_conditional_response(
            "GET", headers=[("if-none-match", etag)]
        )
        self.assertEqual(plan.status, 304)
        self.assertEqual(plan.body_kind, "empty")

    def test_plan_conditional_200_no_match(self):
        f = self._make_file()
        plan = f.plan_conditional_response(
            "GET", headers=[("if-none-match", "W/\"bogus\"")]
        )
        self.assertEqual(plan.status, 200)

    def test_repr(self):
        f = self._make_file()
        r = repr(f)
        self.assertIn("ResolvedFile", r)
        self.assertIn("length=11", r)

    def test_frozen(self):
        f = self._make_file()
        with self.assertRaises(AttributeError):
            f.content_type = "application/json"  # type: ignore[misc]

    def test_plan_range_206(self):
        f = self._make_file("x" * 100)
        plan = f.plan_conditional_response("GET", headers=[("range", "bytes=0-0")])
        self.assertEqual(plan.status, 206)
        self.assertEqual(plan.body_kind, "file_range")
        self.assertEqual(plan.range, (0, 0))
        headers = dict(plan.headers)
        self.assertEqual(headers.get("content-range"), "bytes 0-0/100")
        self.assertEqual(headers.get("content-length"), "1")
        self.assertEqual(headers.get("accept-ranges"), "bytes")
        self.assertIn("etag", headers)
        self.assertIn("last-modified", headers)

    def test_plan_range_open_ended(self):
        f = self._make_file("x" * 100)
        plan = f.plan_conditional_response("GET", headers=[("range", "bytes=50-")])
        self.assertEqual(plan.status, 206)
        self.assertEqual(plan.range, (50, 99))

    def test_plan_range_suffix(self):
        f = self._make_file("x" * 100)
        plan = f.plan_conditional_response("GET", headers=[("range", "bytes=-10")])
        self.assertEqual(plan.status, 206)
        self.assertEqual(plan.range, (90, 99))

    def test_plan_range_416(self):
        f = self._make_file("x" * 100)
        plan = f.plan_conditional_response("GET", headers=[("range", "bytes=200-300")])
        self.assertEqual(plan.status, 416)
        self.assertEqual(plan.body_kind, "empty")
        headers = dict(plan.headers)
        self.assertEqual(headers.get("content-length"), "0")
        self.assertEqual(headers.get("accept-ranges"), "bytes")
        self.assertEqual(headers.get("content-range"), "bytes */100")

    def test_plan_if_range_match(self):
        f = self._make_file("x" * 100)
        etag = generate_etag(f)
        plan = f.plan_conditional_response("GET", headers=[
            ("range", "bytes=0-0"),
            ("if-range", etag),
        ])
        self.assertEqual(plan.status, 206)

    def test_plan_if_range_mismatch(self):
        f = self._make_file("x" * 100)
        plan = f.plan_conditional_response("GET", headers=[
            ("range", "bytes=0-0"),
            ("if-range", "W/\"bogus\""),
        ])
        self.assertEqual(plan.status, 200)
        self.assertEqual(plan.body_kind, "file_full")

    def test_plan_head_range_empty_body(self):
        f = self._make_file("x" * 100)
        plan = f.plan_conditional_response("HEAD", headers=[("range", "bytes=0-49")])
        self.assertEqual(plan.status, 206)
        self.assertEqual(plan.body_kind, "empty")


class TestResolvedDirectory(unittest.TestCase):
    def test_list(self):
        with tempfile.TemporaryDirectory() as td:
            with open(os.path.join(td, "a.txt"), "w") as f:
                f.write("a")
            os.makedirs(os.path.join(td, "subdir"))
            sr = SecureRoot(td)
            res = sr.resolve_path("/")
            d = res.directory
            entries = d.list()
            names = [e[0] for e in entries]
            self.assertIn("a.txt", names)
            self.assertIn("subdir", names)

    def test_list_filters_dotfiles(self):
        with tempfile.TemporaryDirectory() as td:
            with open(os.path.join(td, ".hidden"), "w") as f:
                f.write("x")
            with open(os.path.join(td, "visible.txt"), "w") as f:
                f.write("v")
            sr = SecureRoot(td)
            res = sr.resolve_path("/")
            d = res.directory
            entries = d.list()
            names = [e[0] for e in entries]
            self.assertIn("visible.txt", names)
            self.assertNotIn(".hidden", names)

    def test_resolve_child(self):
        with tempfile.TemporaryDirectory() as td:
            with open(os.path.join(td, "child.txt"), "w") as f:
                f.write("child")
            sr = SecureRoot(td)
            res = sr.resolve_path("/")
            d = res.directory
            child = d.resolve_child("child.txt")
            self.assertTrue(child.is_file)
            f = child.file
            self.assertEqual(f.length, 5)

    def test_resolve_child_not_found(self):
        with tempfile.TemporaryDirectory() as td:
            sr = SecureRoot(td)
            res = sr.resolve_path("/")
            d = res.directory
            child = d.resolve_child("nonexistent")
            self.assertTrue(child.is_not_found)

    def test_safe_relative_components(self):
        with tempfile.TemporaryDirectory() as td:
            os.makedirs(os.path.join(td, "a", "b"))
            sr = SecureRoot(td)
            res = sr.resolve_path("/a")
            d = res.directory
            self.assertEqual(d.safe_relative_components, ["a"])

    def test_repr(self):
        with tempfile.TemporaryDirectory() as td:
            sr = SecureRoot(td)
            res = sr.resolve_path("/")
            d = res.directory
            r = repr(d)
            self.assertIn("ResolvedDirectory", r)

    def test_frozen(self):
        with tempfile.TemporaryDirectory() as td:
            sr = SecureRoot(td)
            res = sr.resolve_path("/")
            d = res.directory
            with self.assertRaises(AttributeError):
                d.components = []  # type: ignore[misc]


class TestGenerateEtag(unittest.TestCase):
    def test_returns_string(self):
        with tempfile.TemporaryDirectory() as td:
            with open(os.path.join(td, "f.txt"), "w") as f:
                f.write("data")
            sr = SecureRoot(td)
            res = sr.resolve_path("/f.txt")
            etag = generate_etag(res.file)
            self.assertIsInstance(etag, str)
            self.assertTrue(etag.startswith("W/"))

    def test_same_content_same_etag(self):
        with tempfile.TemporaryDirectory() as td:
            with open(os.path.join(td, "a.txt"), "w") as f:
                f.write("same")
            with open(os.path.join(td, "b.txt"), "w") as f:
                f.write("same")
            sr = SecureRoot(td)
            ra = sr.resolve_path("/a.txt")
            rb = sr.resolve_path("/b.txt")
            ea = generate_etag(ra.file)
            eb = generate_etag(rb.file)
            self.assertEqual(ea, eb)


class TestExceptionHierarchy(unittest.TestCase):
    def test_path_policy_error_is_eggserve_error(self):
        self.assertTrue(issubclass(PathPolicyError, EggserveError))

    def test_request_target_error_is_eggserve_error(self):
        self.assertTrue(issubclass(RequestTargetError, EggserveError))

    def test_secure_root_error_is_eggserve_error(self):
        self.assertTrue(issubclass(SecureRootError, EggserveError))

    def test_request_validation_error_is_eggserve_error(self):
        self.assertTrue(issubclass(RequestValidationError, EggserveError))

    def test_body_source_error_is_eggserve_error(self):
        self.assertTrue(issubclass(BodySourceError, EggserveError))

    def test_eggserve_error_is_exception(self):
        self.assertTrue(issubclass(EggserveError, Exception))


class TestImmutability(unittest.TestCase):
    def test_path_policy_immutable(self):
        pp = PathPolicy()
        with self.assertRaises(AttributeError):
            pp.allow_dotfiles = True  # type: ignore[misc]
        with self.assertRaises(AttributeError):
            pp.reject_backslash = False  # type: ignore[misc]

    def test_static_policy_immutable(self):
        sp = StaticPolicy()
        with self.assertRaises(AttributeError):
            sp.directory_listing = True  # type: ignore[misc]
        with self.assertRaises(AttributeError):
            sp.follow_symlinks = True  # type: ignore[misc]
        with self.assertRaises(AttributeError):
            sp.allow_dotfiles = True  # type: ignore[misc]


class TestPolicyPreservation(unittest.TestCase):
    def test_request_target_dotfile_resolves_with_matching_policy(self):
        with tempfile.TemporaryDirectory() as td:
            with open(os.path.join(td, ".env"), "w") as f:
                f.write("secret")
            pp = PathPolicy(allow_dotfiles=True)
            sp = StaticPolicy(allow_dotfiles=True)
            target = RequestTarget.parse("/.env", pp)
            root = SecureRoot(td, policy=sp)
            res = root.resolve(target)
            self.assertTrue(res.is_file)

    def test_resolve_path_with_path_policy(self):
        with tempfile.TemporaryDirectory() as td:
            with open(os.path.join(td, ".env"), "w") as f:
                f.write("secret")
            sp = StaticPolicy(allow_dotfiles=True)
            root = SecureRoot(td, policy=sp)
            pp = PathPolicy(allow_dotfiles=True)
            res = root.resolve_path("/.env", path_policy=pp)
            self.assertTrue(res.is_file)

    def test_resolve_path_without_dotfile_policy_rejects(self):
        with tempfile.TemporaryDirectory() as td:
            with open(os.path.join(td, ".env"), "w") as f:
                f.write("secret")
            sr = SecureRoot(td)
            with self.assertRaises(PathPolicyError):
                sr.resolve_path("/.env")

    def test_directory_list_preserves_dotfile_policy(self):
        with tempfile.TemporaryDirectory() as td:
            with open(os.path.join(td, ".hidden"), "w") as f:
                f.write("x")
            with open(os.path.join(td, "visible.txt"), "w") as f:
                f.write("v")
            sp = StaticPolicy(allow_dotfiles=True)
            root = SecureRoot(td, policy=sp)
            res = root.resolve_path("/")
            d = res.directory
            entries = d.list()
            names = [e[0] for e in entries]
            self.assertIn(".hidden", names)
            self.assertIn("visible.txt", names)

    def test_request_target_path_policy_does_not_override_static_policy(self):
        with tempfile.TemporaryDirectory() as td:
            with open(os.path.join(td, ".hidden"), "w") as f:
                f.write("secret")
            target = RequestTarget.parse(
                "/.hidden", PathPolicy(allow_dotfiles=True)
            )
            root = SecureRoot(td, StaticPolicy(allow_dotfiles=False))
            res = root.resolve(target)
            self.assertTrue(res.is_denied)
            self.assertEqual(res.denied_reason[1], "dotfile_denied")

    def test_resolve_path_explicit_path_policy_does_not_bypass_static_policy(self):
        with tempfile.TemporaryDirectory() as td:
            with open(os.path.join(td, ".hidden"), "w") as f:
                f.write("secret")
            root = SecureRoot(td, StaticPolicy(allow_dotfiles=False))
            res = root.resolve_path("/.hidden", PathPolicy(allow_dotfiles=True))
            self.assertTrue(res.is_denied)
            self.assertEqual(res.denied_reason[1], "dotfile_denied")

    def test_directory_resolve_child_with_dotfile_policy(self):
        with tempfile.TemporaryDirectory() as td:
            with open(os.path.join(td, ".env"), "w") as f:
                f.write("secret")
            sp = StaticPolicy(allow_dotfiles=True)
            root = SecureRoot(td, policy=sp)
            res = root.resolve_path("/")
            d = res.directory
            child = d.resolve_child(".env")
            self.assertTrue(child.is_file)

    def test_resolved_directory_list_includes_both_dotfile_and_visible(self):
        with tempfile.TemporaryDirectory() as td:
            with open(os.path.join(td, ".hidden"), "w") as f:
                f.write("x")
            with open(os.path.join(td, "visible.txt"), "w") as f:
                f.write("v")
            root = SecureRoot(td, StaticPolicy(allow_dotfiles=True))
            directory = root.resolve_path("/").directory
            names = [name for name, _is_dir in directory.list()]
            self.assertIn(".hidden", names)
            self.assertIn("visible.txt", names)


class TestValidateMethodExtended(unittest.TestCase):
    def test_get_returns_get(self):
        result = validate_method("GET")
        self.assertEqual(result, "GET")

    def test_head_returns_head(self):
        result = validate_method("HEAD")
        self.assertEqual(result, "HEAD")

    def test_options_rejected(self):
        with self.assertRaises(RequestValidationError):
            validate_method("OPTIONS")

    def test_delete_rejected(self):
        with self.assertRaises(RequestValidationError):
            validate_method("DELETE")

    def test_patch_rejected(self):
        with self.assertRaises(RequestValidationError):
            validate_method("PATCH")


class TestValidateRequestBodyExtended(unittest.TestCase):
    def test_empty_transfer_encoding_allowed(self):
        validate_request_body(transfer_encoding="")

    def test_whitespace_transfer_encoding_allowed(self):
        validate_request_body(transfer_encoding="  ")

    def test_conflicting_headers_rejected(self):
        with self.assertRaises(RequestValidationError):
            validate_request_body(content_length="0", transfer_encoding="chunked")

    def test_overflow_content_length_rejected(self):
        with self.assertRaises(RequestValidationError):
            validate_request_body(content_length="99999999999999999999")

    def test_negative_content_length_rejected(self):
        with self.assertRaises(RequestValidationError):
            validate_request_body(content_length="-1")

    def test_empty_content_length_rejected(self):
        with self.assertRaises(RequestValidationError):
            validate_request_body(content_length="")

    def test_whitespace_content_length_rejected(self):
        with self.assertRaises(RequestValidationError):
            validate_request_body(content_length="  ")

    def test_max_body_bytes_respected(self):
        validate_request_body(content_length="100", max_body_bytes=100)
        with self.assertRaises(RequestValidationError):
            validate_request_body(content_length="101", max_body_bytes=100)


class TestValidateRequestTargetExtended(unittest.TestCase):
    def test_root_valid(self):
        validate_request_target("/")

    def test_deep_path_valid(self):
        validate_request_target("/a/b/c/d.txt")

    def test_empty_rejected(self):
        with self.assertRaises(RequestValidationError):
            validate_request_target("")

    def test_no_leading_slash_rejected(self):
        with self.assertRaises(RequestValidationError):
            validate_request_target("foo")

    def test_asterisk_rejected(self):
        with self.assertRaises(RequestValidationError):
            validate_request_target("*")

    def test_whitespace_rejected(self):
        with self.assertRaises(RequestValidationError):
            validate_request_target("/foo bar")


class TestResponsePlanExtended(unittest.TestCase):
    def _make_file(self, content="x" * 100):
        self._td = tempfile.mkdtemp()
        path = os.path.join(self._td, "test.txt")
        with open(path, "w") as f:
            f.write(content)
        sr = SecureRoot(self._td)
        res = sr.resolve_path("/test.txt")
        return res.file

    def tearDown(self):
        import shutil
        shutil.rmtree(self._td, ignore_errors=True)

    def test_plan_response_status_code(self):
        f = self._make_file()
        plan = f.plan_response("GET")
        self.assertEqual(plan.status, 200)

    def test_plan_response_headers_include_content_type(self):
        f = self._make_file()
        plan = f.plan_response("GET")
        headers = dict(plan.headers)
        self.assertIn("content-type", headers)
        self.assertIn("text/plain", headers["content-type"])

    def test_plan_response_headers_include_content_length(self):
        f = self._make_file("hello")
        plan = f.plan_response("GET")
        headers = dict(plan.headers)
        self.assertEqual(headers.get("content-length"), "5")

    def test_plan_response_headers_include_etag(self):
        f = self._make_file()
        plan = f.plan_response("GET")
        headers = dict(plan.headers)
        self.assertIn("etag", headers)
        self.assertTrue(headers["etag"].startswith("W/\""))

    def test_plan_response_headers_include_accept_ranges(self):
        f = self._make_file()
        plan = f.plan_response("GET")
        headers = dict(plan.headers)
        self.assertEqual(headers.get("accept-ranges"), "bytes")

    def test_plan_response_headers_include_nosniff(self):
        f = self._make_file()
        plan = f.plan_response("GET")
        headers = dict(plan.headers)
        self.assertEqual(headers.get("x-content-type-options"), "nosniff")

    def test_plan_head_body_kind_empty(self):
        f = self._make_file()
        plan = f.plan_response("HEAD")
        self.assertEqual(plan.body_kind, "empty")

    def test_plan_head_status_matches_get(self):
        f = self._make_file()
        get_plan = f.plan_response("GET")
        head_plan = f.plan_response("HEAD")
        self.assertEqual(get_plan.status, head_plan.status)

    def test_plan_conditional_not_modified_304(self):
        f = self._make_file()
        etag = generate_etag(f)
        plan = f.plan_conditional_response("GET", headers=[("if-none-match", etag)])
        self.assertEqual(plan.status, 304)
        self.assertEqual(plan.body_kind, "empty")
        headers = dict(plan.headers)
        self.assertIn("etag", headers)

    def test_plan_conditional_modified_200(self):
        f = self._make_file()
        plan = f.plan_conditional_response("GET", headers=[("if-none-match", "W/\"bogus\"")])
        self.assertEqual(plan.status, 200)
        self.assertEqual(plan.body_kind, "file_full")

    def test_plan_range_single_byte(self):
        f = self._make_file("x" * 100)
        plan = f.plan_conditional_response("GET", headers=[("range", "bytes=0-0")])
        self.assertEqual(plan.status, 206)
        self.assertEqual(plan.body_kind, "file_range")
        self.assertEqual(plan.range, (0, 0))
        headers = dict(plan.headers)
        self.assertEqual(headers.get("content-range"), "bytes 0-0/100")
        self.assertEqual(headers.get("content-length"), "1")

    def test_plan_range_open_ended(self):
        f = self._make_file("x" * 100)
        plan = f.plan_conditional_response("GET", headers=[("range", "bytes=50-")])
        self.assertEqual(plan.status, 206)
        self.assertEqual(plan.range, (50, 99))

    def test_plan_range_suffix(self):
        f = self._make_file("x" * 100)
        plan = f.plan_conditional_response("GET", headers=[("range", "bytes=-10")])
        self.assertEqual(plan.status, 206)
        self.assertEqual(plan.range, (90, 99))

    def test_plan_range_suffix_exceeds_file(self):
        f = self._make_file("x" * 100)
        plan = f.plan_conditional_response("GET", headers=[("range", "bytes=-200")])
        self.assertEqual(plan.status, 206)
        self.assertEqual(plan.range, (0, 99))

    def test_plan_range_416(self):
        f = self._make_file("x" * 100)
        plan = f.plan_conditional_response("GET", headers=[("range", "bytes=200-300")])
        self.assertEqual(plan.status, 416)
        self.assertEqual(plan.body_kind, "empty")
        headers = dict(plan.headers)
        self.assertEqual(headers.get("content-range"), "bytes */100")

    def test_plan_range_inverted_returns_416(self):
        f = self._make_file("x" * 100)
        plan = f.plan_conditional_response("GET", headers=[("range", "bytes=50-10")])
        self.assertEqual(plan.status, 416)

    def test_plan_if_range_matching_etag_206(self):
        f = self._make_file("x" * 100)
        etag = generate_etag(f)
        plan = f.plan_conditional_response("GET", headers=[
            ("range", "bytes=0-0"),
            ("if-range", etag),
        ])
        self.assertEqual(plan.status, 206)

    def test_plan_if_range_mismatched_etag_200(self):
        f = self._make_file("x" * 100)
        plan = f.plan_conditional_response("GET", headers=[
            ("range", "bytes=0-0"),
            ("if-range", "W/\"bogus\""),
        ])
        self.assertEqual(plan.status, 200)
        self.assertEqual(plan.body_kind, "file_full")

    def test_plan_head_range_206_empty_body(self):
        f = self._make_file("x" * 100)
        plan = f.plan_conditional_response("HEAD", headers=[("range", "bytes=0-49")])
        self.assertEqual(plan.status, 206)
        self.assertEqual(plan.body_kind, "empty")
        headers = dict(plan.headers)
        self.assertEqual(headers.get("content-length"), "50")


class TestBodySource(unittest.TestCase):
    def _make_resolved_file(self, content=b"hello world"):
        self._td = tempfile.mkdtemp()
        path = os.path.join(self._td, "test.txt")
        with open(path, "wb") as f:
            f.write(content)
        root = SecureRoot(self._td)
        res = root.resolve_path("/test.txt")
        self.assertIsInstance(res, ResolvedResource)
        self.assertTrue(res.is_file)
        return res.file

    def tearDown(self):
        import shutil
        shutil.rmtree(self._td, ignore_errors=True)

    def test_body_for_plan_file_full(self):
        f = self._make_resolved_file(b"hello world")
        plan = f.plan_response("GET")
        self.assertEqual(plan.status, 200)
        self.assertEqual(plan.body_kind, "file_full")
        body = f.body_for_plan(plan)
        self.assertIsInstance(body, BodySource)
        self.assertEqual(body.kind, "file_full")
        self.assertEqual(body.read_all(), b"hello world")

    def test_body_for_plan_file_range(self):
        f = self._make_resolved_file(b"hello world")
        plan = f.plan_conditional_response("GET", headers=[("range", "bytes=0-4")])
        self.assertEqual(plan.status, 206)
        self.assertEqual(plan.body_kind, "file_range")
        body = f.body_for_plan(plan)
        self.assertEqual(body.kind, "file_range")
        self.assertEqual(body.range, (0, 4))
        self.assertEqual(body.read_all(), b"hello")

    def test_body_for_plan_read_range(self):
        f = self._make_resolved_file(b"hello world")
        plan = f.plan_response("GET")
        body = f.body_for_plan(plan)
        self.assertEqual(body.read_range(6, 10), b"world")

    def test_body_for_plan_consumes_file(self):
        f = self._make_resolved_file(b"hello world")
        plan = f.plan_response("GET")
        body = f.body_for_plan(plan)
        self.assertIsNotNone(body)
        with self.assertRaises(EggserveError):
            f.body_for_plan(plan)

    def test_body_source_length(self):
        f = self._make_resolved_file(b"hello world")
        plan = f.plan_response("GET")
        body = f.body_for_plan(plan)
        self.assertEqual(body.length, 11)

    def test_body_source_repr(self):
        f = self._make_resolved_file(b"hello world")
        plan = f.plan_response("GET")
        body = f.body_for_plan(plan)
        r = repr(body)
        self.assertIn("BodySource", r)
        self.assertIn("FileFull", r)

    def test_response_plan_properties(self):
        f = self._make_resolved_file(b"hello world")
        plan = f.plan_response("GET")
        self.assertIsInstance(plan, ResponsePlan)
        self.assertEqual(plan.status, 200)
        self.assertEqual(plan.body_kind, "file_full")
        self.assertIsNone(plan.range)
        self.assertIsInstance(plan.headers, list)

    def test_response_plan_range(self):
        f = self._make_resolved_file(b"hello world")
        plan = f.plan_conditional_response("GET", headers=[("range", "bytes=0-4")])
        self.assertEqual(plan.status, 206)
        self.assertEqual(plan.body_kind, "file_range")
        self.assertEqual(plan.range, (0, 4))


if __name__ == "__main__":
    unittest.main()

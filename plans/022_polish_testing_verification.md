# Plan 022: Primitive polish, testing, and verification pass

## Status

Planned. This is a small follow-up pass after Plan 021. It should not introduce new architecture. Its purpose is to verify that the policy-preservation fixes are directly tested and that documentation no longer overstates current Python primitive capabilities.

## Objective

Close the remaining verification gap around the primitive API by adding targeted regression tests and a short documentation audit. The implementation appears to have fixed the major Plan 021 issues, but some of the exact regressions should be protected by explicit tests rather than inferred from adjacent coverage.

## Scope

In scope:

- Add targeted Rust and Python regression tests for policy preservation.
- Add targeted Python tests for `resolve_path(..., path_policy=...)` semantics.
- Add targeted Python tests for `ResolvedDirectory` preserving static policy.
- Verify response-planning range headers from Python and Rust.
- Tighten docs around `validate_request_target()` and Python file-streaming limitations.
- Run full validation commands and record results in the commit message or a short verification note.

Out of scope:

- ASGI/WSGI adapters.
- Python request callback server.
- New framework abstractions.
- New streaming API.
- Windows reparse-point hardening.
- Broad HTTP compliance expansion beyond assertions for already-implemented range/conditional behavior.

## Current state to verify

Recent code changes appear to have addressed the major issues:

- `ResolvedFile::from_parts` is no longer public.
- `PyRequestTarget` stores a `ConfinedPath`.
- `SecureRoot.resolve(target)` uses the stored confined path.
- `SecureRoot.resolve_path(raw_path, path_policy=...)` honors the supplied path policy.
- `PyResolvedDirectory` stores and reuses the originating static policy.
- Planner responses now include stronger range metadata.

This pass should convert those observations into explicit regression tests and documentation checks.

## Implementation steps

### 1. Add Python regression tests for `RequestTarget` policy preservation

Update `crates/eggserve-python/python/eggserve/test_primitives.py`.

Add a test with this shape:

```python
def test_request_target_custom_path_policy_survives_resolve(self):
    with tempfile.TemporaryDirectory() as td:
        with open(os.path.join(td, ".hidden"), "w") as f:
            f.write("secret")
        target = RequestTarget.parse("/.hidden", PathPolicy(allow_dotfiles=True))
        root = SecureRoot(td, StaticPolicy(allow_dotfiles=True))
        res = root.resolve(target)
        self.assertTrue(res.is_file)
```

This should fail on the old implementation where `resolve()` reparsed with `PathPolicy::default()`.

Also add the negative paired case:

```python
def test_request_target_path_policy_does_not_override_static_policy(self):
    with tempfile.TemporaryDirectory() as td:
        with open(os.path.join(td, ".hidden"), "w") as f:
            f.write("secret")
        target = RequestTarget.parse("/.hidden", PathPolicy(allow_dotfiles=True))
        root = SecureRoot(td, StaticPolicy(allow_dotfiles=False))
        res = root.resolve(target)
        self.assertTrue(res.is_denied)
        self.assertEqual(res.denied_reason[1], "dotfile_denied")
```

This proves path policy permits parsing, but static policy still controls serving.

### 2. Add Python regression tests for `resolve_path(..., path_policy=...)`

Add tests proving the argument is not ignored:

```python
def test_resolve_path_honors_explicit_path_policy_allow_dotfiles(self):
    with tempfile.TemporaryDirectory() as td:
        with open(os.path.join(td, ".hidden"), "w") as f:
            f.write("secret")
        root = SecureRoot(td, StaticPolicy(allow_dotfiles=True))
        res = root.resolve_path("/.hidden", PathPolicy(allow_dotfiles=True))
        self.assertTrue(res.is_file)
```

Add a static-policy denial pair:

```python
def test_resolve_path_explicit_path_policy_does_not_bypass_static_policy(self):
    with tempfile.TemporaryDirectory() as td:
        with open(os.path.join(td, ".hidden"), "w") as f:
            f.write("secret")
        root = SecureRoot(td, StaticPolicy(allow_dotfiles=False))
        res = root.resolve_path("/.hidden", PathPolicy(allow_dotfiles=True))
        self.assertTrue(res.is_denied)
        self.assertEqual(res.denied_reason[1], "dotfile_denied")
```

Add a backslash-policy test if platform-neutral behavior allows it. If behavior is ambiguous across platforms, document and skip platform-specific cases.

### 3. Add Python regression tests for directory static-policy preservation

Add tests proving `ResolvedDirectory` does not fall back to safe defaults:

```python
def test_resolved_directory_list_preserves_allow_dotfiles_policy(self):
    with tempfile.TemporaryDirectory() as td:
        with open(os.path.join(td, ".hidden"), "w") as f:
            f.write("secret")
        with open(os.path.join(td, "visible.txt"), "w") as f:
            f.write("visible")
        root = SecureRoot(td, StaticPolicy(allow_dotfiles=True))
        directory = root.resolve_path("/").directory
        names = [name for name, _is_dir in directory.list()]
        self.assertIn(".hidden", names)
        self.assertIn("visible.txt", names)
```

Add child-resolution preservation:

```python
def test_resolved_directory_resolve_child_preserves_allow_dotfiles_policy(self):
    with tempfile.TemporaryDirectory() as td:
        with open(os.path.join(td, ".hidden"), "w") as f:
            f.write("secret")
        root = SecureRoot(td, StaticPolicy(allow_dotfiles=True))
        directory = root.resolve_path("/").directory
        child = directory.resolve_child(".hidden")
        self.assertTrue(child.is_file)
```

If follow-symlink preservation is straightforward on Unix, add a `@unittest.skipUnless(hasattr(os, "symlink"), ...)` test proving `StaticPolicy(follow_symlinks=True)` survives directory `resolve_child()` for an internal symlink.

### 4. Add Rust regression tests where appropriate

If not already covered, add Rust tests for:

- `ConfinedPath::path_policy()` returns the policy used during parsing.
- `ResolvedFile::from_parts` is not public. This may be documented rather than tested; do not add trybuild unless already used.
- Range planner `206` includes `content-type`, `accept-ranges`, `etag`, and `last-modified` when available.
- Range planner `416` includes `content-length: 0`, `accept-ranges: bytes`, and `content-range: bytes */len`.

Prefer normal unit tests in `crates/eggserve-core/src/primitives/planner.rs` and `crates/eggserve-core/src/path/mod.rs`.

### 5. Strengthen Python range-header assertions

Current Python range tests validate status/body/range. Add header assertions:

```python
headers = dict(plan.headers)
self.assertEqual(headers.get("content-type"), "text/plain; charset=utf-8")
self.assertEqual(headers.get("accept-ranges"), "bytes")
self.assertIn("etag", headers)
self.assertIn("last-modified", headers)
self.assertEqual(headers.get("content-range"), "bytes 0-0/100")
```

For `416`:

```python
headers = dict(plan.headers)
self.assertEqual(headers.get("content-length"), "0")
self.assertEqual(headers.get("accept-ranges"), "bytes")
self.assertEqual(headers.get("content-range"), "bytes */100")
```

### 6. Documentation polish

Review and update:

- `README.md`
- `docs/python-api.md`
- `docs/public-api-boundary.md`
- `docs/secure-root.md`
- `docs/http-response-planning.md`
- `docs/extension-contract.md`
- `docs/invariants.md`

Required statements:

- `validate_request_target()` is only a coarse origin-form check; `RequestTarget.parse()` / `ConfinedPath` is the path-security boundary.
- Python `ResolvedFile` currently supports metadata and response planning. It does not yet expose a resolver-opened streaming handle.
- Python examples must not imply production-safe byte serving through a reopened path.
- `ResolvedDirectory.list()` is a policy-filtered primitive listing, not the same thing as enabling HTTP directory listing exposure.
- The path policy controls request-target acceptance; the static policy controls whether a resolved resource may be served.

### 7. Optional verification note

If the repo has a convention for verification notes, add a small `docs/verification.md` update or a short section in the commit message listing the commands run. Do not create a new permanent doc solely for one command run unless this repo already does that.

## Validation commands

Run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-bin --features tls --all-targets -- -D warnings
cargo test -p eggserve-bin --features tls
PYTHONPATH=crates/eggserve-python/python python -m unittest eggserve.test_server -v
PYTHONPATH=crates/eggserve-python/python python -m unittest eggserve.test_primitives -v
```

If maturin is available:

```sh
cd crates/eggserve-python
maturin build --release -o dist
python -m pip install --force-reinstall dist/*.whl
python -m eggserve --help
python - <<'PY'
from eggserve import NATIVE_AVAILABLE
assert NATIVE_AVAILABLE
from eggserve import SecureRoot, RequestTarget, PathPolicy, StaticPolicy
print(SecureRoot, RequestTarget, PathPolicy, StaticPolicy)
PY
```

If available:

```sh
cargo audit
cargo deny check
```

## Completion criteria

This pass is complete when:

- The exact old Python policy-loss bug is covered by a regression test.
- `resolve_path(..., path_policy=...)` has a regression test proving the argument is honored.
- `ResolvedDirectory.list()` and `resolve_child()` have tests proving static policy preservation.
- Python range tests assert important response headers, not only status/body kind.
- Rust planner tests assert `206` and `416` header completeness.
- Docs clearly distinguish coarse request-target validation from full path confinement.
- Docs accurately describe Python `ResolvedFile` as metadata/response-planning only until a future streaming primitive lands.

# Plan 160 — Bug Report Closure (2026-09-03 Interrogation)

## Status

**COMPLETE — 2026-09-03.**

## Scope

Resolve all actionable findings B1–B8 in the temporary `bugs.md`
interrogation dated 2026-09-03 (commit `211db3a`; routine suite green:
1513 passed/2 ignored workspace, 138 TLS, 8 doc tests, conformance 51).
Keep changes minimal, preserve existing contracts, no feature additions or
optimizations. Delete `bugs.md`, verify locally, commit and push on `main`.

## Fixes

1. **B1 — `FileRange` invariant** (`primitives/response.rs`, callers):
   private fields + `pub start()/end_inclusive()` getters; construction only
   via `try_new` (pub) / `new` (pub(crate), internal callers already guard).
   `len()` keeps `expect` but is now unreachable from external input;
   document invariant. Update field reads in `body.rs`, `canonical.rs`,
   `planner.rs` (+ tests/proptests), `tests/common/mod.rs`,
   `tests/corpus_replay.rs`, `eggserve-python/src/lib.rs:264,287-288`,
   `server.rs:1175`.

2. **B2 — TLS log `unwrap`** (`eggserve-bin/src/lib.rs:146-155`): replace
   `if tls_config.is_some()` + `args.tls_cert.as_ref().unwrap()` with
   `if let (Some(_), Some(cert)) = (&tls_config, &args.tls_cert)`.

3. **B3 — control-char parity** (`fs/mod.rs:113-159`): add
   `is_ascii_control` → `ControlCharacter` rejection in
   `validate_child_component_with_policy`, mirroring
   `path/components.rs:15-17`; add unit test.

4. **B4 — non-UTF-8 listing** (`fs/unix.rs:201-242`): use raw `&CStr`
   `entry.file_name()` for `.`/`..`/dotfile checks and the `statat` lookup;
   lossy-convert only for display/sort/push. Fallback path
   (`fs/mod.rs:534-572`) already stats via `entry.path()`, no change.

5. **B5 — empty component parity** (`path/components.rs:6-59,140-144`):
   reject empty at parse level (`PathRejection::Empty`) for parity with
   child layer; fix `reject_empty_component` test to assert the rejection.

6. **B6 — `If-Range` strong comparison** (`primitives/planner.rs:315-354`):
   use `current_etag`: strong `If-Range` (`"..."`) strong-compares against a
   strong current tag (match → `NotModified`, i.e. range allowed); weak
   (`W/...`) never authorizes. Update
   `evaluate_if_range_etags_never_authorize_ranges_for_weak_metadata` and add
   strong-match/mismatch cases. No behavior change today (emitted tags are
   weak).

7. **B7 — duplicate `Content-Length` disagreement**
   (`server/connection.rs:794-815`): collect CL values; disagreeing values →
   400 conflicting (smuggling vector), agreeing duplicates → 400 duplicate
   (preserve current safe default); narrow doc comment; add unit tests.

8. **B8 — fallback listing `is_dir` follow** (`fs/mod.rs:557-563`): stop
   following symlinks for display (`let is_dir = meta.is_dir()` on the
   `symlink_metadata`), removing the pathname TOCTOU and aligning with the
   fd-relative backend (links report non-dir). Reachable only under
   `SymlinkPolicy::Follow`. Comment documents follow-mode-only scope.

Intentionally not done: optimizations #1–#6 (allocation/log/clone/sort
tradeoffs, deferred per report); explicit non-findings (verified correct).

## Validation

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --lib --bins --tests -- -D warnings`
- `cargo test --workspace`
- `cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings`
- `cargo test -p eggserve-bin --features tls`
- `cargo test --doc -p eggserve-core`
- `python3 scripts/verify-conformance-matrix.py`
- `cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked`
- `cargo check -p eggserve-core --examples`
- dist builds (default + tls)

## Completion criteria

- [x] B1–B8 fixed as above, tests adjusted;
- [x] verification passes;
- [x] `bugs.md` deleted;
- [x] committed and pushed from `main`.

## Validation (2026-09-03, this machine)

- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --lib --bins --tests -- -D warnings`: clean.
- `cargo test --workspace`: **1518 passed, 2 ignored** (39 suites; +5 new
  regression tests vs the 1513 in the report).
- `cargo clippy -p eggserve-bin --features tls -- -D warnings`: clean.
- `cargo test -p eggserve-bin --features tls`: **138 passed** (7 suites).
- `cargo test --doc -p eggserve-core`: **8 passed**.
- `python3 scripts/verify-conformance-matrix.py`: 51 entries validated.
- `cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked`:
  clean (covers the `r.start()`/`r.end_inclusive()` getter migration).
- `cargo check -p eggserve-core --examples`: clean.
- `cargo build --profile dist` (default + `--features tls`): clean.

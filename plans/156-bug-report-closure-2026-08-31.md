# Plan 156 — Bug Report Closure (2026-08-31)

## Status

**COMPLETE — 2026-08-31.**

## Scope

Resolve every actionable bug and low-priority correctness/documentation gap in
the temporary `bugs.md` report dated 2026-08-31. Keep changes minimal and do
not add the listed performance optimizations or unrelated features.

## Planned changes

1. Make Cargo package verification derive core/bin versions and assert that the
   staged bin manifest uses the local registry dependency.
2. Preserve Unix root-descriptor clone failures as an explicit unavailable
   resource result, mapping them to a service error/500 and the low-level
   Rust/Python boundaries rather than 404; add Windows parity where the same
   handle duplication can fail.
3. Add Windows `WSAENFILE` classification, remove the misleading Unix `abs()`,
   simplify intentional weak-ETag `If-Range` handling, and add focused tests.
4. Document generic empty header values, malformed percent-component
   pass-through, unbounded timeout semantics, and the feature-gated extraction
   security boundary; align Python static-header validation with Rust.
5. Run focused and routine verification, delete `bugs.md`, review the final
   diff, commit on `main`, and push to `origin/main`.

## Dispositions

- The `normalize_metadata` HEAD behavior/documentation and connection-token
  stripping are already correct in `HEAD`; no duplicate implementation change
  is needed.
- `HeaderValue::new` intentionally permits empty values for generic HTTP
  headers, while static metadata validation rejects empty values. This is
  documented rather than changed.
- The absolute timeout ceiling and filesystem fallback allocation notes are
  intentional/optimization concerns; the timeout behavior is documented and
  no new policy ceiling is introduced.

## Validation

- `cargo fmt --all -- --check` — passed.
- `cargo clippy --workspace --lib --bins --tests -- -D warnings` — passed.
- `cargo test --workspace` — passed.
- TLS Clippy/tests for `eggserve-bin` — passed.
- `cargo test --doc -p eggserve-core` — passed.
- `cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked` — passed.
- Python wheel smoke/tests — 752 tests passed.
- Conformance matrix validation — 51 entries passed.
- Cargo package dry-run — core publish dry-run and staged bin published-graph build passed.

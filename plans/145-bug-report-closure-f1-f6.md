# Plan 145 — Bug Report Closure (F1–F6, 2026-08-25 Second Pass)

## Status

**COMPLETE — 2026-08-25.**

## Scope

Resolve every actionable finding in the temporary `bugs.md` report (dated
2026-08-25 against commit `85d8b05`, following Plan 144), keeping code changes
minimal and avoiding feature additions.

### Applied

1. **F1** — `path::components::validate_components` guards its conservative
   second percent-decode with `component.contains('%')`. The canonical decode
   in `ConfinedPath::parse` still happens once up front; the re-check only
   denies dot-segments after one more decode (e.g. a file literally named
   `%2e%2e`) and now allocates nothing for `%`-free components. Behavior is
   unchanged (pinned by the existing `components.rs` unit test and
   `http_wire_correctness.rs` / `fault_injection.rs` cases).
2. **F2** — `serve_connection_with_runtime_state` takes
   `Arc<RuntimeConfig>` instead of `&RuntimeConfig`, eliminating one deep
   struct clone plus an `Arc` allocation per connection; the per-exchange
   closure now clones only the caller's existing `Arc`. Signature change is
   permitted: the function lives in the experimental `server` module. Internal
   callers (`accept_loop_generic`, both TLS/plain sites) and the integration/
   unit tests updated.
3. **F3** — new `FileRange::try_new(start, end_inclusive) -> Option<Self>`
   gives embedders a fallible constructor; `len()` keeps its documented panic
   as an invariant for internally validated ranges (`new` docs now point to
   `try_new`). Unit tests cover accept/reject boundaries including
   `(0, u64::MAX - 1)` vs `(0, u64::MAX)`.
4. **F4** — Windows handle-relative opens (`open_directory_relative`,
   `open_file_relative`) build their `NtUnicodeString` through a shared
   `build_nt_unicode_string` helper that computes byte lengths with `usize`
   math and returns the new controlled error `WindowsFsError::NameTooLong`
   when the UTF-16 encoding exceeds NT's 16-bit length fields (64 KiB),
   instead of silently truncating via `as u16`. The helper documents the
   `length` vs `maximum_length` split around the NUL terminator. Unreachable
   under the current 8 KiB raw-path cap; this closes the latent defect if the
   cap ever moves.
5. **F6** — planner test helper `make_file_with_size` takes `usize` directly;
   the truncating `size as usize` cast is gone and no caller passed a value
   above literal sizes anyway.

### Intentionally unchanged

- **F5** — directory-buffer parser rejecting exact-fit `NextEntryOffset ==
  total_len`: left strict. The Win32 contract makes the final entry use
  `NextEntryOffset == 0`, so the kernel never produces this shape, and the
  loop head already terminates cleanly on `offset >= total_len`; the report
  itself defers any relaxation to Windows VM qualification of the two
  `#[ignore]`d parser tests (`windows.rs`, "Parse offset behavior differs from
  expected"). Changing untestable-here parser semantics before that
  qualification would be scope creep.

## Validation

- `cargo check --target x86_64-pc-windows-msvc -p eggserve-core`: clean
  (covers `fs/windows.rs`; test-profile cross-check blocked by pre-existing
  `aws-lc-sys` cross-compile limitation, unrelated).
- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --lib --bins --tests -- -D warnings`: clean.
- `cargo test --workspace`: all passed.
- TLS lint + `cargo test -p eggserve-bin --features tls`: clean.

## Completion criteria

- [x] all actionable findings fixed or dispositioned with rationale;
- [x] focused regression tests cover changed behavior;
- [x] required verification passes;
- [x] `bugs.md` deleted;
- [x] changes committed and pushed from `main`.

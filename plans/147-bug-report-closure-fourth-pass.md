# Plan 147 — Bug Report Closure (Fourth-Pass Audit, 2026-08-25 Baseline `a5d99ee`)

## Status

**COMPLETE — 2026-08-26.**

## Scope

Resolve every actionable finding in the temporary `bugs.md` fourth-pass audit
(baseline commit `a5d99ee`, following Plans 144–146), keeping code changes
minimal and avoiding feature additions.

### Applied

1. **BUG-1/BUG-2 (Windows dir-buffer parser)** — the advance step in
   `parse_directory_buffer` is factored into an `advance_offset` helper used by
   both advance sites (regular entries and `.`/`..` skip). The helper rejects a
   non-zero `NextEntryOffset` whose target would fall inside the current
   record's fixed header extent with `DirBufParseError::OffsetUnderflow`,
   making the previously unreachable variant real: a crafted backward/overlap
   chain is now cleanly rejected instead of being parsed as garbage. Strictly
   monotonic advances (`checked_add` of a non-zero delta) make cycles
   impossible by construction, so `OffsetLoop` stays reserved and its doc now
   says so honestly; `OffsetUnderflow`'s doc was corrected to describe the
   overlap condition actually enforced. Tests:
   - `parse_offset_underflow` now passes as written (delta `1` overlaps the
     current record → `OffsetUnderflow`); no ignore needed.
   - `parse_offset_loop` was based on a misunderstanding (`NextEntryOffset`
     is relative; `0` is the end-of-list marker, not "jump to absolute 0").
     It is rewritten un-ignored as `parse_zero_next_offset_terminates_chain`,
     asserting the correct delta semantics.
   - Added `parse_underflow_rejected_at_dot_skip_site` covering the second
     advance site.
   - The fuzz-harness allow-list is unchanged; every listed variant is now
     producible or documented as reserved.
2. **BUG-3 (`OwnedHandle::try_clone`)** — an invalid source handle now returns
   `Err(WindowsFsError::IoError(ERROR_INVALID_HANDLE))` (new local constant,
   Win32 code 6) instead of `Ok(INVALID_HANDLE_VALUE)`, so the `Result`
   contract is honest and callers cannot reach `handle_to_std_file`'s assert
   with a cloned-invalid handle. Test `owned_handle_invalid_try_clone` updated
   to expect the error. Sole production caller (`windows.rs` child-handle
   duplication) already operates on a just-opened valid handle and propagates
   errors via `?`, so behavior is unchanged there.
3. **BUG-4 (`validate_request_body` contract)** — docstring rewritten to state
   the actual guarantees: single-value format + limit validation only;
   duplicate `Content-Length` detection remains the caller's responsibility
   (the bundled server does it in `validate_body_framing`), and method
   semantics are not enforced here. Signature unchanged (no breaking API
   change); Python binding inherits the accurate Rust docs implicitly through
   unchanged behavior.
4. **BUG-5 (CLI `--flag=value`)** — long options now accept GNU-style attached
   values. Inside the parse loop (after `--` handling, so post-`--` tokens are
   never split) a `--name=value` token whose name is a known value-taking flag
   is pre-split into the flag plus its separate argument before matching;
   known boolean flags reject `--flag=value` with a precise
   "does not take a value" error; unknown flags keep the verbatim
   "unknown flag" error. Short flags are out of scope per the finding.
5. **BUG-6 (startup signal-loss race)** — dispositioned **not a bug** after
   empirical verification with tokio 1.52.3: the receiver returned by
   `broadcast::channel` exists from creation (`lib.rs:204`/`lib.rs:331`),
   which is before the signal task spawn and before `server.start().await`;
   `let mut signal_rx = shutdown_rx;` is a move/rebind, not a subscription
   point. A send during `start()` is buffered for the live receiver and
   returned immediately by the later `recv()` (verified with a scratch
   program reproducing the exact structure). Clarifying comments added at both
   channel-creation sites to prevent future regressions toward
   late-subscription patterns. (Capacity-1 overflow after two signals yields
   `Lagged`, whose `is_err()` path still performs the graceful shutdown.)

### Intentionally not done

- **NOTE-1 / NOTE-2** — informational robustness/diagnostics notes, not bugs;
  excluded to avoid scope creep in a bug-fix closure.
- **Optimizations O1–O3** — perf-only with zero correctness impact, same
  rationale as Plan 146's deferred items.
- **BUG-4 signature extension** — duplicate-CL parity would change the public
  primitive's signature (breaking); documentation route chosen per the report.

## Validation

- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --lib --bins --tests -- -D warnings`: clean.
- `cargo test --workspace`: 1475 passed, 2 ignored. The parser offset test
  from this report is active; the remaining ignores are unrelated Windows
  race/qualification tests.
- TLS lint + `cargo test -p eggserve-bin --features tls`: clean.
- Windows-only changes compile-checked via
  `cargo check -p eggserve-core --target x86_64-pc-windows-msvc` (parser and
  handle tests are cfg(windows)); runtime qualification remains gated on the
  manual platform-qualification workflow per repo policy.
- `cargo test --doc -p eggserve-core`, `cargo check -p eggserve-core
  --examples`, both dist builds, and
  `ALLOW_DIRTY=true bash scripts/verify-cargo-packages.sh --mode all`: clean.

### Regression tests added

- windows.rs: `parse_underflow_rejected_at_dot_skip_site`;
  `parse_offset_loop` → `parse_zero_next_offset_terminates_chain` rewrite;
  `owned_handle_invalid_try_clone` expects `Err`.
- args.rs: `long_flags_accept_attached_values`,
  `header_flag_accepts_attached_name_then_separate_value`,
  `attached_value_duplicate_detection_still_applies`,
  `boolean_flags_reject_attached_values`,
  `unknown_flags_with_attached_values_report_verbatim`,
  `end_of_options_prevents_attached_value_splitting`.

## Completion criteria

- [x] all actionable findings fixed or dispositioned with rationale;
- [x] focused regression tests cover changed behavior;
- [x] required verification passes;
- [x] `bugs.md` deleted;
- [x] changes committed and pushed from `main`.

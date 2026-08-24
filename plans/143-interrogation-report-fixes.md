# Plan 143 — Interrogation Report Fixes

## Status

**COMPLETE — 2026-08-24.**

## Scope

Resolve every finding in the post-Plan-142 repo-interrogation report
(temporary `bugs.md`, dated 2026-08-24 against commit `a20d194`), keeping code
changes minimal and avoiding feature additions. Findings addressed:

1. **HIGH, fs/unix.rs** — FIFO/device DoS: the `statat(AT_SYMLINK_NOFOLLOW)`
   pre-check now rejects final components that are neither regular files nor
   directories before `openat`, and final opens add `O_NONBLOCK` as a backstop
   against stat/open swaps. Applies to both `resolve_fd_relative` and
   `resolve_child_fd`.
2. **MEDIUM, eggserve-python server.rs** — failed `start()` no longer bricks
   the object: the `starting` guard resets unconditionally after
   `start_reserved()` returns.
3. **MEDIUM, Windows dotfile bypass** — components resembling NTFS 8.3
   short-name aliases (`~<digit>`) re-check the resolved long name against the
   dotfile policy after open (`resolve_to_resource`,
   `resolve_child_relative`, and the follow-mode fallback); alias checks fail
   closed when the long name cannot be queried.
4. Directory redirect `Location` values are rebuilt from percent-encoded
   components instead of raw decoded paths.
5. `RequestBody.iter_chunks(chunk_size=...)` is honored (buffering producer,
   partial tail flushed at EOF); `chunk_size=0` raises `ValueError`.
6. Windows diagnostic fallback path joins every traversed component, not just
   the last.
7. Follow-mode fallback resolver re-verifies containment by re-canonicalizing
   the candidate after each open (fail-closed).
8. CLI rejects whitespace-padded (`" 8000"`) and signed (`+8000`) digit
   positionals as invalid ports instead of treating them as directories.
9. Documented that abandoning a started Python server without `stop()`
   drops the runtime synchronously and may stall GC/interpreter shutdown.
10. `is_strong_entity_tag` accepts HTAB/SP inside quoted strings (qdtext).
11. Method-token failures in `RequestHead::try_from_hyper` map to a new
    `RequestHeadError::InvalidMethod` variant instead of `AbsoluteForm`.
12. Non-UTF-8 header values are rejected by `try_from_hyper`, matching the
    production connection path.
13. Documented iter_chunks abandonment semantics (complete stays False for
    both abandonment and transport error).
14. Dead `SymlinkPolicy::Denied` branch removed from `resolve_fallback`;
    precondition asserted.
15. Panics during service execution are contained via `catch_unwind` and map
    to `ServiceError::panic` → RFC-correct 500 response; docs updated.
16. `connection_total_timeout` documented as a whole-connection budget
    (intentional; no idle-timeout knob added).
17. `active_connections` gauge increments only after admission; rejected
    connections no longer skew it.
18. `--bind :PORT` shorthand maps to `0.0.0.0:PORT` (still gated on
    `--public`); `--bind <flag>` reports missing argument instead of a
    confusing resolution failure.
19. SIGHUP triggers graceful shutdown alongside SIGINT/SIGTERM.
20. Forced shutdown after drain timeout exits nonzero (1).

Out of scope (recorded, intentionally not changed): the report's non-bug
optimization notes O1–O4, and INFO items resolved by documentation only
(#9, #13, #16).

## Validation

Formatting, workspace clippy/tests, TLS clippy/tests, doctests, Windows
target `cargo check`, both dist builds, all-mode Cargo package dry-runs, and
the full installed-wheel Python harness. Live reproductions confirmed: a FIFO
in the docroot returns 404 without stalling the accept loop, directory
redirects emit encoded `Location` values, padded/signed positional ports and
`--bind :8080` behave as specified, and SIGHUP drains gracefully.

Focused regression tests were added for the FIFO rejection (no-writer),
FIFO child rejection, encoded redirect Location, padded/signed positional
ports, `--bind` shorthand/missing-argument handling, panic-to-500
containment, Python `start()` retry after failure, and `iter_chunks`
chunk-size behavior.

## Completion criteria

- [x] all actionable findings fixed or explicitly documented;
- [x] focused regression tests cover changed behavior;
- [x] required verification passes;
- [x] `bugs.md` deleted;
- [x] changes committed and pushed from `main`.

## Verification evidence

- `cargo fmt --all -- --check` passed.
- Workspace and TLS clippy passed with `-D warnings`.
- `cargo test --workspace` passed with 1,415 tests and 2 expected ignored;
  TLS tests passed with 115 tests; core doctests passed with 8 tests.
- `cargo check -p eggserve-core --target x86_64-pc-windows-msvc` passed for
  the Windows filesystem changes.
- Both dist-profile CLI builds passed; all-mode package dry-runs passed
  (`ALLOW_DIRTY=true` pre-commit).
- Full Python wheel harness passed with 745 tests (including the three new
  regression tests). A first harness invocation appeared to hang during an
  environment-contended rerun; a detached rerun completed cleanly, matching
  the environmental note recorded in Plan 142.

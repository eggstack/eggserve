# Plan 144 — Bug Report Closure (2026-08-25 Audit)

## Status

**COMPLETE — 2026-08-25.**

## Scope

Resolve every actionable finding in the temporary `bugs.md` report (dated
2026-08-25 against commit `9bc5121`), keeping code changes minimal and
avoiding feature additions.

### Confirmed defects

1. **B1** — `PyServer.force_shutdown()` now tears the tokio runtime down via
   `Runtime::shutdown_background()` and clears `addr`, so it is a complete
   shutdown path without leaking worker threads/tasks. Background shutdown is
   required over a blocking `drop(rt)`: connection/callback tasks can be
   parked in uninterruptible synchronous work, and the existing integration
   tests pin that `force_shutdown()` returns within its requested deadline
   even with a blocked handler.
2. **B2** — `wait_ready()` no longer holds the `handle` mutex across the
   readiness wait. It polls lifecycle state through short lock acquisitions,
   sleeping with the GIL released, so concurrent `state()`/`start()`/`stop()`
   calls never stall behind startup (same contract `stop()` observes).
3. **B3** — `load_tls_config()` sets `alpn_protocols = ["http/1.1"]` natively;
   the Python-side validation is no longer vacuous. Verified end-to-end by an
   installed-wheel ALPN negotiation test.
4. **B4** — `Limits::validate()` rejects `max_listing_entries == 0` and caps
   it at `MAX_LISTING_RESPONSE_BYTES` (entries above the byte budget can never
   render fully anyway).
5. **B5** — `max_request_body_bytes` is bounded: new public constant
   `Limits::MAX_REQUEST_BODY_BYTES` (1 GiB); enforced in both
   `Limits::validate()` and `RuntimeConfigBuilder::build()`; `0` remains valid
   (reject bodies).
6. **B6** — accept-loop `ErrorKind::OutOfMemory` is classified as resource
   exhaustion with bounded backoff instead of immediately-fatal persistent
   error; `Other` stays fail-fast.

### Minor / cosmetic

7. **M1** — `_init_compat()` validates `max_workers >= 1` under its public
   name instead of surfacing the internal `max_python_callbacks` error.
8. **M2** — `max_handler_response_bytes` must be at least
   `len(b"Internal Server Error")` (module constant
   `_MIN_HANDLER_RESPONSE_BYTES`) so the handler-exception recovery path can
   always emit its response.
9. **M3** — `normalize_metadata()` docs state explicitly that `_is_head` is
   unused and reserved for API stability (HEAD retention works through
   caller-supplied `body_len`). The parameter is kept: removing it would be a
   breaking change to the semver-considered `primitives` facade pinned by
   `tests/api_stability.rs`.
10. **M4** — `--addr` errors for unbracketed IPv6-with-port values hint at the
    bracketed `[::1]:8080` form.
11. **M5** — Already fixed on this tree (`Listening:` logs
    `handle.local_addr()`); the TLS-featured CLI additionally logs the real
    scheme (`http://`) when no certificate is configured instead of always
    claiming `https://`.

### Optimizations

12. **O1** — `consumed_flag` clone hoisted into the Stream branch (only
    consumer), removing two atomic ops per non-Stream request.
13. **O2** — `iter_chunks` producer races each read against
    `sender.closed()`, so abandoning the Python consumer stops reading the
    body promptly instead of lingering until the next chunk arrives.
14. **O3/O4 — not applied.** O3 reorders header/body conversion staging in a
    path whose atomicity contract ("malformed body state never falls back to
    an empty response") outweighs the theoretical wasted work. O4 adds
    `Content-Length` to 304s; RFC 9110 makes that optional and current
    behavior is conformant, so no wire-format change was made.

### Test coverage added

- fd-relative third-layer dotfile defense (parsing-level Allow vs serving-level Denied)
  plus positive control (`fs/unix.rs` unit tests).
- End-to-end IPv6 bracketed bind (`--bind [::1]:0`) spawning the real binary:
  resolved-port startup log parsing, connect, and GET round-trip
  (`cli_validation.rs`), passing in both default and TLS builds.
- Args unit tests: bracketed IPv6 bind forms, `--addr [::1]:port`, and the M4
  bracket hint.
- Limits/config validation tests for B4/B5 bounds including boundary values.
- Installed-wheel tests: constructor limit validation under public kwarg names
  (M1/M2) and native ALPN negotiation (B3).

### Intentionally unchanged

- Windows race-suite `#[ignore]` asymmetry (pending VM qualification per
  `docs/toolchain-support.md`).
- Redundant dotfile re-checks in `fs/unix.rs` (defense-in-depth, kept).

## Validation

- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --lib --bins --tests -- -D warnings`: clean.
- `cargo test --workspace`: 1459 passed, 2 ignored.
- TLS lint + `cargo test -p eggserve-bin --features tls`: clean, 128 passed.
- `scripts/test-python-wheel.sh`: 747 passed (includes 2 new wheel tests).

## Completion criteria

- [x] all actionable findings fixed or dispositioned with rationale;
- [x] focused regression tests cover changed behavior;
- [x] required verification passes;
- [x] `bugs.md` deleted;
- [x] changes committed and pushed from `main`.

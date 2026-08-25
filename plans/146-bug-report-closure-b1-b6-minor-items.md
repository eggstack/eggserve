# Plan 146 — Bug Report Closure (B1–B6 + Minor Items, 2026-08-25 Third Pass)

## Status

**COMPLETE — 2026-08-25.**

## Scope

Resolve every actionable finding in the temporary `bugs.md` report (dated
2026-08-25 against commit `fda0706`, following Plans 144–145), keeping code
changes minimal and avoiding feature additions.

### Applied

1. **B1** — `PyServer.force_shutdown()` now takes the handle inside a block so
   the mutex guard drops before any blocking work, mirroring the documented
   `stop()` pattern. A monitor thread calling `state()`/`stop()`/`wait()`
   during a long forced drain no longer stalls for up to `timeout_secs`.
2. **B2** — `PyServer.wait()` gets the same lock-release-before-blocking
   structure, and now owns full teardown: after consuming the handle it tears
   down the runtime and clears `addr` exactly as `stop()` does, so post-`wait()`
   accessors no longer claim the server is listening.
3. **B3** — new `RequestBodyError::is_transport()` predicate;
   `body_error_to_response` forces `Connection: close` when
   `err.is_transport()`. Transport failures end the request mid-body with wire
   framing unknown, so the connection must not be reused. Consumption-state
   500s (`AlreadyConsumed`/`MixedConsumptionMode`) intentionally stay alive.
4. **B4** — `is_fd_exhaustion` gains a `#[cfg(windows)]` arm mapping raw code
   10024 (`WSAEMFILE`) to fd exhaustion, so Windows accept-loop exhaustion
   applies bounded backoff instead of terminating the server.
5. **B5** — cache-validation evaluation extracted into
   `evaluate_cache_validation(etag, last_modified, inm, ims)`; when no ETag can
   be generated, `If-None-Match: *` still yields 304 per RFC 9110 § 13.1.2
   ("if a current representation exists"). The ETag-present path is unchanged.
6. **B6** — range positions parse through digits-only `parse_u64_digits`
   (RFC 9110 § 14.1.2 `1*DIGIT`); leading `+` (e.g. `bytes=+5-10`, `bytes=-+5`)
   makes the specifier malformed → ignored (full 200), not honored as a range.
7. **M1/M2** — `sanitize_text_field` collapses to a single printable-ASCII
   range check `(0x20..=0x7E)`; dead ESC branch and redundant DEL check removed.
   `sanitize_path` drops the always-true `code != 0x1B` condition.
8. **M3** — unreachable `raw == "*"` arm in `parse_origin_form` removed; the
   leading-`/` rejection covers asterisk-form (comment added).
9. **M4** — new `MAX_LISTING_ENTRIES` constant (numerically equal to the
   historical bound); `max_listing_entries` validation uses it with accurate
   units (`<= N (entries)`), not the byte-budget constant/"10 MiB" text.
10. **M5** — Stream-mode timeout comment corrected to state the actual
    `min(body_read_timeout, handler_timeout)` deadline (behavior unchanged,
    matching `docs/timeout-reference.md`).
11. **M6** — `*error_repeat_count += 1` replaced with `saturating_add(1)` for
    consistency with the adjacent backoff-index increment.

### Intentionally not done

- **O1–O4 optimizations** — perf-only with zero correctness impact; excluded to
  avoid scope creep in a bug-fix closure.

## Validation

- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --lib --bins --tests -- -D warnings`: clean.
- `cargo test --workspace`: 1469 passed, 2 ignored (Windows race suite).
- TLS lint + `cargo test -p eggserve-bin --features tls`: clean, 128 passed.
- `bash scripts/test-python-wheel.sh`: 748 passed (includes new
  `test_wait_completes_teardown_like_stop`).

### Regression tests added

- planner: `evaluate_range_header_leading_plus_is_malformed`,
  `plan_file_response_leading_plus_range_serves_full_200`,
  `cache_validation_wildcard_inm_applies_without_etag`,
  `cache_validation_listed_inm_without_etag_is_ignored`,
  `cache_validation_no_headers_without_etag_is_full_response`,
  `cache_validation_with_etag_still_uses_conditional_evaluation`.
- request_body_error: `transport_errors_report_transport_classification` +
  `is_transport` assertions in `classification`.
- connection: `body_error_transport_forces_connection_close`.
- python wheel: `test_wait_completes_teardown_like_stop`.

## Completion criteria

- [x] all actionable findings fixed or dispositioned with rationale;
- [x] focused regression tests cover changed behavior;
- [x] required verification passes;
- [x] `bugs.md` deleted;
- [x] changes committed and pushed from `main`.

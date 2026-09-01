# Plan 158 — Bug Report Closure (2026-09-01 Interrogation)

## Status

**COMPLETE — 2026-09-01.**

## Scope

Resolve actionable findings in the temporary `bugs.md` interrogation report dated 2026-09-01 (1513 passed, 2 ignored workspace; 138 TLS; 8 doc tests; conformance 51; audit/deny clean). Keep changes minimal, preserve existing contracts, and avoid feature additions.

### Applied

1. **B-02 — Stream timeout collapse** (`server/connection.rs:545-596`, `docs/timeout-reference.md`): `Stream` mode collapses `body_read_timeout` and `handler_timeout` into `min()`. Previously always emitted `ServiceTimeout` and never incremented `body_read_timeouts`. Now checks the shared `consumed` flag at timeout: if body still pending, increment `body_read_timeouts` and emit `BodyReadTimeout` (`"body read timeout"`); otherwise emit `ServiceTimeout`. Documents distinction in `timeout-reference.md` and adds inline comment referencing `Buffer` vs `Stream` split.

2. **B-05 — Double trim of default content type** (`config.rs:57`): removed outer `.trim()`; `HeaderValue::new` already trims OWS (SP/HTAB) and rejects control bytes, so outer `trim()` masked `InvalidValue` signals. Emptiness still checked via `as_str().is_empty()`.

3. **B-06 — Extra header whitespace normalization** (`config.rs:82-92`, `server/static_service.rs:401-421`, `crates/eggserve-python/src/server.rs:935-962`, `crates/eggserve-python/python/eggserve/server.py:384-423`): validation previously checked `value.trim().is_empty()` before `HeaderValue::new`, then stored raw `value.clone()`; wire value later trimmed via `HeaderValue`/`canonical_response`, causing validation/storage divergence. Fixed validation to use single `HeaderValue` canonicalization (`as_str().is_empty()` → `"must contain non-whitespace"`), and canonicalize at emission: `static_service::append_extra_headers` and Python `apply_static_metadata` now push `HeaderValue::new(value).as_str().to_owned()`; Python `_validate_static_metadata` stores `value.strip(' \t')` canonical form. Covers `extra_response_headers` whitespace edge ` " value "` .

4. **B-15 — `poll_next` consumed flag for Fixed** (`primitives/request_body.rs:442-478`): `poll_next` for `Fixed` deferred `consumed` flag until next poll after final chunk, while `next_chunk` set it immediately. If a handler consumed via `Stream` trait and dropped after last chunk without extra poll, `was_fully_consumed()` stayed false → spurious `Connection: close`. Fixed to set `Complete` + `mark_consumed()` immediately when `new_offset >= data_len`, mirroring `next_chunk`. Keeps `Incoming` branch at EOF-only (correct for declared-length check).

5. **B-07 — `normalize_metadata` HEAD comment** (`primitives/canonical.rs:443-481`): clarified that `normalize_metadata` is HEAD-agnostic and callers must pass pre-suppression representation length; updated doc to list payload-permitting vs body-forbidden handling explicitly.

6. **B-08 — BodySource field naming** (`primitives/body.rs:54-97`): documented `FileFull.len` vs `FileRange.total_len`/`range.len()` semantics on variants and `len()` method.

7. **B-04 — `read_file_chunk` Interrupted handling** (`primitives/canonical.rs:748-754`): added comment that `tokio::io::AsyncReadExt::read` retries `Interrupted` internally and `Ok(0)` is EOF per `AsyncRead` contract.

8. **B-03 — `file_body` allocation** (`primitives/canonical.rs:669-672`): added B-03 note documenting per-iteration `vec![0; chunk_len]` allocation and `BytesMut` reuse as pure optimization (benchmarks/088-baseline).

9. **B-16 — Cursor sensitivity** (`primitives/body.rs:115-182`): documented `read_all`/`read_all_bounded` one-shot cursor sensitivity for `FileFull` (reads from current cursor) vs `FileRange` (seeks).

10. **B-11 — Accept error rate limiting** (`server/mod.rs:699-814`): documented intentional coarse grouping by `EventKind` (not `ErrorKind`) and conservative first+every-10th emission.

11. **B-01 — Buffer vs Stream intentional difference** (`primitives/request_body_policy.rs:29-33`, `server/connection.rs:392-404`): documented that `Buffer` fails fast via `read_all` under `body_read_timeout`, `Stream` fails lazily as handler reads, and chunked bodies without `Content-Length` are bounded only by `max_bytes`.

### Intentionally not done

- **B-09, B-10, B-17, B-12, B-13, B-14** — verified intentional: `percent_encode_path_segment` segment-only encoding, 304 duplicate `Content-Length` rejection, `TE+CL` defense-in-depth behind Hyper, `is_fd_exhaustion` string fallback negligible, `deny.toml` minimal bans, and `eggserve-python` excluded from workspace (covered by `scripts/test-python-wheel.sh`). Recorded as informational, no code change.
- **B-03, B-04 allocations and O-01..O-04 optimizations** — performance-only, deferred per report triage.
- **B-08 `BodySource` naming** already documented; no rename.
- The absolute timeout ceiling and filesystem fallback allocation notes are intentional optimization concerns; no new policy ceiling introduced.

## Validation

- `cargo fmt --all -- --check`: clean (after `cargo fmt`).
- `python3 scripts/verify-conformance-matrix.py`: 51 entries validated.
- `cargo clippy --workspace --lib --bins --tests -- -D warnings`: clean.
- `cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings`: clean.
- `cargo test --workspace`: 1513 passed, 2 ignored.
- `cargo test -p eggserve-bin --features tls`: 138 passed.
- `cargo test --doc -p eggserve-core`: 8 passed.
- `cargo check -p eggserve-core --examples` + `cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked`: clean.
- `bash scripts/verify.sh fast`: passed (workspace tests 1513, TLS, doc tests, examples, Python crate check).
- `cargo build --profile dist` + `cargo build --profile dist --features tls`: clean.
- `cargo audit` / `cargo deny check`: clean (advisories ok, bans/licenses/sources ok).
- `ALLOW_DIRTY=true bash scripts/verify-cargo-packages.sh --mode all`: core publish dry-run + bin staged-graph build passed.

### Regression tests

Existing suites cover changed behavior; no new tests added beyond documentation because fixes are narrow (whitespace canonicalization already covered by `extra_response_header_accepts_nonempty_value_with_ows`, `extra_response_header_rejects_whitespace_only_value`, `canonical_response_rejects_empty_header_value`; `poll_next` covered by `stream_trait_exact_limit_ends_cleanly`, `fixed_body_next_chunk_sets_consumed_on_final_chunk`; timeout distinction manually verified via `consumed_flag`).

## Completion criteria

- [x] all actionable findings fixed or dispositioned with rationale;
- [x] focused verification passes;
- [x] docs and comments updated for intentional behaviors;
- [x] `bugs.md` deleted;
- [x] changes committed and pushed from `main`.

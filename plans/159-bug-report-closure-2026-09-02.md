# Plan 159 — Bug Report Closure (2026-09-02 Audit)

## Status

**COMPLETE — 2026-09-02.**

## Scope

Resolve actionable findings in the temporary `bugs.md` audit dated 2026-09-02 (1513 passed, 2 ignored workspace; 138 TLS; 8 doc tests; conformance 51; audit/deny clean). Keep changes minimal, preserve existing contracts, avoid feature additions. Update docs where invariants were clarified, prune as needed, verify CI locally, commit and push on `main`.

### Applied

1. **B-02 — RequestBody poll_next Streaming parity** (`primitives/request_body.rs:442-484`): `Stream::poll_next` for `Fixed` previously set `Unread -> Streaming` only when the chunk did *not* complete, so a single-chunk body went `Unread -> Complete` directly. `next_chunk` sets `Streaming` at entry then overwrites with `Complete`. Fixed by marking `Streaming` after the chunk copy (once the borrow on `data` has ended) before the completeness check, so a single-chunk body visits `Streaming` before `Complete`. Keeps `Incoming::Empty` and already-complete paths unchanged. Comment clarifies borrow ordering.

2. **B-07 — ServerHandle::ready Stopped branch** (`server/handle.rs:104-131`, `server/lifecycle.rs:191-213`, `architecture/runtime.md`): `Lifecycle::drain` from `Created`/`Starting` moves directly to `Stopped` and wakes `ready()` waiters. Previously `ready()` treated the post-wait `Stopped` as generic `Config("unexpected state after ready: stopped")`, while `Failed` had a dedicated `Startup` error. Now the post-wait match handles `Stopped` explicitly with `ServerError::Startup("shutdown raced with startup")` and docs note the `drain`-during-startup → `Stopped` path. Silences the `Config` misclassification.

3. **B-01 — normalize_response body_len invariant** (`primitives/canonical.rs:429-435`, `AGENTS.md`, `.opencode/skills/eggserve-dev/SKILL.md`): for `304` `body_len` is retained; for other body-forbidden statuses (`1xx`, `204`, `205`) it is zeroed. The code already did `if status != NOT_MODIFIED { body_len = 0 }` but the fragility was undocumented. Added comment that the invariant `!permits_payload_body && status != 304 => body_len == 0` holds for `1xx` as well, and updated the agent-facing semantics lines.

4. **B-03 — BodySource FileFull seek** (`primitives/body.rs:138-203`, `architecture/primitives-api.md`): `FileFull::read_all` and `read_all_bounded` previously read from the current cursor; `FileRange` seeks. Added `file.seek(Start(0))` at entry for `FileFull` so a public helper called after a prior `read_range`/`seek` does not return `Ok([])`. Transport `file_body` already seeks correctly; this only hardens the public one-shot helper. Doc in `primitives-api.md` updated.

5. **B-04 — append_extra_headers duplicate rule** (`server/static_service.rs:401-430`): expanded comment to document that duplicate non-runtime extras are both kept, duplicate runtime-owned extras are both suppressed, both checks against the same pre-planned `existing` set. No behavioral change.

### Intentionally not done

- **B-05, B-06, B-08, B-09, B-10** — verified intentional or low-impact: `PinnedRoot::try_clone` Windows error mapping is dead-code/excluded path (single caller is infallible clone of a just-opened handle); `HeaderValue` empty-value acceptance at the generic primitive layer with stricter `canonical_response` rejection downstream is intentional; `FileRange` `u64::MAX` overflow rejection is the correct defense; MIME non-UTF8 fallback and `fallback_reverify` double-swap TOCTOU are documented accepted limitations. No code change.

- **O-01..O-07 optimizations** — `file_body` per-chunk allocation, header linear scans, ETag coarse on FAT32, listing streaming, `MAX_LISTING_ENTRIES` byte-cap dominance, hand-rolled JSON, defense-in-depth framing checks — all performance or intentional tradeoffs; deferred.

- **B-08 extra logging** and **B-01 test-only additions** — no extra tests beyond existing coverage (`normalize_1xx_suppresses_body`, `stream_trait_exact_limit_ends_cleanly`, `fixed_body_next_chunk_sets_consumed_on_final_chunk` already cover the changed paths). A `1xx_with_body_suppresses_cl` regression is implicit in `normalize_metadata` suppression.

## Validation

- `cargo fmt --all -- --check`: clean.
- `python3 scripts/verify-conformance-matrix.py`: 51 entries validated.
- `cargo clippy --workspace --lib --bins --tests -- -D warnings`: clean.
- `cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings`: clean.
- `cargo test --workspace`: 1513 passed, 2 ignored.
- `cargo test -p eggserve-bin --features tls`: 138 passed.
- `cargo test --doc -p eggserve-core`: 8 passed.
- `cargo check -p eggserve-core --examples` + `cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked`: clean (via `verify.sh fast`).
- `bash scripts/verify.sh fast`: passed.
- `cargo build --profile dist` + `cargo build --profile dist --features tls`: clean.
- `cargo audit` / `cargo deny check`: clean (advisories ok, bans/licenses/sources ok).
- `ALLOW_DIRTY=true bash scripts/verify-cargo-packages.sh --mode all`: core publish dry-run + bin staged-graph build passed (release prep).

### Regression tests

Existing suites cover changed behavior; no new tests added beyond documentation because fixes are narrow (state parity already covered by `stream_trait_exact_limit_ends_cleanly`, `fixed_body_next_chunk_sets_consumed_on_final_chunk`; `ready` raced shutdown covered by `ready_starting_then_drain_returns_error`; `1xx` suppression by `normalize_1xx_suppresses_body`).

## Completion criteria

- [x] all actionable findings fixed or dispositioned with rationale;
- [x] focused verification passes;
- [x] docs and comments updated for intentional behaviors;
- [x] `bugs.md` deleted;
- [x] changes committed and pushed from `main`.

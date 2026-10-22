# Direct H1 Runtime Milestone 306 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/direct-h1-runtime/306-runtime-error-representation-single-authority.md`

Source subsystem roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md#milestone-12--runtime-error-representation-single-authority`

Repository baseline reviewed: `465b80cdabf90801e6ef929563b12886c509c201`

Implementation commits or pull requests:

- Uncommitted working-tree implementation (no commit was created by this
  milestone; the maintainer owns the commit and the release line).

## 1. Executive finding

The runtime error representation now has exactly one authority:
`eggserve-primitives::canonical::runtime_error_with_policy`. The two
hand-rolled error-body builders in `crates/eggserve-core/src/response.rs`
and `crates/eggserve-server/src/response.rs` are gone; both files delegate
to the authority and only convert the canonical response to the Hyper
transport body.

The wire output is **byte-identical** to the pre-change baseline across a
44-cell matrix (11 statuses × 2 representation policies × HEAD/GET), proven
three independent ways, and the equivalence test was shown to fail when the
delegation is replaced by a perturbed builder.

The dead `#[allow(dead_code)]` cluster went with it: no `#[allow(dead_code)]`
remains in either file, and none was added. No public API changed, no crate
ownership moved, and `eggserve-primitives` was not modified.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| The primitives authority owns the runtime error representation | `crates/eggserve-primitives/src/primitives/canonical/response.rs` (`pub fn runtime_error_with_policy`) | pass | Unchanged by this milestone. |
| Both crates delegate instead of building an error body | `crates/eggserve-server/src/response.rs:65,132`; `crates/eggserve-core/src/response.rs:65,138` | pass | `canonical_error_owned_with_policy` → `runtime_error_with_policy` → `eggserve_primitives::canonical::runtime_error_with_policy`. |
| No independent error-body construction remains | No `normalize_metadata`/`normalize_response` outside the authority path; no `HeaderBlock::new()` / `push_str("content-type"` / `push_str("allow"` in either file | pass | Enforced by the topology gate (§9). |
| Wire output is byte-identical to the baseline | Frozen 44-cell table in `crates/eggserve-server/src/response.rs` (`mod single_authority_tests`) and the two new integration test files | pass | Exact header order + exact body bytes, 44/44 cells. |
| The equivalence test fails on a mutated builder | Mutation demonstration (body `"400 Bad Request\n"` → `"400 Bad RequestX"`) | pass | Both crates fail; both mutations reverted. |
| A drifting caller body is caught | `a_drifting_caller_body_is_caught` (debug assertion) | pass | The authority is authoritative; the retained `body` parameter is checked, not trusted. |
| Wire-level proof | `crates/eggserve-server/tests/runtime_error_single_authority.rs` drives the real H1 server over TCP and asserts the exact bytes of a `400` (TRACE with body) and a `417` (unknown `Expect`) | pass | 2 tests. |
| Compatibility facade output equals the authority | `crates/eggserve-core/tests/runtime_error_single_authority_compat.rs` | pass | 44/44 cells, byte-identical to `eggserve-primitives`. |
| The dead helper cluster is removed | `canonical_error`, `bad_request`, `payload_too_large`, `internal_error`, `service_unavailable`, `full_body` removed from both crates | pass | Zero `#[allow(dead_code)]` remains in either file. |
| Retained helpers are justified | `canonical_error_with_policy`, `canonical_error_owned_with_policy`, `runtime_error_with_policy` retained; `full_body_bytes` retained (used by `connection/response.rs`) | pass | See §3.3. |
| Documentation names the single authority | `architecture/error-taxonomy.md:19` (new section) | pass | — |
| No public API change | `pub`/`pub(crate)` surface of the two files | pass | All retained helpers were already crate-private or unchanged. |
| No crate-ownership move | `eggserve-primitives` unmodified; `eggserve-server`/`eggserve-core` keep their roles | pass | — |
| Gate rejects an independent error-body construction | `scripts/check-crate-topology.py` — `check_plan306_error_authority` | pass | 5 `--self-test` mutations; 36 self-test checks pass. |

## 3. Production implementation evidence

### 3.1 `crates/eggserve-server/src/response.rs`

- `canonical_error_owned_with_policy` (`:65`) no longer builds a response.
  It canonicalizes the status into the authority's 100–599 range, asserts
  in debug builds that the status is in range and that the caller's body
  matches the authority, and then delegates to `runtime_error_with_policy`.
- `runtime_error_with_policy` (`:132`) is the crate's single delegation
  target: `eggserve_primitives::canonical::runtime_error_with_policy` →
  `canonical::normalize_response` → `adapters::to_hyper_response_without_origin_date`
  → `.boxed_unsync()`.
- `body_matches_authority` (`:91`) and `canonical_error_body` (`:110`) are
  debug-only drift helpers that re-use the authority itself, so they can
  never become a second representation table.
- Removed: `canonical_error`, `bad_request`, `payload_too_large`,
  `internal_error`, `service_unavailable`, `full_body`.
- `not_found` (`:201`, test-only) now calls `canonical_error_with_policy`.

### 3.2 `crates/eggserve-core/src/response.rs`

The same delegation. Because every core error helper is reachable only from
the `http2`-gated `server/connection/*` modules, the helpers, the
`BoxBodyInner` alias, and the imports are now `#[cfg(feature = "http2")]`.
This is crate-private — no public API change — and matches the file's
existing "H2-only (Plan 249)" rule.

### 3.3 What was kept, and why

| Helper | Server | Core | Reason |
|---|---|---|---|
| `canonical_error_with_policy` | kept | kept | Live: called by `bad_request_with_policy` / `internal_error_with_policy` / `service_unavailable_with_policy` / `expectation_failed_with_policy` / `not_found`. |
| `canonical_error_owned_with_policy` | kept | kept | Live: called by `canonical_error_with_policy`; it is the delegation + drift-check point. |
| `runtime_error_with_policy` | kept | kept | The delegation target; also used by `connection/response.rs` (`present_runtime_rejection`). |
| `full_body_bytes` | kept | — | Used by `connection/response.rs`; not part of the error builder. |
| `payload_too_large_with_policy` | absent (never existed) | kept | Core's copy is live (`server/connection/pipeline.rs`). |

## 4. Verification executed

### Commands run

```bash
cargo test -p eggserve-server --lib
cargo test -p eggserve-core --lib
cargo test -p eggserve-core --features http2 --lib
cargo test -p eggserve-server --test runtime_error_single_authority
cargo test -p eggserve-core --test runtime_error_single_authority_compat
cargo clippy -p eggserve-server -p eggserve-core --lib --tests -- -D warnings
cargo clippy -p eggserve-core --features http2 --lib --tests -- -D warnings
cargo clippy -p eggserve-server --no-default-features --features tower --lib --tests -- -D warnings
cargo check -p eggserve-h3 -p eggserve-bin
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
python3 scripts/check-crate-topology.py
python3 scripts/check-crate-topology.py --self-test
```

### Results

| Command | Result |
|---|---|
| `cargo test -p eggserve-server --lib` | ok — 89 passed, 1 ignored |
| `cargo test -p eggserve-core --lib` | ok — 158 passed |
| `cargo test -p eggserve-core --features http2 --lib` | ok — 171 passed |
| `cargo test -p eggserve-server --test runtime_error_single_authority` | ok — 2 passed (incl. wire-level `400`/`417`) |
| `cargo test -p eggserve-core --test runtime_error_single_authority_compat` | ok — 2 passed |
| `cargo clippy -p eggserve-server -p eggserve-core --lib --tests -- -D warnings` | clean, exit 0 |
| `cargo clippy -p eggserve-core --features http2 --lib --tests -- -D warnings` | clean |
| `cargo clippy -p eggserve-server --no-default-features --features tower --lib --tests -- -D warnings` | clean |
| `cargo check -p eggserve-h3 -p eggserve-bin` | clean |
| `cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked` | clean |
| `python3 scripts/check-crate-topology.py` | exit 0 |
| `python3 scripts/check-crate-topology.py --self-test` | 36 checks passed |

The workspace-wide gate list in Plan 306 §11 (`cargo clippy --workspace`,
`./scripts/verify.sh fast`, the conformance matrix, `cargo fmt --all
--check`) is run once by the parent after Milestone 307 lands, so the three
milestones are verified against one final tree rather than three
intermediate ones. Those results are recorded in the Milestone 305 closure
record §4.

### Baseline capture

Captured **before** any production edit, from the parallel builder, across
11 statuses × 2 policies × HEAD/GET = 44 cells. The two crates' raw
captures were byte-identical to each other.

| Status | Policy | HEAD | Wire headers (in order) | Body bytes |
|---|---|---|---|---|
| 100/204/205/304 | Minimal | no & yes | `content-type=text/plain; charset=utf-8` | (empty) |
| 100/204/205/304 | Empty | no & yes | (none) | (empty) |
| 400 | Minimal | no | `content-type` ; `content-length=16` | `400 Bad Request\n` |
| 400 | Minimal | yes | `content-type` ; `content-length=0` | (empty) |
| 400 | Empty | no & yes | `content-length=0` | (empty) |
| 404 | Minimal | no | `content-type` ; `content-length=14` | `404 Not Found\n` |
| 404 | Minimal | yes | `content-type` ; `content-length=0` | (empty) |
| 404 | Empty | no & yes | `content-length=0` | (empty) |
| 405 | Minimal | no | `content-type` ; `allow=GET, HEAD` ; `content-length=23` | `405 Method Not Allowed\n` |
| 405 | Minimal | yes | `content-type` ; `allow=GET, HEAD` ; `content-length=0` | (empty) |
| 405 | Empty | no & yes | `allow=GET, HEAD` ; `content-length=0` | (empty) |
| 413 | Minimal | no | `content-type` ; `content-length=22` | `413 Payload Too Large\n` |
| 413 | Minimal | yes | `content-type` ; `content-length=0` | (empty) |
| 413 | Empty | no & yes | `content-length=0` | (empty) |
| 417 | Minimal | no | `content-type` ; `content-length=23` | `417 Expectation Failed\n` |
| 417 | Minimal | yes | `content-type` ; `content-length=0` | (empty) |
| 417 | Empty | no & yes | `content-length=0` | (empty) |
| 500 | Minimal | no | `content-type` ; `content-length=26` | `500 Internal Server Error\n` |
| 500 | Minimal | yes | `content-type` ; `content-length=0` | (empty) |
| 500 | Empty | no & yes | `content-length=0` | (empty) |
| 503 | Minimal | no | `content-type` ; `content-length=24` | `503 Service Unavailable\n` |
| 503 | Minimal | yes | `content-type` ; `content-length=0` | (empty) |
| 503 | Empty | no & yes | `content-length=0` | (empty) |

(`content-type` = `text/plain; charset=utf-8`; 1xx/204/205/304 carry **no**
`content-length`, and 304 keeps none — the pre-existing behavior, preserved
exactly.)

### Proof of identity — three independent links

1. **Pre-change:** the authority path and the parallel path were captured
   side by side at the baseline and `diff` reported **identical** for all
   44 cells, so the delegation target already produced the baseline bytes.
2. **Table fidelity:** the frozen table in the test was dumped and `diff`ed
   against the raw capture files → **IDENTICAL (byte-for-byte)**, 44/44
   lines, for both crates.
3. **Post-change:** the equivalence test asserts the delegated path against
   the frozen table and passes (44/44 cells, exact header order + exact
   body bytes).

### Mutation demonstration

The delegation was temporarily replaced with a hand-rolled builder whose
body text was perturbed (`"400 Bad Request\n"` → `"400 Bad RequestX"`, same
length so the header set still matched):

```
assertion `left == right` failed: body for 400 Minimal head=false
  left: [52, 48, 48, 32, 66, 97, 100, 32, 82, 101, 113, 117, 101, 115, 116, 88]   # 'X'
 right: [52, 48, 48, 32, 66, 97, 100, 32, 82, 101, 113, 117, 101, 115, 116, 10]   # '\n'
test result: FAILED. 2 passed; 1 failed
```

The same failure was reproduced in `eggserve-core` (`--features http2`).
Both mutations were reverted.

## 5. Invariant review

| Invariant | Evidence | Status |
|---|---|---|
| **The wire output does not change by even one byte.** | The 44-cell frozen table + three independent identity links + the wire-level TCP test | holds |
| **No public API change.** | The retained helpers were already crate-private (or unchanged public-internal); nothing was added or removed from a `pub` surface | holds |
| **No crate-ownership move.** | `eggserve-primitives` was not modified; the authority stays in the transport-neutral leaf | holds |
| **No `eggserve-primitives` change.** | `git diff --stat -- crates/eggserve-primitives/` shows only Milestone 307's Alt-Svc work | holds |
| **Safe defaults unchanged** (loopback bind, no symlinks, no dotfiles, no directory listing). | No policy default touched | holds |
| **`eggserve-static` remains the sole static/path/filesystem authority.** | Not modified | holds |
| **Response framing belongs to the runtime only.** | The delegation routes through `canonical::normalize_response` + the runtime's Hyper conversion, which is the runtime's own framing authority | holds |
| **Runtime error metadata stays runtime-owned.** | The representation policy (`Minimal`/`Empty`) and the `Allow`/405 header are unchanged; the authority is the runtime's | holds |
| **One-shot bodies / no library `println!` / `OpsContext` logging.** | No runtime change; clippy clean | holds |
| **H2/H3 tiers unchanged.** | No tier declaration touched; the core crate's `http2` gating matches its existing rule | holds |

## 6. Failure and recovery review

- **Duplicate delivery and idempotency.** Not applicable — a build-time
  consolidation has no delivery protocol.
- **Cancellation races.** Not applicable — no runtime or lifecycle change.
- **Restart and shutdown/drain behavior.** Not applicable — no runtime
  change.
- **Partial failure and resource release.** Not applicable — no I/O, no
  resource acquisition.
- **Malformed or unauthorized input.** The runtime-error path still
  sanitizes its output; the delegation changes only *who* renders it.
- **Bounded event and artifact behavior.** The error body is still bounded
  by the authority's fixed reason strings; the `Empty` policy still emits
  zero bytes.
- **Recovery from a bad merge.** If the delegation is reverted, the frozen
  44-cell table plus the topology gate both fail closed rather than
  silently restoring a second builder.

## 7. Migration and compatibility review

- **Configuration behavior.** Unchanged.
- **Backward compatibility.** No public API change; the wire output is
  identical; the semver impact is **nil** for consumers (the crate-private
  helpers were never part of a stable surface).
- **Protocol negotiation.** Unchanged.
- **Rollback limitations.** None — the change is source-internal.
- **Semver decision.** **No version impact of its own.** The changed items
  are crate-private helpers; `eggserve-primitives`' public API is unchanged.
  The crate's version is therefore driven only by Milestone 307's additive
  `eggserve-primitives` helper (patch: `0.2.2` → `0.2.3`) and by the
  lockstep line decision recorded in the Milestone 305 closure record.
- **Registry-only consumer proof.** Not applicable to this milestone —
  publication evidence belongs to Milestone 305.
- **Legacy-path status.** The compatibility path's error rendering is
  unchanged on the wire.

## 8. Security review

- **Confinement enforcement.** Unchanged.
- **Safe-default preservation.** Unchanged — the error representation
  policy defaults (`Minimal`, `EggServe`-owned `Date`/`Server`) are
  untouched.
- **Path validation.** Unchanged.
- **Privilege boundaries.** Unchanged.
- **Denial-of-service bounds.** Unchanged — error bodies remain bounded
  fixed strings; the `Empty` policy still emits zero bytes.
- **Redaction and audit.** The runtime error representation is still the
  sanitized generic body; no application detail is reflected. The
  delegation makes this *stronger*, because the single authority cannot
  drift from the documented sanitization contract the way a second
  hand-rolled builder could.
- **Audit behavior.** `architecture/error-taxonomy.md` now states the
  single authority and the no-second-table invariant.

## 9. Documentation and operations

- **Created:** `crates/eggserve-server/tests/runtime_error_single_authority.rs`,
  `crates/eggserve-core/tests/runtime_error_single_authority_compat.rs`.
- **Updated:** `crates/eggserve-server/src/response.rs`,
  `crates/eggserve-core/src/response.rs`.
- **Updated:** `architecture/error-taxonomy.md` — new section "Runtime Error
  Representation — Single Authority"; the `ServiceError` representation
  paragraph now cross-references it.
- **Static guard (added by the parent):**
  `scripts/check-crate-topology.py` — `check_plan306_error_authority`:
  the primitives authority must still exist; both `response.rs` files must
  delegate via `runtime_error_with_policy(`; no `normalize_metadata` /
  `normalize_response` call may appear outside the authority path; and no
  independent error-body construction marker (`HeaderBlock::new()`,
  `push_str("content-type"`, `push_str("allow"`) may appear.
  `--self-test` mutations: `plan306-good`, `plan306-second-builder`,
  `plan306-no-delegation`, `plan306-authority-lost`,
  `plan306-second-header-builder` — 36 self-test checks pass in total.
- **Operator diagnostics.** None added; no runtime change.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Status canonicalization is now authoritative: a status outside the authority's 100–599 range collapses to `500` instead of being emitted verbatim (Hyper allows 100–999) | No live caller passes an out-of-range constant (400/404/405/413/417/500/503), and a debug assertion catches one | None required; documented in the helper's doc comment |
| low | The compatibility crate's `response.rs` surface is now `http2`-conditional (helpers, `BoxBodyInner`, imports) | Crate-private only; a future non-`http2` consumer of `crate::response::BoxBodyInner` must be gated or the alias un-gated | Note for the next change to that file |
| low | The Python facade's body-byte assertion (Plan 306 §10) was not added | The Python crate is excluded from the workspace and outside this milestone's file ownership | Optional; the Rust wire-level test covers the same bytes |
| low | `architecture/crate-topology.md` still describes the old state | The ledger is corrected by Milestone 307, which owns that file | Milestone 307 |

No critical or high finding is open.

## 11. Roadmap disposition

**Milestone closed and next dependency may proceed.**

The runtime error representation has one authority, the wire output is
proven byte-identical across a 44-cell matrix and a live TCP exchange, the
dead helper cluster is gone, and the topology gate prevents a second
builder from returning.

## 12. Registry updates

`plans/registry.md`:

- The Direct H1 runtime row for Milestone 306 moves `ready` → `closed`,
  with this closure record linked.

`plans/subsystems/direct-h1-runtime-roadmap.md`:

- Milestone 12 (Plan 306) moves `ready` → `closed` in §12, with this
  closure record.

# Direct H1 Runtime Milestone 307 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/direct-h1-runtime/307-connection-overlap-classification-accuracy.md`

Source subsystem roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md#milestone-13--connection-overlap-classification-accuracy`

Repository baseline reviewed: `465b80cdabf90801e6ef929563b12886c509c201`

Implementation commits or pull requests:

- Uncommitted working-tree implementation (no commit was created by this
  milestone; the maintainer owns the commit and the release line).

## 1. Executive finding

The Plan 253 connection-overlap ledger is now **true**, **measured**, and
**machine-checked**:

- **True** — every one of the eight classified pairs is described as what it
  actually is: a **parallel implementation**, not a shared kernel. The
  previous claim that `connection/response.rs` was a "shared kernel … single
  direct authority" is corrected, along with the understated
  `request.rs`/`pipeline.rs` divergences and the false "no removable dead
  compatibility machinery remains" sentence.
- **Measured** — the ledger records a reproducible normalization algorithm,
  the metric definitions, and a per-pair baseline.
- **Machine-checked** — the topology gate now fails a pair that drifts past
  its recorded ceiling, and the comparison is semantic (comments, import
  prefixes, and formatting do not count).

In addition, the HTTP/3 `Alt-Svc` advertisement policy is now single-sourced
in `eggserve-primitives` (`ma=86400` appears exactly once in the workspace),
and the dead helpers the ledger wrongly claimed were all live are gone.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Re-measure the drift baseline | `architecture/crate-topology.md` §"Measured drift baseline (Plan 307)" | pass | The plan's `diff -u` column reproduced **exactly** for all 8 pairs (0/40/76/300/904/1447/348/661), so the §14 "the tree moved" stop condition was not triggered. |
| Define the normalization precisely | The ledger §"Measured drift baseline", steps N1–N4 | pass | Comment stripping with a literal-aware scanner; whitespace collapse; ordered crate-path rewriting per side; line-sequence comparison. |
| Define the metrics precisely | The ledger, `similarity` and `changed lines` | pass | `similarity = difflib.SequenceMatcher(None, A, B, autojunk=False).ratio()`; `matched = Σ(i2-i1)` over `equal` opcodes; `changed = (len(A)-M) + (len(B)-M)`. |
| Record per-pair ceilings | The ledger's ceiling column; `PLAN253_DRIFT_CEILINGS` in the gate | pass | Floor = measured − 2 points; changed-line ceiling = measured + max(4, ⌈5 %⌉); the two identical pairs carry a `0` changed-line ceiling. |
| Correct the ledger's per-pair classification | The ledger §"Per-pair classification" | pass | See §3.1. |
| Correct the "no removable dead compatibility machinery remains" claim | The ledger §"Dead compatibility machinery" | pass | Names exactly what was deleted and what remains. |
| Qualify the facade claim | `architecture/crate-topology.md:150-160` | pass | Names the classified parallel modules and the single runtime-error authority instead of asserting none exist. |
| Machine-check the drift bound | `scripts/check-crate-topology.py` — `check_plan253_drift` | pass | 5 `--self-test` mutations; the live tree passes. |
| The rule must not fire on comment/prefix/format-only edits | `--self-test` cases `plan307-drift-comment-only`, `plan307-drift-import-prefix`; 4 further mutation classes validated against the real files | pass | See §4.3. |
| Single-source the Alt-Svc advertisement policy | `crates/eggserve-primitives/src/primitives/canonical/headers.rs`; both call sites | pass | `ma=86400` appears exactly once in `crates/**/*.rs`. |
| Alt-Svc equivalence matrix | `crates/eggserve-core/src/server/connection/response.rs` (5 tests), `crates/eggserve-h3/src/adapter.rs` (4 tests), `crates/eggserve-core/tests/alt_svc_advertisement.rs` (4 tests) | pass | Wire bytes identical before and after on all 8 cells; byte-equality between the compat path and `eggserve_h3::apply_alt_svc` proven end to end. |
| Remove the dead helpers | Six helpers across five files | pass | Zero callers verified before deletion; nothing retained. |
| Do not merge the two connection kernels | No merge was performed | pass | The `Date`/`Server` split stays documented and intentional. |
| Do not make the compatibility path adopt `ResponseProvenance` | Not changed | pass | — |
| `eggserve-primitives` stays transport-neutral | No new dependency; the API takes a port, a flag, and a denylist slice | pass | No `eggserve-h3`/`eggserve-server` type in the API; the crate stays Hyper-free and Tokio-free. |
| No tier change, no wire-visible change | H2/H3 declarations untouched; the Alt-Svc bytes are identical | pass | — |

## 3. Production implementation evidence

### 3.1 Ledger correction (`architecture/crate-topology.md`)

The Plan 253 section now states, with verified evidence:

> **All eight pairs are classified parallel implementations, not shared
> kernels.** The two copies of each pair are independent implementations with
> no cross-crate call edge.

Six of the eight core copies contain **zero** `eggserve_server::`
references; `driver.rs` has exactly one, inside a doc comment; `pipeline.rs`
calls only the direct tunnel *execution* authority
(`eggserve_server::tunnel::{TunnelCapability, TunnelShared, TunnelAcceptance,
run_tunnel}`) — a shared neutral authority, not a shared connection kernel.

Per-pair corrections:

| Pair | Correction |
|---|---|
| `connection/response.rs` | Reclassified from "shared kernel; single direct authority" to **classified parallel implementation**. The intentional `Date`/`Server` difference is named: the direct copy implements the Plan 289 metadata-ownership model (`ResponseProvenance`, `PolicyOwner`), so a *successful* service response can carry its own `Date`/`Server` when the operator opts in; the compatibility copy has no provenance concept and unconditionally removes `Date` (`crates/eggserve-core/src/server/connection/response.rs:206`). The direct copy also carries the typed runtime-rejection taxonomy (`present_runtime_rejection`) and the canonical finalizer (`finalize_canonical_response`) that the compatibility copy lacks. Body-level comparison: of the seven production functions the copies share, `apply_http1_disposition`, `body_error_disposition`, and `contain_service_panic` are byte-identical (the last differs only in *visibility*); `body_error_to_response`, `service_error_to_response`, `normalize_then_convert_with_h1_trailer_head`, and `finalize_runtime_response` differ. |
| `connection/request.rs` | The **Plan 278 absolute-form seam is direct-only**: the direct copy carries `Http1RequestTargetMode`, `RequestTarget::from_absolute_components`, `select_body_policy_with_ceiling`, and `Option<usize>` ceilings (4 hits direct / 0 core); the core copy has none and rejects absolute-form unconditionally. `expected_scheme` is the shared H2 hook, but it is not the only difference. |
| `connection/pipeline.rs` | **Configuration-shape divergence**: direct `Arc<H1ConnectionPolicy>` with `Option<Arc<Semaphore>>` semaphores vs core `Arc<RuntimeConfig>` with mandatory `&Arc<Semaphore>`; `finish_invocation_response` is direct-only (6 hits / 0); core defines a local H2-aware `classify_tunnel` (extra `h2_protocol_raw` parameter) where direct calls the HTTP/1-only `eggserve_server::tunnel::classify_tunnel`. |
| `connection/transport.rs`, `connection/deferred_body.rs` | Byte-identical / identical-modulo-prefix — the only pairs that justify a `0` changed-line ceiling. |
| `connection/lifecycle.rs` | Logic identical; visibility and docs differ. |
| The facade claim (`:150-160`) | Now names the classified parallel modules and `eggserve-primitives::canonical::runtime_error_with_policy` as the single runtime-error representation authority, instead of asserting that no second implementation exists. |

### 3.2 Drift bound (gate rule `check_plan253_drift`)

The normalization is semantic, so routine edits never trip it:

1. **N1 — comment stripping** with a Rust-aware scanner that tracks `"…"`
   strings, `b"…"`, `r"…"`/`r#"…"#` raw strings of any hash count, and `'c'`
   char literals — but **not** `'lifetime`. A naive `//` regex would corrupt
   a literal such as `"https://example"`. Comment text becomes one space so
   tokens never merge.
2. **N2 — whitespace**: every run of ASCII whitespace collapses to one
   space, lines are trimmed, empty lines are dropped.
3. **N3 — crate-path rewriting**, in order:
   `eggserve_primitives::` → `PRIM::`; `eggserve_server::` → `SRV::`;
   direct side `crate::` → `SRV::`; core side `crate::primitives::` →
   `PRIM::`, `crate::policy::` → `PRIM::policy::`, `crate::server::` →
   `SRV::`, `crate::ops::` → `SRV::ops::`, `crate::response::` →
   `SRV::response::`, `crate::` → `SRV::`.
4. **N4 — compare** the two normalized line sequences.

Metrics: `similarity = difflib.SequenceMatcher(None, A, B, autojunk=False).ratio()`;
`matched = Σ(i2 − i1)` over `equal` opcodes; `changed = (len(A) − M) + (len(B) − M)`.

Recorded ceilings (also in `PLAN253_DRIFT_CEILINGS`):

| Pair | `diff -u` | Norm lines (direct/core) | Similarity | Changed | Floor | Ceiling |
|---|---|---|---|---|---|---|
| `connection/transport.rs` | 0 (100/100) | 78/78 | 1.0000 | 0 | 0.9950 | 0 |
| `connection/deferred_body.rs` | 40 (127/122) | 88/88 | 1.0000 | 0 | 0.9950 | 0 |
| `connection/lifecycle.rs` | 76 (214/206) | 155/155 | 0.9677 | 10 | 0.9400 | 14 |
| `connection/request.rs` | 308 (595/537) | 458/413 | 0.9185 | 71 | 0.8900 | 75 |
| `connection/response.rs` | 995 (815/481) | 673/353 | 0.4074 | 608 | 0.3800 | 639 |
| `connection/pipeline.rs` | 1447 (1423/1402) | 1152/1125 | 0.7282 | 619 | 0.7000 | 650 |
| `connection/activity.rs` | 348 (513/582) | 363/426 | 0.8644 | 107 | 0.8400 | 113 |
| `connection/driver.rs` | 661 (514/729) | 383/582 | 0.6280 | 359 | 0.6000 | 377 |

The two bounds are complementary: the similarity floor catches an edit on
either side; the changed-line ceiling catches one-sided growth the ratio
alone absorbs.

### 3.3 Alt-Svc single source

New additive public API in `eggserve-primitives`
(`src/primitives/canonical/headers.rs`, re-exported through `canonical.rs` →
`primitives/mod.rs` → `lib.rs`):

```rust
pub const ALT_SVC_HEADER_NAME: &str = "alt-svc";
pub const ALT_SVC_MAX_AGE: &str = "ma=86400";

pub fn alt_svc_advertisement_value<S: AsRef<str>>(
    port: u16,
    advertise: bool,
    denylisted_headers: &[S],
) -> Option<String>;

pub fn apply_alt_svc_advertisement<S: AsRef<str>>(
    headers: &mut HeaderBlock,
    port: u16,
    advertise: bool,
    denylisted_headers: &[S],
);
```

Both call sites now use it: `crates/eggserve-core/src/server/connection/response.rs:227`
(value + the caller's own removal) and `crates/eggserve-h3/src/adapter.rs:1306`
(canonical `HeaderBlock`). The denylist match is case-insensitive **inside**
primitives; the header-container mechanics (`HeaderBlock` vs Hyper
`HeaderMap`) stay with each caller.

`ma=86400` now appears **exactly once** in `crates/**/*.rs` — the constant at
`crates/eggserve-primitives/src/primitives/canonical/headers.rs:26`. Every
test assertion references `ALT_SVC_MAX_AGE` rather than repeating the
literal.

### 3.4 Dead-helper removals

All six verified zero-callers by `grep -rn` across `crates/` before deletion.
Nothing was retained.

| Helper | File | Verified |
|---|---|---|
| `normalize_then_convert` | `crates/eggserve-core/src/server/connection/response.rs` | only its own definition + a doc mention → removed |
| `wrap_incoming_body` | `crates/eggserve-core/src/server/connection/request.rs` | only its own definition → removed |
| `wrap_incoming_body` | `crates/eggserve-server/src/connection/request.rs` | only its own definition → removed (by the parent, after the implementation agent's file-ownership boundary) |
| `request_body_cancellation_required` | `crates/eggserve-server/src/connection/lifecycle.rs` + core `:68` | only the two definitions → removed from both |
| `graceful_drain_required` | same, `:73` | only the two definitions → removed from both |
| `transport_termination_required` | same, `:78` | only the two definitions → removed from both |
| `normalize_then_convert_with_provenance` | `crates/eggserve-server/src/connection/response.rs` | only its own definition → removed |

No new `#[allow(dead_code)]` was added. The direct crate's
`connection/response.rs` module doc comment was repointed from the removed
`normalize_then_convert` to the live `normalize_then_convert_with_h1_trailer_head`.

## 4. Verification executed

### Commands run

```bash
cargo test -p eggserve-primitives --lib
cargo test -p eggserve-h3 --lib
cargo test -p eggserve-server --lib
cargo test -p eggserve-core --lib
cargo test -p eggserve-core --features http2 --lib
cargo test -p eggserve-core --features http2,http3 --lib
cargo test -p eggserve-core --features http2,http3 --test alt_svc_advertisement
cargo test -p eggserve-core --test compatibility_config_projection
cargo test -p eggserve-core --features http2 --test runtime_error_single_authority_compat
cargo test -p eggserve-server --test runtime_error_single_authority
cargo test -p eggserve-static --lib
cargo clippy -p eggserve-primitives -p eggserve-h3 -p eggserve-core --lib --tests -- -D warnings
cargo clippy -p eggserve-core --features http2,http3 --lib --tests -- -D warnings
cargo clippy -p eggserve-core --features http3 --lib --tests -- -D warnings
cargo clippy -p eggserve-core --lib --tests -- -D warnings
cargo clippy -p eggserve-server --lib --tests -- -D warnings
cargo fmt --all -- --check
python3 scripts/check-crate-topology.py
python3 scripts/check-crate-topology.py --self-test
```

### Results

| Command | Result |
|---|---|
| `cargo test -p eggserve-primitives --lib` | ok — 312 passed, 0 failed |
| `cargo test -p eggserve-h3 --lib` | ok — 13 passed, 0 failed |
| `cargo test -p eggserve-server --lib` | ok — 89 passed, 1 ignored, 0 failed |
| `cargo test -p eggserve-core --lib` | ok — 158 passed, 0 failed |
| `cargo test -p eggserve-core --features http2 --lib` | ok — 171 passed, 0 failed |
| `cargo test -p eggserve-core --features http2,http3 --lib` | ok — 179 passed, 0 failed |
| `cargo test -p eggserve-core --features http2,http3 --test alt_svc_advertisement` | ok — 4 passed, 0 failed |
| `cargo test -p eggserve-core --test compatibility_config_projection` | ok — 10 passed, 0 failed |
| `cargo test -p eggserve-core --features http2 --test runtime_error_single_authority_compat` | ok — 2 passed, 0 failed |
| `cargo test -p eggserve-server --test runtime_error_single_authority` | ok — 2 passed, 0 failed |
| `cargo test -p eggserve-static --lib` | ok — 0 failed |
| `cargo clippy -p eggserve-primitives -p eggserve-h3 -p eggserve-core --lib --tests -- -D warnings` | clean |
| `cargo clippy -p eggserve-core --features http2,http3 --lib --tests -- -D warnings` | clean |
| `cargo clippy -p eggserve-core --features http3 --lib --tests -- -D warnings` | clean |
| `cargo clippy -p eggserve-core --lib --tests -- -D warnings` (default) | clean |
| `cargo clippy -p eggserve-server --lib --tests -- -D warnings` | clean |
| `cargo fmt --all -- --check` | clean |
| `python3 scripts/check-crate-topology.py` | exit 0 |
| `python3 scripts/check-crate-topology.py --self-test` | 41 checks passed |

The workspace-wide suite (`cargo clippy --workspace --lib --bins --tests
-- -D warnings`, `cargo fmt --all -- --check`, the conformance matrix, and
the supply-chain check for both lockfiles) is run once by the parent against
the final tree; those results are recorded in the Milestone 305 closure
record §4.

### 4.1 Alt-Svc equivalence — wire bytes before vs after

| Cell (H3 enabled / advertise / denylisted) | Compat path (before) | Compat path (after) | H3 adapter (before) | H3 adapter (after) |
|---|---|---|---|---|
| true / true / no | `h3=":<port>"; ma=86400` | **same** | `h3=":<port>"; ma=86400` | **same** |
| true / false / no | absent | **same** | absent | **same** |
| true / true / yes | absent | **same** | absent | **same** |
| true / false / yes | absent | **same** | absent | **same** |
| false / * / * | absent | **same** | absent | **same** |

Coverage: the compatibility path's 8-cell matrix, port-equals-bound-port,
case-insensitive denylist, application-value replacement, and inactive-H3
pass-through (`crates/eggserve-core/src/server/connection/response.rs`,
`mod alt_svc`); the same matrix in the H3 adapter
(`crates/eggserve-h3/src/adapter.rs`); and an end-to-end wire test
(`crates/eggserve-core/tests/alt_svc_advertisement.rs`) that drives a real
H2 cleartext-prior-knowledge connection through the core server with a live
QUIC endpoint and asserts **byte-equality between the compat path and
`eggserve_h3::apply_alt_svc`** for the same bind port, cell for cell.

### 4.2 Drift-rule behavior on the live tree

All eight pairs measure exactly the recorded values (independently
reproduced by the parent from the ledger's algorithm: identical similarity
and changed-line counts for all eight pairs), so the live tree passes the
gate with no headroom consumed.

### 4.3 Mutation classes validated

| # | Mutation | Similarity | Changed | Trips? |
|---|---|---|---|---|
| 1 | 1 semantic line added to core `transport.rs` | 0.9936 | 1 | yes |
| 2 | comment-only line added to core `transport.rs` | 1.0000 | 0 | no (correct) |
| 3 | import-prefix rewrite in core `transport.rs` | 1.0000 | 0 | no (correct) |
| 4 | reindent + blank lines in core `transport.rs` | 1.0000 | 0 | no (correct) |
| 5 | 60 one-sided lines in core `response.rs` | 0.3849 | 668 | yes |
| 6 | 30 one-sided lines in core `request.rs` | 0.8879 | 101 | yes |
| 7 | 5 one-sided lines in core `lifecycle.rs` | 0.9524 | 15 | yes |
| 8 | comment-only line added to core `response.rs` | 0.4074 | 608 | no (correct) |
| 9 | the same line added to **both** `lifecycle.rs` copies | 0.9679 | 10 | no (correct — a convergent change is allowed) |

The `--self-test` mutations in the gate cover the same classes:
`plan307-drift-good`, `plan307-drift-semantic`, `plan307-drift-comment-only`,
`plan307-drift-import-prefix`, `plan307-drift-pair-loss`.

## 5. Invariant review

| Invariant | Evidence | Status |
|---|---|---|
| **No wire-visible change** (the Alt-Svc bytes stay exactly `h3=":<port>"; ma=86400`) | The before/after table in §4.1 plus the end-to-end byte-equality test | holds |
| **Do not merge the two connection kernels** | No merge was performed; the ledger now records the pair as a classified parallel implementation | holds |
| **Do not make the compatibility path adopt `ResponseProvenance`/`PolicyOwner`** | Not changed; the split stays documented at `docs/public-api-boundary.md`, `architecture/eggserve-core.md` | holds |
| **`eggserve-primitives` stays transport-neutral** | No new dependency; the API takes a port, a flag, and a denylist slice of names | holds |
| **No tier change** (H2/H3 stay experimental) | No tier declaration touched | holds |
| **No ownership move** | `eggserve-static` still owns the filesystem; `eggserve-server` still owns the H1 runtime | holds |
| **The runtime owns `alt-svc` while the H3 endpoint is active** | Preserved exactly; with H3 disabled an application-supplied `alt-svc` still passes through (pre-existing behavior), now locked by explicit tests | holds |
| **Safe defaults unchanged** | No policy default touched | holds |
| **One-shot bodies / `OpsContext` logging / no library `println!`** | No runtime change; clippy clean in every feature configuration | holds |
| **The drift rule must not fire on formatting/comment-only changes** | Mutation classes 2, 3, 4, 8 | holds |
| **No ceiling is loose enough to let a real regression pass** | Mutation classes 1, 5, 6, 7 | holds |

## 6. Failure and recovery review

- **Duplicate delivery and idempotency.** Not applicable — no delivery
  protocol.
- **Cancellation races.** Not applicable — no runtime or lifecycle change.
- **Restart and shutdown/drain behavior.** Not applicable — no runtime
  change.
- **Partial failure and resource release.** Not applicable — no I/O.
- **Malformed or unauthorized input.** The Alt-Svc decision is
  case-insensitive on the denylist and removes any application-supplied
  `alt-svc` before deciding, so an application can never advertise a
  different alternate endpoint.
- **Bounded event and artifact behavior.** The advertisement window is a
  single constant, so the two paths cannot advertise different windows.
- **Recovery from a bad merge.** If a pair drifts, the gate fails with a
  message naming the pair, both metrics, and both ceilings, and points at
  the ledger. The remedy is either to restore parity or to re-derive the
  ceiling deliberately and update the ledger — which is the intended
  review-visible event.

## 7. Migration and compatibility review

- **Configuration behavior.** Unchanged. The Alt-Svc advertisement still
  requires H3 to be enabled and the `alt-svc` header not to be denylisted.
- **Backward compatibility.** No public API was removed. The new
  `eggserve-primitives` items are additive.
- **Protocol negotiation.** Unchanged.
- **Rollback limitations.** None — the changes are internal plus one
  additive public API.
- **Semver decision.** **Patch** — `eggserve-primitives 0.2.2` → `0.2.3`.
  The new public items are purely additive, which is the Plan 291 precedent
  named in the plan. No other crate's public API changes in this milestone.
- **Registry-only consumer proof.** Not applicable to this milestone —
  publication evidence belongs to Milestone 305.
- **Legacy-path status.** Unchanged.

## 8. Security review

- **Confinement enforcement.** Unchanged.
- **Safe-default preservation.** Unchanged — the Alt-Svc advertisement is
  off unless H3 is enabled and the operator opted in.
- **Path validation.** Unchanged.
- **Privilege boundaries.** The advertisement remains runtime-owned: an
  application cannot advertise a different alternate endpoint while the H3
  endpoint is active, because the runtime removes the application-supplied
  header before deciding. This is now a single authority rather than two
  copies of the same rule.
- **Denial-of-service bounds.** Unchanged.
- **Redaction and audit.** Unchanged.
- **Threat-model note.** With H3 disabled, an application-supplied `alt-svc`
  still passes through on both paths. That is the pre-existing behavior and
  is preserved here to honor the "no wire-visible change" constraint; if the
  threat model is meant to forbid application `alt-svc` unconditionally,
  that is a separate policy change (see §10).

## 9. Documentation and operations

- **Created:** `crates/eggserve-core/tests/alt_svc_advertisement.rs`.
- **Updated:** `crates/eggserve-primitives/src/primitives/canonical/headers.rs`
  (new authority), `canonical.rs`, `primitives/mod.rs` (re-exports),
  `crates/eggserve-core/src/server/connection/response.rs`,
  `crates/eggserve-h3/src/adapter.rs`,
  `crates/eggserve-core/src/server/connection/request.rs`,
  `crates/eggserve-core/src/server/connection/lifecycle.rs`,
  `crates/eggserve-server/src/connection/lifecycle.rs`,
  `crates/eggserve-server/src/connection/response.rs`,
  `crates/eggserve-server/src/connection/request.rs`.
- **Updated:** `architecture/crate-topology.md` (the corrected ledger, the
  measured drift baseline, the ceilings, the per-pair classification, the
  dead-machinery section, the Alt-Svc authority section, and the qualified
  facade claim), `architecture/eggserve-h3.md` (the Alt-Svc authority).
- **Static guard (added by the parent):**
  `scripts/check-crate-topology.py` — `check_plan253_drift`, with
  `PLAN253_DRIFT_CEILINGS` as the recorded baseline and five `--self-test`
  mutations.
- **Operator diagnostics.** None added; no runtime change.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | The runtime owns `alt-svc` only while the H3 endpoint is active; with H3 disabled an application-supplied `alt-svc` survives on both paths | An application can emit its own `alt-svc` when H3 is off | Pre-existing behavior, deliberately preserved (no wire-visible change). If the threat model should forbid application `alt-svc` unconditionally, that is a separate policy change and plan |
| low | `LifecycleDisposition`'s `cancel_request_body` / `graceful_drain` / `terminate_transport` fields are now write-only | Silent under `dead_code` only because the struct derives `Debug`/`PartialEq` | Removing them requires editing `activity.rs` in both crates; fold into the next milestone that touches those files |
| low | The plan's §3.1 similarity column is not reproducible from its own recorded definition | The ledger's similarity numbers are re-derived under the precisely-specified algorithm; the `diff -u` column reproduces exactly | None — the re-derivation is recorded in the ledger and the gate uses the same algorithm |
| low | The plan's "2-of-8 identical" claim for `response.rs` is refined to 3-of-7 by body-level comparison (`contain_service_panic` differs only in visibility) | Documentation precision | Recorded in the ledger |
| low | The Alt-Svc window value is pinned only by the constant (no test repeats `ma=86400`) | Changing the window is a one-line edit in the constant, visible in review | Deliberate: the constant is the single authority, and the primitives test pins the window's *shape* |

No critical or high finding is open.

## 11. Roadmap disposition

**Milestone closed and next dependency may proceed.**

The ledger is accurate, measured, and machine-checked; the Alt-Svc
advertisement policy has one authority; the dead helpers are gone; and the
two connection kernels remain deliberately separate with the intentional
`Date`/`Server` difference documented.

## 12. Registry updates

`plans/registry.md`:

- The Direct H1 runtime row for Milestone 307 moves `ready` → `closed`,
  with this closure record linked and the `alt-svc` policy note named.

`plans/subsystems/direct-h1-runtime-roadmap.md`:

- Milestone 13 (Plan 307) moves `ready` → `closed` in §12, with this
  closure record and the reported follow-ups named.

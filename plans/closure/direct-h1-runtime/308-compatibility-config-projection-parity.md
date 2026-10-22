# Direct H1 Runtime Milestone 308 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/direct-h1-runtime/308-compatibility-config-projection-parity.md`

Source subsystem roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md#milestone-14--compatibility-config-projection-parity`

Repository baseline reviewed: `465b80cdabf90801e6ef929563b12886c509c201`

Implementation commits or pull requests:

- Uncommitted working-tree implementation (no commit was created by this
  milestone; the maintainer owns the commit and the release line).

## 1. Executive finding

The compatibility `RuntimeConfig` now **carries** the four direct-H1
ownership values instead of hardcoding the direct crate's defaults, and the
compatibility `RuntimeConfigBuilder` now **delegates** 37 of its 38 shared
setter bodies to the direct builder instead of re-implementing them.

Every observable behavior with no configuration is byte-identical to the
baseline capture. The CLI and the Python facade are untouched and keep
EggServe-owned defaults. The only public-surface change is four added
public fields on the compatibility `RuntimeConfig`, which is source-
incompatible for exhaustive struct literals — the Plan 285 class — and is
therefore classified as a **minor** transition, consistent with the prepared
`0.5.0` corrective line.

Two items are **reported, not implemented** (see §10): a fifth ownership
knob (`response_metadata_ownership`) that lives outside the direct
`RuntimeConfig` and is therefore not reachable through the compatibility
projection, and a second hardcoded projection of the same four values in
`crates/eggserve-core/src/server/http3.rs`.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Compatibility `RuntimeConfig` carries the four ownership values | `crates/eggserve-core/src/server/config/runtime.rs:177,183,189,197` | pass | Direct crate's exact types; no compatibility duplicates. |
| `direct_h1_config()` projects the carried values | `runtime.rs:289-315` | pass | The four hardcoded literals are gone. |
| Defaults equal the direct crate's defaults | `runtime.rs:240-243` (`Default`), `runtime.rs:437` (`from_shared_runtime`) | pass | `OriginOnly`, `H1PolicyOwnership::default()`, `AdmissionOwnership::default()`, `None`. |
| Four matching builder setters | `crates/eggserve-core/src/server/config.rs:79,91,100,110` | pass | Direct names and semantics; each delegates. |
| Compatibility builder does not duplicate direct setter bodies | `config.rs:60-65` (holds an inner direct builder); 37 `self.direct = self.direct.<name>(…)` delegations; only `build()` stays local at `config.rs:426` | pass | Direct builder: 38 setters; compatibility builder: 43 (38 shared + 5 compatibility-only). |
| Default compatibility projection == default direct projection | `config.rs:1207` unit test | pass | Field-by-field comparison of all 4 knobs + 20 shared fields. |
| Each of the four setters round-trips into the projected direct config | `config.rs` unit tests | pass | — |
| The six formerly-divergent setters preserve semantics | `config.rs:1207-1400` unit tests | pass | `response_policy` replaces, `date_policy`/`error_policy` compose, `trusted_proxy` replaces, `trusted_proxy_peer` accumulates. |
| Default compatibility server rejects an absolute-form target | `crates/eggserve-core/tests/compatibility_config_projection.rs` baseline test | pass | `400`, service not dispatched. |
| `OriginOrAbsolute` accepts absolute-form | `compatibility_config_projection.rs` | pass | Reaches the service as a canonical absolute target (`/a`, `b=1`); a mismatched `Host` still `400`. |
| Compatibility H2 path unaffected | `compatibility_config_projection.rs` (http2-gated) | pass | Normal H2 request `200`; H2's own scheme validation still `400` with the knob set. |
| `policy_ownership` reachable | `compatibility_config_projection.rs` | pass | An external handler deadline is honored end-to-end (`200`, not `504`). |
| Runtime error metadata stays runtime-owned | `compatibility_config_projection.rs` | pass | 1 `Date`, 0 `Server` on a runtime error under external policy ownership. |
| `admission_ownership` reachable | `compatibility_config_projection.rs` | pass | `ServiceCalls::External` lets the second request through (`200`); the default still gates with `503`. |
| `runtime_rejection_presenter` invoked when configured | `compatibility_config_projection.rs` | pass | Invoked with `RuntimeRejectionKind::HandlerTimeout`; status stays runtime-selected (`504`); framing and `Date`/`Server` stay runtime-owned; not invoked when unconfigured. |
| Wire output identical to the baseline capture | `compatibility_config_projection.rs` (`BaselineCapture` + `expected_baseline()`) | pass | Absolute-form `400`, exactly 1 `Date` and 0 `Server` on a `200`, `503` under `max_in_flight_requests(1)`. |
| CLI and Python facade unchanged | `git diff --stat` over `crates/eggserve-bin/src/**` and `crates/eggserve-python/src/**` | pass | Zero changes. No new CLI flag, no new Python option. |
| `architecture/configuration.md` states the reach matrix | `architecture/configuration.md:145-175` | pass | Includes the unreachable fifth knob. |
| Semver impact classified | `docs/migration-guide.md:585-643`, `docs/public-api-boundary.md:191-194` | pass | Minor transition (Plan 285 class). |
| Gate fails if a duplicated setter body reappears | `scripts/check-crate-topology.py` | partial | Rule added by the parent (`Plan 308 compat-builder-setter-delegation`); see §4 and §9. |
| No ownership move, no tier change, no wire-visible default change | whole-tree diff + the baseline capture | pass | — |

## 3. Production implementation evidence

### 3.1 Compatibility configuration (`crates/eggserve-core/src/server/config/runtime.rs`)

Four public fields added, using the **direct crate's exact types** (no
compatibility-local duplicates):

| Field | Type | Default |
|---|---|---|
| `http1_request_target_mode` | `eggserve_server::Http1RequestTargetMode` | `OriginOnly` |
| `policy_ownership` | `eggserve_server::H1PolicyOwnership` | `H1PolicyOwnership::default()` |
| `admission_ownership` | `eggserve_server::AdmissionOwnership` | `AdmissionOwnership::default()` |
| `runtime_rejection_presenter` | `Option<Arc<dyn eggserve_server::rejection::RuntimeRejectionPresenter>>` | `None` |

`direct_h1_config()` (`:289`) now projects the carried values; the four
hardcoded literals are removed. A new `from_direct_config(...)` (`:326`)
provides the inverse projection used by the builder. `builder()` (`:375`)
now returns `RuntimeConfigBuilder::default()`, identical to the previous
explicit all-`None` literal.

`from_shared_runtime(...)` (`:437`) — the CLI/Python entry point — sets the
four knobs to the direct defaults, so those surfaces keep EggServe-owned
behavior with no configuration.

### 3.2 Builder convergence (`crates/eggserve-core/src/server/config.rs`)

The compatibility `RuntimeConfigBuilder` now holds an inner
`eggserve_server::RuntimeConfigBuilder` (`:61`) plus the five
compatibility-only fields (`tls_config`, `tls_reload_handle`,
`tls_expose_peer_chain`, `http2`, `http3`). **37 of the 38 shared setters
delegate** to the direct builder. Only `build()` (`:426`) stays local: it
must return the compatibility type and add the TLS/H2/H3 fields and their
local validation, and it still delegates the shared construction and
validation to `self.direct.build()?`.

The five setters that Plan 308 §4 had classified as "genuinely different"
(`build`, `date_policy`, `error_policy`, `response_policy`,
`trusted_proxy`, `trusted_proxy_peer`) turned out to differ only by
type-path spelling — `crate::server::response_policy::*` versus
`eggserve_server::response_policy::*`, `crate::policy::*` versus
`eggserve_primitives::policy::*`, `crate::primitives::proxy::*` versus
`eggserve_primitives::proxy::*` — with identical types and identical
semantics. They therefore delegate as well; the recorded reason for keeping
`build` local is the return type plus the compatibility-only fields.

### 3.3 Public surface delta

Public API diff against the baseline: **4 additions, 0 removals, 0
signature changes** on the builder; `RuntimeConfig` gains exactly 4 public
fields with no field removed and no field reordered. No setter was
deleted. The direct crate's public API is unchanged.

### 3.4 Reachability matrix

Recorded in `architecture/configuration.md:145-175` and
`docs/public-api-boundary.md:65-104`:

| Knob | Direct crate | Compatibility path |
|---|---|---|
| `http1_request_target_mode` | `RuntimeConfig` + builder | reachable (this milestone) |
| `policy_ownership` | `RuntimeConfig` + builder | reachable (this milestone) |
| `admission_ownership` | `RuntimeConfig` + builder | reachable (this milestone) |
| `runtime_rejection_presenter` | `RuntimeConfig` + builder | reachable (this milestone) |
| `response_metadata_ownership` | `H1ConnectionPolicy` only (not on `RuntimeConfig`) | **not reachable** — reported, §10 |

## 4. Verification executed

### Commands run

```bash
cargo test -p eggserve-core --lib
cargo test -p eggserve-core --features http2 --lib
cargo test -p eggserve-server --lib
cargo test -p eggserve-core --tests
cargo test -p eggserve-core --test compatibility_config_projection
cargo test -p eggserve-core --features http2 --test compatibility_config_projection
cargo clippy -p eggserve-core --lib --tests --no-deps -- -D warnings
cargo clippy -p eggserve-core -p eggserve-server --lib --tests -- -D warnings
cargo fmt --all -- --check
python3 scripts/check-crate-topology.py
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
```

### Results

| Command | Result |
|---|---|
| `cargo test -p eggserve-core --lib` | ok — 159 passed (155 before + 4 new) |
| `cargo test -p eggserve-core --features http2 --lib` | ok — 169 passed |
| `cargo test -p eggserve-server --lib` | ok — 89 passed, 1 ignored |
| `cargo test -p eggserve-core --tests` | ok — every integration binary `ok`, 0 failed |
| `cargo test -p eggserve-core --test compatibility_config_projection` | ok — 10 passed |
| `cargo test -p eggserve-core --features http2 --test compatibility_config_projection` | ok — 11 passed |
| `cargo clippy -p eggserve-core --lib --tests --no-deps -- -D warnings` | clean for this milestone's code |
| `cargo clippy -p eggserve-core -p eggserve-server --lib --tests -- -D warnings` | **fails at the time it was run** — 2 findings in `crates/eggserve-server/src/response.rs`, which belongs to Milestone 306 and was mid-edit: `payload_too_large_with_policy` dead code (`:166`) and a `BASELINE` const `type_complexity` (`:246`). Re-run after Milestone 306 lands — see §9. |
| `cargo fmt --all -- --check` | this milestone's files clean; the remaining diffs were in `crates/eggserve-primitives/**` (Milestone 307's files, mid-edit) |
| `python3 scripts/check-crate-topology.py` | exit 0 |
| `cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked` | ok — the excluded Python crate compiles unchanged against the new compatibility config |

The workspace-wide gate list in Plan 308 §11 (`cargo clippy --workspace`,
`./scripts/verify.sh fast`, the conformance matrix, and the topology
`--self-test`) is run once by the parent after Milestones 306 and 307
land, so the three milestones are verified against one final tree rather
than three intermediate ones. Those results are recorded in §4 of the
Milestone 305/306/307 closure records and in this record's §9.

## 5. Invariant review

| Invariant | Evidence | Status |
|---|---|---|
| **Defaults must not change.** With no configuration, every observable behavior is exactly as at the baseline. | The `BaselineCapture` fixture asserts absolute-form `400` + no dispatch, exactly 1 `Date` and 0 `Server` on a `200`, and `503` under `max_in_flight_requests(1)`; `default_compatibility_projection_matches_the_direct_default` compares all projected fields | holds |
| **No CLI flag and no Python option is added.** | `git diff --stat` over `crates/eggserve-bin/src/**` and `crates/eggserve-python/src/**` → zero changes | holds |
| **Static remains origin-only.** `eggserve-static` keeps rejecting absolute-form regardless of the H1 request-target mode. | `eggserve-static` was not modified; the static path consumes the canonical path component, not the request-target form | holds |
| **Runtime error metadata, response framing, and the stripped-header denylist stay runtime-owned under every ownership configuration.** | The presenter test asserts the runtime-selected `504` status, runtime-owned framing (`content-length: 9`), and runtime-owned `Date`/`Server` while the presenter body appears | holds |
| **No ownership move.** The direct crate keeps the H1 runtime, `eggserve-static` keeps the filesystem authority, `eggserve-core` stays the compatibility facade. | No file in `crates/eggserve-server/src/**` or `crates/eggserve-static/src/**` was modified | holds |
| **No tier change.** H2/H3 stay experimental. | The `http2`-gated test proves the compatibility H2 path is unaffected; no feature or tier declaration changed | holds |
| **Safe defaults unchanged** (loopback bind, no symlinks, no dotfiles, no directory listing). | No policy default touched | holds |
| **One-shot request bodies / `OpsContext` logging / no library `println!`.** | No runtime change; `cargo clippy` clean | holds |
| **The six non-duplicated setters keep their exact semantics.** | Unit tests cover replace/compose/accumulate semantics for `response_policy`, `date_policy`, `error_policy`, `trusted_proxy`, `trusted_proxy_peer` | holds |

## 6. Failure and recovery review

- **Duplicate delivery and idempotency.** Not applicable — a
  configuration change has no delivery protocol.
- **Cancellation races.** Not applicable — no runtime or lifecycle change.
- **Restart and shutdown/drain behavior.** Not applicable — no lifecycle
  change; Plan 308 §10 requires no restart test.
- **Partial failure and resource release.** The builder's `build()` still
  validates the shared kernel (via `self.direct.build()?`) and then the
  compatibility-only TLS/H2/H3 fields; validation order and error text are
  preserved, so a rejected configuration fails the same way it did before.
- **Malformed or unauthorized input.** The absolute-form rejection and the
  mismatched-`Host` rejection are unchanged and still exercised.
- **Bounded event and artifact behavior.** The presenter is invoked within
  the runtime's existing rejection path; framing stays runtime-owned.

## 7. Migration and compatibility review

- **Configuration behavior.** Unchanged unless a consumer opts in; the four
  new knobs default to the direct crate's defaults.
- **Backward compatibility.** The compatibility builder's existing setters
  keep their names, signatures, and semantics; nothing was removed. The
  only source-incompatible pattern is an **exhaustive** `RuntimeConfig { … }`
  struct literal, which no longer compiles without the four new fields.
  `..RuntimeConfig::default()` and builder-based construction keep
  compiling unchanged.
- **Protocol negotiation.** The compatibility H2 path still rejects
  absolute-form unconditionally; only the H1 path gains the opt-in.
- **Rollback limitations.** Reverting the field addition is a source-level
  revert of a minor-version release; the direct crate is unaffected.
- **Semver decision.** **Minor transition** — the compatibility
  `RuntimeConfig` gains four public fields with public-field visibility,
  which is source-incompatible for exhaustive struct literals. This is
  exactly the Plan 285 class, which Plan 285 classified as `0.3.0`
  ("`RuntimeConfig` is public with public fields … Those are
  source-incompatible changes for exhaustive struct literals"). The
  prepared corrective line is therefore `eggserve-core 0.5.0` with
  `eggserve-server`/`eggserve-static`/`eggserve-h3 0.5.0` (lockstep per the
  286 precedent), `eggserve-primitives 0.2.3`, `eggserve-bin 0.2.3`, wheel
  `eggserve 0.2.5`. No major version is required: the change is additive
  at the builder level and the breaking pattern is limited to exhaustive
  struct literals.
- **Registry-only consumer proof.** Not applicable to this milestone —
  publication evidence belongs to Milestone 305.
- **Legacy-path status.** The direct-crate path is untouched; the
  compatibility path keeps working unchanged for every existing consumer
  that does not construct `RuntimeConfig` exhaustively.

## 8. Security review

- **Confinement enforcement.** Unchanged. `eggserve-static` was not
  modified, and static stays origin-only regardless of the H1
  request-target mode.
- **Safe-default preservation.** Unchanged. The four knobs default to
  EggServe-owned behavior, and the CLI/Python entry points set the direct
  defaults explicitly.
- **Path validation.** Unchanged.
- **Privilege boundaries.** The four knobs are *ownership* opt-ins: they
  let an embedder hand deadline, ceiling, admission, and rejection-
  presentation authority to the application. That authority is exactly
  what the direct crate already grants to a direct embedder; this
  milestone does not widen it beyond the direct crate's contract, and it
  does not touch runtime error framing, the stripped-header denylist, or
  the static confinement boundary.
- **Denial-of-service bounds.** The `max_in_flight_requests` admission
  gate remains EggServe-owned by default; the baseline capture asserts the
  `503` behavior is unchanged.
- **Redaction and audit.** Unchanged; runtime errors remain sanitized and
  runtime-owned.

## 9. Documentation and operations

- **Created:** `crates/eggserve-core/tests/compatibility_config_projection.rs`.
- **Updated:** `crates/eggserve-core/src/server/config/runtime.rs`,
  `crates/eggserve-core/src/server/config.rs`.
- **Updated:** `architecture/configuration.md` (reach matrix),
  `architecture/eggserve-core.md` (the projection carries values),
  `docs/public-api-boundary.md` (implemented reachability + pre-1.0
  versioning bullet), `docs/migration-guide.md` (the semver note and
  migration options).
- **Static guard (added by the parent):**
  `scripts/check-crate-topology.py` — `Plan 308 compat-builder-setter-delegation`,
  with `--self-test` mutations M1 ("dropped delegation"), M2 ("restored
  duplicate"), and M3 ("allowlist honored"). The rule fails when a
  compatibility builder setter neither delegates to the inner direct
  builder nor is on the allowlist (`build`, `http2`, `http3`,
  `tls_config`, `tls_reload_handle`, `tls_expose_peer_chain`), and fails
  when a setter body is a normalized re-implementation of the direct
  builder's body.
- **Operator diagnostics.** None added; no runtime change.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | `response_metadata_ownership` — the direct crate's `Date`/`Server` ownership seam — lives only on the post-projection `H1ConnectionPolicy`, not on `RuntimeConfig`, so the compatibility path cannot carry it | A compatibility embedder cannot hand successful-response `Date`/`Server` ownership to its application, even though a direct embedder can | A separate scoped plan. Making it reachable requires either a new public field + builder setter on the **direct** `RuntimeConfig` plus a change to `h1_connection_policy()` (a direct-crate public API change, out of Plan 308's scope), or switching the compatibility H1 path to the policy-based driver entry point in `crates/eggserve-core/src/server/connection/**` |
| low | `crates/eggserve-core/src/server/http3.rs:68-97` (`project_to_server_config`) still hardcodes the same four values for the H3 adapter | Not a live reachability gap — `crates/eggserve-h3/src/**` contains no reference to the four knobs, so H3 ignores them — but it is a second copy of the projection table | Route it through `config.direct_h1_config()` to delete the second table; fold into the next direct-H1-runtime milestone |
| low | `crates/eggserve-core/src/server/connection/request.rs:366-373` still describes the direct `OriginOrAbsolute` opt-in as "a direct-only forward-proxy seam" | Stale comment: the compatibility H1 path can now opt in (the compatibility H2 path still rejects absolute-form unconditionally) | Comment refresh in the next milestone that touches that file |
| low | The new compatibility types are re-exported at `eggserve_core::server::config::*` but not at `eggserve_core::server::*` | Cosmetic API surface asymmetry | Optional re-export; `server/mod.rs` was outside this milestone's file ownership |
| low | CLI/Python exposure of the four knobs | A CLI or Python consumer still cannot opt in | Explicitly out of scope for Plan 308 §5; a separate plan is required before any CLI-grammar or Python-API change |

No critical or high finding is open. Nothing here blocks the corrective
publication line.

## 11. Roadmap disposition

**Milestone closed and next dependency may proceed.**

The compatibility config now carries the four direct-H1 ownership values
with unchanged defaults, the compatibility builder no longer duplicates
direct setter bodies, the CLI and Python surfaces are untouched, and the
semver impact is classified as a minor transition consistent with the
prepared `0.5.0` line.

## 12. Registry updates

`plans/registry.md`:

- The Direct H1 runtime row for Milestone 308 moves `ready` → `closed`,
  with this closure record linked and the reported findings named.

`plans/subsystems/direct-h1-runtime-roadmap.md`:

- Milestone 14 (Plan 308) moves `ready` → `closed` in §12, with this
  closure record and the two reported follow-ups named.

# Direct H1 Runtime Milestone 307 — Connection-overlap classification accuracy, drift bound, and Alt-Svc single source

Status: ready for handoff

Repository baseline: `465b80cdabf90801e6ef929563b12886c509c201`

Source roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md#milestone-13--connection-overlap-classification-accuracy`

Long-term requirements:

- `plans/000-long-term-specification.md#2` (single H1 authority, single `Service` contract)
- `plans/001-terminology-and-domain-model.md`
- `plans/003-planning-process.md#7-corrective-passes`

Applicable historical contracts:

- `architecture/crate-topology.md` (Plan 253 connection-overlap classification ledger)
- `architecture/eggserve-core.md` (documented direct-vs-compat Date/Server split)
- `docs/public-api-boundary.md` (direct-H1 Date/Server ownership boundary)
- `architecture/policy-system.md` (`H1ConnectionPolicy` Date/Server transfer)
- `release/plan-253-*` lineage via `architecture/crate-topology.md`

Primary class: invariant

## 1. Objective

Make the Plan 253 overlap classification true, and remove the last
undocumented parallel policy in the response-metadata path.

Three defects, one boundary:

1. **The ledger mislabels a pair.** `architecture/crate-topology.md:251`
   classifies `core/src/server/connection/response.rs` as "shared kernel …
   **single direct authority for the kernel**". It is not. The core file
   imports nothing from `eggserve_server` (zero `eggserve_server::`
   occurrences) and is a fully independent implementation whose function
   set is only 2-of-8 identical to the direct copy, with a materially
   different metadata-ownership model. The classification must describe
   what the code actually is.
2. **The gate measures presence, not drift.** `check_plan253_overlap`
   (`scripts/check-crate-topology.py:566`) enforces that each pair still
   exists, that core's parallels stay `pub(crate)`, that a public surface
   allow-list stays frozen, and that H2-only modules stay
   `#[cfg(feature = "http2")]`-gated. It never compares contents, so the
   divergence can grow without limit and without any signal.
3. **The Alt-Svc advertisement policy exists twice.**
   `crates/eggserve-core/src/server/connection/response.rs:241` and
   `crates/eggserve-h3/src/adapter.rs:1294` independently implement
   "remove any `alt-svc`, then re-add `h3=":<port>"; ma=86400` unless H3 is
   disabled, advertisement is off, or the header is denylisted", including
   the same magic literal in two places.

This milestone corrects the classification, adds a machine-checked drift
bound, and collapses the Alt-Svc policy onto one authority — without
merging the two connection kernels and without changing any tier.

## 2. Why this milestone is ready

- The divergence is already measured and documented in this plan's §3, so
  no exploratory work is required.
- The Alt-Svc duplication is a small, self-contained policy that can move
  to a transport-neutral owner without touching the connection runtime.
- Milestone 306 has established the pattern for a single-source
  consolidation with byte-equivalence evidence.
- Hard dependencies (Plan 249 single H1 authority, Plan 253 ledger, Plan
  289 Date/Server ownership, Milestone 306) are closed or satisfied.

## 3. Current implementation evidence

### 3.1 The measured state of the eight classified pairs

Measured at the baseline with `diff -u` on the raw pairs and a
normalized (crate-path-stripped, comment-stripped) similarity score:

| Pair | Direct | Core | diff lines | Similarity | Ledger claim | Verdict |
|---|---|---|---|---|---|---|
| `connection/transport.rs` | 100 | 100 | 0 | 100% | "byte-identical" | accurate |
| `connection/deferred_body.rs` | 127 | 122 | 40 | 100% | "identical modulo the primitives import prefix" | accurate |
| `connection/lifecycle.rs` | 229 | 221 | 76 | 98.6% | "logic identical; visibility/docs differ" | accurate |
| `connection/request.rs` | 595 | 551 | 300 | 88.7% | "substantially identical; `expected_scheme` is the shared H2 hook" | understated — see §3.2 |
| `connection/response.rs` | 842 | 404 | **904** | **30.9%** | "shared kernel … single direct authority for the kernel" | **contradicted** |
| `connection/pipeline.rs` | 1423 | 1402 | 1447 | 75.2% | "shared kernel shape; core carries the H2 classifier…" | understated |
| `connection/activity.rs` | 513 | 582 | 348 | 80.1% | "shared counters/guards/tunnel-drain shape…" | accurate |
| `connection/driver.rs` | 514 | 729 | 661 | 50.7% | "intentionally split: direct drives Hyper H1, core drives Hyper H2…" | accurate |

### 3.2 What the ledger does not mention

- `connection/request.rs`: the two copies differ at feature level, not
  just at the H2 hook. The direct copy carries the Plan 278 absolute-form
  seam (`Http1RequestTargetMode`, `RequestTarget::from_absolute_components`,
  `select_body_policy_with_ceiling`, `Option<usize>` ceilings); the core
  copy has none of these and rejects absolute-form unconditionally. This is
  the *reason* the copies diverge and it is invisible in the ledger.
- `connection/pipeline.rs`: the two copies use different configuration
  shapes (direct: `Arc<H1ConnectionPolicy>` with `Option` semaphores and
  ceilings; core: `Arc<RuntimeConfig>` with mandatory ones), and the
  function sets differ (`finish_invocation_response` is direct-only;
  `classify_tunnel` is core-only).

### 3.3 The `response.rs` pair in detail

`crates/eggserve-core/src/server/connection/response.rs` imports only
`crate::response`, `crate::server::config`, and `crate::server::service`.
It references `eggserve_server` nowhere. Function by function:

| Function | direct | core | State |
|---|---|---|---|
| `normalize_then_convert_with_h1_trailer_head` | `:159` | `:52` | differs |
| `contain_service_panic` | `:221` | `:101` | differs |
| `body_error_to_response` | `:330` | `:126` | differs |
| `service_error_to_response` | `:374` | `:153` | differs |
| `finalize_runtime_response` | `:435` | `:212` | differs |
| `body_error_disposition` | `:357` | `:169` | identical |
| `apply_http1_disposition` | `:403` | `:181` | identical |

Semantic differences:

- The direct copy implements the Plan 289 metadata-ownership model
  (`ResponseProvenance` at `crates/eggserve-server/src/connection/response.rs:430`,
  `PolicyOwner` checks at `:450-468`), so a *successful* service response
  can carry its own `Date`/`Server` when the operator opts in.
- The core copy has no provenance concept. It unconditionally removes
  `Date` and re-inserts a runtime-generated one
  (`crates/eggserve-core/src/server/connection/response.rs:232`) and has no
  `Server` transfer path.
- The direct copy has a typed runtime-rejection taxonomy
  (`present_runtime_rejection`, `crates/eggserve-server/src/connection/response.rs:21`)
  that the core copy lacks.

**This divergence is intentional and already documented** — it is a
direct-H1-only opt-in feature, not a defect:

- `docs/public-api-boundary.md:106` — "Direct H1 can explicitly transfer
  Date/Server ownership for successful service" responses.
- `architecture/eggserve-core.md:269` and `:275` — "Direct H1 has opt-in
  successful service-response Date/Server ownership."
- `architecture/policy-system.md:69` — "`H1ConnectionPolicy` can transfer
  Date and/or Server ownership for successful" responses.

Therefore this milestone must **not** merge the kernels and must **not**
make the compatibility path adopt provenance. It must make the ledger say
what is true.

### 3.4 Dead code inside the classified pairs

- `crates/eggserve-core/src/server/connection/response.rs:23-41`
  `normalize_then_convert` — `#[allow(dead_code)]`, zero callers.
- `crates/eggserve-core/src/server/connection/request.rs:120`
  `wrap_incoming_body` — `#[allow(dead_code)]`, zero callers.
- `crates/eggserve-server/src/connection/lifecycle.rs:68,73,78` and
  `crates/eggserve-core/src/server/connection/lifecycle.rs:68,73,78` — the
  same three accessors are dead in **both** crates.
- `crates/eggserve-server/src/connection/response.rs:126-127`
  `normalize_then_convert_with_provenance` — `#[allow(dead_code)]`, zero
  callers.

`architecture/crate-topology.md:256-259` asserts "every core helper has a
live H2 caller" and that "no removable dead compatibility machinery
remains". At least two core helpers have no caller in any build, so that
sentence overstates.

### 3.5 The duplicated Alt-Svc policy

`crates/eggserve-core/src/server/connection/response.rs:241-259`:

```rust
response.headers_mut().remove("alt-svc");
if config.http3.advertise_alt_svc
    && !policy.stripped_response_headers.iter()
        .any(|name| name.eq_ignore_ascii_case("alt-svc"))
{
    let value = format!("h3=\":{}\"; ma=86400", config.bind.port());
    ...
}
```

`crates/eggserve-h3/src/adapter.rs:1294-1319`:

```rust
headers.retain(|field| !field.name.as_str().eq_ignore_ascii_case("alt-svc"));
if h3_config.advertise_alt_svc
    && !server_config.response_policy.stripped_response_headers.iter()
        .any(|name| name.eq_ignore_ascii_case("alt-svc"))
{
    let _ = headers.push_str("alt-svc", format!("h3=\":{}\"; ma=86400", server_config.bind.port()));
}
```

The `ma=86400` window literal appears twice (`core:254`, `h3:1314`).
Changing the advertisement window or format in one place silently
desynchronizes H1/H2 advertisement from H3 advertisement.

### 3.6 Gate coverage

`python3 scripts/check-crate-topology.py` passes today, and
`python3 scripts/check-crate-topology.py --self-test` passes with 25
checks, including Plan 253 mutations (`plan253-good`, `plan253-core-public`,
`plan253-direct-public`, `plan253-pair-loss`, `plan253-gate-loss`). The
mutations cover presence, privacy, and gating — none covers content drift.

## 4. Invariants that must not regress

- The Plan 253 DEFER conditions remain in force: the classified parallel
  connection helpers stay `pub(crate)`, no new public transport API is
  grown merely to share source, and H2 ownership does not move out of
  core.
- The direct-H1 Date/Server ownership model stays a **direct-only opt-in**
  feature. The compatibility path keeps runtime-owned `Date`/`Server`.
- Runtime error metadata, response framing, and the stripped-header
  denylist stay runtime-owned.
- The Alt-Svc advertisement stays runtime-owned: an application can never
  advertise a different H3 endpoint, and a denylisted `alt-svc` stays
  denylisted.
- H2/H3 tiers stay experimental; no promotion is attempted.
- `eggserve-primitives` stays Hyper/Tokio/Tower-free.
- No wire-visible change: the Alt-Svc header bytes and the `Date`/`Server`
  behavior stay exactly as they are today.
- The topology gate keeps passing, and its `--self-test` mutation suite
  keeps passing with the new drift rule included.
- No published artifact changes without a separate publication milestone.

## 5. Scope

### In scope

- Correct the Plan 253 classification ledger in
  `architecture/crate-topology.md` so each of the eight pairs is described
  accurately, including:
  - `connection/response.rs` reclassified as a **classified parallel
    implementation** (not an adapter, not a shared kernel), with the
    documented direct-vs-compat `Date`/`Server` split named as the
    intentional semantic difference;
  - `connection/request.rs` and `connection/pipeline.rs` drift (the
    absolute-form seam and the configuration-shape difference) recorded;
  - the "no removable dead compatibility machinery remains" sentence
    corrected to match the measured dead helpers;
  - the "facades over the direct authorities (no second envelope,
    taxonomy, normalization, or state machine)" claim qualified to name
    the classified parallel modules explicitly.
- Add a **drift bound** to `check_plan253_overlap`: a recorded per-pair
  similarity/diff ceiling, compared on every gate run, failing when a
  classified pair diverges further. The bound must be set at the current
  measured value with a small, explicit allowance, and the ledger must
  record the value.
- Add a `--self-test` mutation proving the drift rule fires.
- Remove the dead helpers listed in §3.4, or record why each surviving one
  is still required.
- Collapse the Alt-Svc advertisement policy onto one transport-neutral
  authority and call it from both the compatibility H2/H3 path and the H3
  adapter.
- Byte-equivalence tests for the Alt-Svc header across the decision matrix
  (H3 enabled/disabled, advertisement on/off, denylisted/not).

### Explicitly out of scope

- Merging the two `connection/response.rs` kernels, or making the
  compatibility path adopt `ResponseProvenance` / `PolicyOwner`. The
  direct-vs-compat split is documented and intentional.
- Moving H2 ownership out of core.
- Growing a public transport API to share connection source.
- Changing the Alt-Svc advertisement window (`ma=86400`) or format. This
  milestone consolidates the policy; it does not change it.
- Promoting H2 or H3 out of the experimental tier.
- The `RuntimeConfigBuilder` / `RuntimeConfig` shape convergence and the
  ownership-knob reachability question — Milestone 308.
- The accept-error backoff divergence — Milestone 309.
- The runtime error-representation consolidation — Milestone 306.
- Publication.

## 6. Required production changes

### Crates and ownership

Expected owners:

- `eggserve-primitives`: gains the transport-neutral Alt-Svc advertisement
  helper and the max-age constant. Additive public API.
- `eggserve-server`: unchanged code; its `connection/response.rs` is the
  direct kernel and stays as-is.
- `eggserve-core`: `connection/response.rs` and `server/connection/response.rs`
  call the shared Alt-Svc helper instead of formatting the value locally.
- `eggserve-h3`: `adapter.rs::apply_alt_svc` calls the shared helper.
- `eggserve-python`, `eggserve-bin`, `eggserve-static`: unchanged.

No new crate. No ownership boundary moves, so the topology gate's ownership
rules are unaffected.

### Config and policy

No configuration default changes. `advertise_alt_svc`, `http3.enabled`, and
`response_policy.stripped_response_headers` keep their current meaning and
defaults.

### Protocol and compatibility

No wire change. The Alt-Svc header value stays byte-identical:
`h3=":<port>"; ma=86400`.

The classification ledger change is documentation only; it must not imply
that the two connection kernels were merged or that their behavior was
changed.

### Runtime and concurrency

No runtime change. Removing the dead helpers must not remove a live call
path; each removal is verified by the clippy run and the test suite.

### Frontend or operator surface

None.

### Security and confinement

- The Alt-Svc header must remain runtime-owned: the helper removes any
  application-supplied `alt-svc` before deciding, and it must respect the
  denylist.
- The direct-vs-compat `Date`/`Server` split is a privacy-relevant
  difference; the corrected ledger must state which path owns which header
  so that neither is accidentally read as "the runtime always strips
  `Date`".

### Documentation and static guards

- `architecture/crate-topology.md` — the corrected ledger, the recorded
  drift bounds, and the qualified facade claim.
- `architecture/eggserve-core.md` — the direct-vs-compat split, already
  present at `:269`/`:275`; confirm it stays consistent with the ledger.
- `architecture/eggserve-h3.md` — the Alt-Svc advertisement authority.
- `scripts/check-crate-topology.py` — the drift rule and its `--self-test`
  mutation.

## 7. Ordered work packages

### Work package A — Re-measure and record the drift baseline

Recompute the per-pair diff and similarity at the milestone baseline,
store the numbers in a machine-readable form inside the gate script (a
table of pair → ceiling), and record the same numbers in the ledger.

Choose the ceiling so that it is: (a) at or slightly above the current
measurement, (b) tight enough to catch a meaningful new divergence, and
(c) tolerant of comment-only and import-prefix differences.

Acceptance evidence: the gate passes at the baseline with the recorded
ceilings, and the ledger shows the same numbers.

### Work package B — Add the drift rule and its self-test mutation

Extend `check_plan253_overlap` to compare each pair against its ceiling
and fail with a clear message naming the pair, the recorded ceiling, and
the measured value.

Add a `--self-test` mutation that perturbs a classified pair in a synthetic
fixture tree and asserts the gate fails.

Acceptance evidence: `python3 scripts/check-crate-topology.py` passes;
`python3 scripts/check-crate-topology.py --self-test` passes with the new
mutation; the mutation demonstrably fails the rule when applied to the
live tree.

### Work package C — Correct the classification ledger

Rewrite the Plan 253 ledger section so each pair's description matches the
measured reality. Specifically:

- reclassify `connection/response.rs` as a classified parallel
  implementation and name the intentional `Date`/`Server` difference;
- record the `request.rs` absolute-form seam divergence and the
  `pipeline.rs` configuration-shape divergence;
- correct the "no removable dead compatibility machinery remains"
  sentence;
- qualify the "no second ... normalization, or state machine" sentence so
  it names the classified parallel modules instead of asserting none
  exist.

Acceptance evidence: every sentence in the ledger is verifiable against the
source at the baseline.

### Work package D — Remove the dead helpers

Remove the dead helpers in §3.4 that have no caller in any build. For each
surviving helper, record the reason it must stay.

Acceptance evidence: `cargo clippy --workspace --lib --bins --tests -- -D
warnings` is clean with no new `#[allow(dead_code)]`; the ledger's
"removable dead machinery" sentence now matches reality.

### Work package E — Single-source the Alt-Svc advertisement

Add the transport-neutral advertisement helper to `eggserve-primitives`
(the owner of the canonical header model), including the max-age constant
that today appears twice as a literal.

Call it from:

- `crates/eggserve-core/src/server/connection/response.rs` (the H2/H3
  response post-pass), and
- `crates/eggserve-h3/src/adapter.rs::apply_alt_svc`.

The helper takes the port, the advertisement flag, and the denylist
predicate, and returns the header value or "no advertisement". The
header-insertion mechanics stay with each caller (Hyper header vs canonical
header block).

Acceptance evidence: the literal `ma=86400` appears exactly once in the
workspace; both callers use the helper; no caller formats the Alt-Svc
value itself.

### Work package F — Alt-Svc equivalence tests

Add tests covering the decision matrix: H3 enabled/disabled × advertisement
on/off × denylisted/not, for both the compatibility path and the H3
adapter, asserting the exact header bytes and the absence of the header
where it must be absent.

Acceptance evidence: the matrix is green and the expected bytes match the
baseline behavior.

### Work package G — Documentation and registry

Update `architecture/crate-topology.md`, `architecture/eggserve-h3.md`, the
source roadmap, and `plans/registry.md`.

Acceptance evidence: no document asserts that the compatibility path owns
`Date`/`Server` in the same way the direct path does, and no document
asserts that the two connection kernels are shared.

## 8. Failure, cancellation, restart, and contention semantics

- No runtime behavior change; the Alt-Svc helper is a pure computation with
  no state, no await, and no allocation beyond the formatted value.
- Removing dead helpers must not change shutdown, drain, or cancellation
  behavior; the clippy and test runs are the evidence.
- The drift rule is a build-time check; a false positive stops the gate
  rather than degrading the runtime.

## 9. Compatibility and migration

- No consumer-visible change. The Alt-Svc bytes are identical; the
  `Date`/`Server` split is unchanged.
- `eggserve-primitives` gains additive public API. Per the 291 precedent,
  additive public API is a patch bump; the semver classification is
  recorded in the closure record and handled by whichever publication
  milestone follows.
- The ledger correction is documentation-only. If a downstream consumer
  relied on the ledger's "single direct authority for the kernel" wording,
  the corrected wording is the accurate one and the correction is
  recorded in the closure record so the change is traceable.

## 10. Required tests

### Focused unit tests

- Alt-Svc decision matrix (Work package F).
- Alt-Svc helper: denylist matching is case-insensitive; the max-age
  constant is used; the port is taken from the bound address.
- The drift-bound comparison in the gate's `--self-test`.

### Integration tests

- An H2 response through the compatibility path with H3 enabled advertises
  the expected `alt-svc`.
- An H3 response through the adapter advertises the same value.
- An application-supplied `alt-svc` header is removed before the runtime
  decides (application cannot override the advertisement).
- A denylisted `alt-svc` is not emitted.

### Restart and recovery tests

None required.

### Contention and cancellation tests

None required.

### Security and negative tests

- An application cannot inject its own `alt-svc` value.
- The denylist is honored on both paths.
- The advertised port matches the bound port.

### Migration and compatibility tests

- The Alt-Svc header bytes are identical to the pre-change baseline for
  every matrix cell.
- The direct H1 `Date`/`Server` opt-in still works and the compatibility
  path still strips them (no accidental convergence in either direction).

## 11. Required verification commands

```bash
cargo test -p eggserve-primitives --lib
cargo test -p eggserve-server --lib
cargo test -p eggserve-core --features http2,tls --lib
cargo test -p eggserve-core --features http3,tls --lib
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo clippy -p eggserve-core --features http3,tls --lib --tests -- -D warnings
python3 scripts/check-crate-topology.py
python3 scripts/check-crate-topology.py --self-test
python3 scripts/verify-conformance-matrix.py
cargo fmt --all -- --check
./scripts/verify.sh fast
```

## 12. Documentation updates

- `architecture/crate-topology.md` — the corrected ledger, the recorded
  drift bounds, and the qualified facade claim.
- `architecture/eggserve-h3.md` — the Alt-Svc advertisement authority.
- `architecture/eggserve-core.md` — confirm the direct-vs-compat
  `Date`/`Server` wording stays consistent.
- `plans/subsystems/direct-h1-runtime-roadmap.md` — Milestone 13 status.
- `plans/registry.md` — the Milestone 13 row.
- `plans/closure/direct-h1-runtime/307-connection-overlap-classification-accuracy.md`
  (new).

## 13. Acceptance criteria

- Every sentence in the Plan 253 ledger is verifiable against the source.
- `connection/response.rs` is classified as a parallel implementation, and
  the documented intentional `Date`/`Server` split is stated rather than
  glossed.
- The topology gate fails when a classified pair's drift exceeds its
  recorded ceiling, and the `--self-test` mutation proves the rule is
  wired.
- The `ma=86400` literal exists exactly once in the workspace, and both
  Alt-Svc callers use the shared helper.
- The Alt-Svc header bytes are identical to the baseline across the whole
  decision matrix.
- The dead helpers in §3.4 are removed, or each survivor has a recorded
  reason.
- No wire-visible behavior change, no tier change, no ownership move, and
  no public API break.

## 14. Stop conditions

Stop and report rather than improvise if:

- re-measuring shows the drift numbers differ materially from this plan's
  §3 baseline (the baseline was recorded at `465b80c`; a large shift means
  the tree moved and the ceilings must be re-derived deliberately);
- the drift rule produces false positives on unrelated formatting or
  comment changes — the rule must compare semantics, not bytes;
- collapsing the Alt-Svc policy would require `eggserve-primitives` to
  become Hyper-aware or to depend on `eggserve-h3` configuration types;
- any proposal appears that would make the compatibility path adopt
  `ResponseProvenance` / `PolicyOwner`, or would merge the two connection
  kernels — both are out of scope and need a separately scoped plan;
- a proposed ceiling would be so loose that a real regression passes it.

## 15. Closure evidence required

Create `plans/closure/direct-h1-runtime/307-connection-overlap-classification-accuracy.md`
containing:

- the re-measured drift table and the recorded ceilings;
- the gate rule text and the `--self-test` mutation result, including a
  demonstration that the rule fires on a perturbed live tree;
- the corrected ledger text;
- the dead-helper removals with reasoning for anything retained;
- the Alt-Svc equivalence matrix results and the before/after byte
  comparison;
- the `eggserve-primitives` additive API and its semver classification;
- the verification command/outcome table;
- residual findings and the roadmap/registry disposition.

## 16. Handoff notes

The tempting move here is to merge the two connection kernels because they
look like duplicates. Do not. The divergence is deliberate and documented:
the direct path supports opt-in service-response `Date`/`Server` ownership
and typed runtime rejections; the compatibility path does not and is not
required to. This milestone makes the classification true and stops the
divergence from growing — it does not close the gap.

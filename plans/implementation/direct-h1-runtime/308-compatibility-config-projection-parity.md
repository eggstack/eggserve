# Direct H1 Runtime Milestone 308 — Compatibility config projection parity and builder convergence

Status: ready for handoff

Repository baseline: `465b80cdabf90801e6ef929563b12886c509c201`

Source roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md#milestone-14--compatibility-config-projection-parity`

Long-term requirements:

- `plans/000-long-term-specification.md#2` (single H1 authority, single `Service` contract)
- `plans/001-terminology-and-domain-model.md` (`RuntimeConfig`, `H1ConnectionPolicy`, `PolicyOwner`)
- `plans/003-planning-process.md#7-corrective-passes`

Applicable historical contracts:

- `architecture/configuration.md` (ownership knobs and their projections)
- `architecture/eggserve-core.md` (compatibility projection role)
- `architecture/policy-system.md` (external ownership seams)
- `docs/public-api-boundary.md`
- `architecture/crate-topology.md` (Plan 215/225 facade authority; Plan 253 ledger)

Primary class: capability

## 1. Objective

Stop the compatibility `RuntimeConfig` from silently discarding the direct
H1 ownership knobs, and stop the two `RuntimeConfigBuilder`
implementations from drifting apart.

Today `crates/eggserve-core/src/server/config/runtime.rs:254-283`
`direct_h1_config()` hardcodes four direct-crate capabilities to their
default or disabled values:

```rust
http1_request_target_mode: eggserve_server::Http1RequestTargetMode::OriginOnly,
policy_ownership: eggserve_server::H1PolicyOwnership::default(),
admission_ownership: eggserve_server::AdmissionOwnership::default(),
runtime_rejection_presenter: None,
```

Because the compatibility `RuntimeConfig` has no fields to carry these
values, **every `eggserve-core` consumer — including the shipped CLI and
the shipped Python wheel — is pinned to them**, and there is no way for an
embedder using `eggserve_core::server` to reach the Plan 278 absolute-form
seam or the Plan 289 service-response `Date`/`Server` ownership that the
direct crate exposes.

Separately, `crates/eggserve-server/src/config.rs:370+` and
`crates/eggserve-core/src/server/config.rs:83+` carry two independent
`RuntimeConfigBuilder` implementations whose setter bodies are, for 28 of
the shared methods, byte-identical after path normalization.

This milestone makes the knobs reachable through the compatibility config
with **defaults unchanged**, and converges the two builders onto one
implementation.

## 2. Why this milestone is ready

- The direct crate already owns the authority and the knobs; nothing has
  to be designed.
- The defaults are already correct and safe. The change is additive
  reachability, not a behavior change.
- Milestone 306 and 307 have established the pattern for compatibility
  consolidation without observable change.
- Hard dependencies (Plan 278 absolute-form seam, Plan 280–283 ownership
  seams, Plan 289 metadata ownership, Milestones 306/307) are closed.

## 3. Current implementation evidence

### 3.1 The four hardcoded values

`crates/eggserve-core/src/server/config/runtime.rs:254-283`:

| Direct field | Value | Direct default | Consumer effect |
|---|---|---|---|
| `http1_request_target_mode` | `OriginOnly` | `OriginOnly` | absolute-form request targets rejected |
| `policy_ownership` | `H1PolicyOwnership::default()` | default | external `Date`/`Server` ownership unavailable |
| `admission_ownership` | `AdmissionOwnership::default()` | default | external admission control unavailable |
| `runtime_rejection_presenter` | `None` | `None` | typed runtime rejection presentation unavailable |

The direct crate's own builder defaults are the same values
(`crates/eggserve-server/src/config.rs:226-240`), so the projection is
currently *equivalent to the default* — which is why nothing looks broken.
The defect is that the compatibility consumer cannot choose otherwise.

`crates/eggserve-server/src/config.rs` also exposes
`with_response_metadata_ownership` (`:358`) and
`response_metadata_ownership` (`:212`, `pub(crate)`), which the
compatibility projection does not carry at all.

### 3.2 The two builders

`crates/eggserve-server/src/config.rs:370+` (`RuntimeConfigBuilder`) and
`crates/eggserve-core/src/server/config.rs:83+` (also
`RuntimeConfigBuilder`).

- Direct: 38 methods. Core: 39 methods.
- 34 methods are common to both. **28 of them have byte-identical bodies**
  after crate-path normalization (roughly 3,100 characters of duplicated
  setter logic).
- 6 differ: `build`, `date_policy`, `error_policy`, `response_policy`,
  `trusted_proxy`, `trusted_proxy_peer`.
- Direct-only: `admission_ownership`, `http1_request_target_mode`,
  `policy_ownership`, `runtime_rejection_presenter`.
- Core-only: `http2`, `http3`, `tls_config`, `tls_expose_peer_chain`,
  `tls_reload_handle`.

The core-only methods are legitimate: TLS/H2/H3 orchestration is
compatibility-owned.

### 3.3 Consumers affected

- The CLI: `crates/eggserve-bin/src/lib.rs` starts the server through
  `eggserve_core::server::Server`, so it inherits the hardcoded projection.
- The Python wheel: `crates/eggserve-python/src/server/runtime.rs` builds
  `eggserve_core::server::config::RuntimeConfig`, so it inherits the
  hardcoded projection.
- Any Rust embedder that uses `eggserve_core::server` inherits it.

### 3.4 What the documentation already says

The direct-only framing is documented and must be preserved:

- `architecture/eggserve-core.md:275` — "Direct H1 may opt into
  aggregate-header ownership and successful service-response Date/Server
  ownership through `H1ConnectionPolicy`".
- `docs/public-api-boundary.md:106` — "Direct H1 can explicitly transfer
  Date/Server ownership for successful service" responses.
- `architecture/configuration.md:135` — the `H1ConnectionPolicy` projection
  table.

So the *feature* is documented as a direct-H1 seam. What is **not**
documented anywhere is that a compatibility consumer is silently pinned to
the default and has no way to opt in.

### 3.5 Structural difference already tolerated in the request path

The two `connection/request.rs` copies differ in absolute-form support:
the direct copy has `Http1RequestTargetMode`,
`RequestTarget::from_absolute_components`, and `Option<usize>` ceilings;
the core copy rejects absolute-form unconditionally
(`crates/eggserve-core/src/server/connection/request.rs:377`).

**Important consequence:** the compatibility H1 path delegates to the
direct server, so the direct server's `http1_request_target_mode` *is* the
behavior for compatibility H1 traffic. The core copy's own request module
serves the compatibility H2 path, where absolute-form is not applicable.
Any plan that plumbs `http1_request_target_mode` through core must
therefore confirm that the value reaches the direct driver and that the
compatibility H2 path is unaffected.

## 4. Invariants that must not regress

- **Defaults do not change.** With no configuration, every observable
  behavior is exactly what it is at the baseline: origin-only request
  targets, EggServe-owned `Date`/`Server`, EggServe-owned admission, and no
  rejection presenter.
- Static remains origin-only. `eggserve-static` continues to reject
  absolute-form regardless of any configuration; the Plan 278 seam is an
  H1-embedding seam, not a static one.
- The CLI grammar `[OPTIONS] [PORT] [DIRECTORY]` is unchanged by this
  milestone.
- The Python facade's public surface is unchanged by this milestone.
- `eggserve-static` stays the sole static/path/filesystem authority;
  `eggserve-server` stays the sole H1 runtime owner.
- The `eggserve_core::layers` module keeps exposing the direct crates for
  migration.
- No published artifact changes without a separate publication milestone.
- H2/H3 tiers stay experimental.
- `eggserve-primitives` stays transport-neutral.
- The topology gate keeps passing, and its `--self-test` mutation suite
  keeps passing.

## 5. Scope

### In scope

- Extend the compatibility `RuntimeConfig` (in
  `crates/eggserve-core/src/server/config/runtime.rs`) so it can carry the
  four ownership values, defaulting to exactly today's hardcoded values.
- Make `direct_h1_config()` project the carried values instead of
  hardcoding them.
- Plumb the values through the compatibility `RuntimeConfigBuilder` with
  the same names and semantics the direct builder uses.
- Converge the duplicated builder setters: the compatibility builder
  delegates the 28 identical setters to the direct builder rather than
  re-implementing them, and the 6 that genuinely differ are re-derived
  explicitly rather than copied.
- Reachability evidence: a test that configures each knob through the
  compatibility path and observes the effect.
- Default-preservation evidence: a test that the unconfigured
  compatibility path behaves identically to the baseline.
- A topology-gate rule (or an equivalent guard) that prevents the two
  builders from re-diverging.
- Documentation that states the compatibility-vs-direct ownership reach
  matrix.

### Explicitly out of scope

- **No CLI flag and no Python option is added.** The CLI and the Python
  facade keep their current surfaces and their current defaults. If the
  maintainer wants those surfaces to expose the knobs, that is a separate
  plan with its own compatibility and grammar analysis (see §14).
- No change to any default value.
- No change to the direct crate's public API, except where the builder
  convergence requires exposing an existing value through an existing
  accessor.
- No change to the `H1ConnectionPolicy` projection contract.
- No change to `eggserve-static`'s origin-only rule.
- Merging the two `connection/request.rs` or `connection/pipeline.rs`
  kernels — Milestone 307.
- The runtime error-representation consolidation — Milestone 306.
- The accept-error backoff divergence — Milestone 309.
- Publication.

## 6. Required production changes

### Crates and ownership

Expected owners:

- `eggserve-server`: no production change expected. If the compatibility
  builder needs to read a builder value the direct crate currently keeps
  private, expose it through the existing accessor pattern (a `pub(crate)`
  field plus a `pub` accessor, or a `pub fn` on the builder) rather than
  by widening a field's visibility.
- `eggserve-core`: `server/config/runtime.rs` gains the four carried
  values and the projection change; `server/config.rs` builder delegates
  to the direct builder for the shared setters.
- `eggserve-static`, `eggserve-h3`, `eggserve-python`, `eggserve-bin`,
  `eggserve-primitives`: unchanged.

No new crate. No ownership boundary moves.

### Config and policy

The compatibility `RuntimeConfig` gains:

- `http1_request_target_mode` (default `OriginOnly`),
- `policy_ownership` (default `H1PolicyOwnership::default()`),
- `admission_ownership` (default `AdmissionOwnership::default()`),
- `runtime_rejection_presenter` (default `None`),

with the same types and the same default values the direct crate uses, so
that the projection is value-identical to today's hardcoding.

The compatibility builder gains the matching setters
(`http1_request_target_mode`, `policy_ownership`, `admission_ownership`,
`runtime_rejection_presenter`) with the direct crate's names and
semantics.

**Defaults must be provably identical**: a test constructs a default
compatibility `RuntimeConfig` and a default direct `RuntimeConfig`, runs
both through their projections, and asserts the resulting policies are
equal.

### Protocol and compatibility

- Absolute-form request targets remain rejected under the default
  (`OriginOnly`). The compatibility H2 path is unaffected by the mode.
- External `Date`/`Server` ownership remains unavailable under the
  default. Runtime error metadata, response framing, and the
  stripped-header denylist remain runtime-owned in every configuration.
- External admission ownership remains unavailable under the default.
- The builder convergence must not change any setter's semantics,
  including validation, clamping, and the `Option`/`None` handling of
  `max_connections`, `max_file_streams`, and the timeouts.

### Runtime and concurrency

No runtime change to the direct crate. The compatibility path's projection
is the only runtime-visible change, and it is value-identical under
defaults.

### Frontend or operator surface

None. The CLI and Python facade are untouched and behave exactly as they
do at the baseline.

### Security and confinement

- The knobs are ownership seams. Enabling them from a compatibility
  consumer must not weaken any default. Specifically:
  - external `Date`/`Server` ownership applies only to **successful**
    service responses; runtime errors stay under `ResponsePolicy`;
  - external admission ownership is opt-in and bounded by the caller's own
    policy;
  - the absolute-form seam must not let a static-served path accept
    absolute-form.
- `eggserve-static` remains origin-only and continues to reject
  absolute-form.

### Documentation and static guards

- `architecture/configuration.md` — the compatibility-vs-direct reach
  matrix for the four knobs.
- `architecture/eggserve-core.md` — the projection now carries the values
  rather than hardcoding them.
- `docs/public-api-boundary.md` — confirm the direct-H1 framing still
  matches the implemented reachability.
- `scripts/check-crate-topology.py` — a guard against builder re-divergence.

## 7. Ordered work packages

### Work package A — Capture the default-behavior baseline

Before changing production code, capture the observable baseline for the
compatibility path: the default `RuntimeConfig`'s projected policies, the
rejection of an absolute-form request target, the `Date`/`Server` headers
on a successful response, and the admission behavior.

Store the capture as an expected-value fixture.

Acceptance evidence: a committed baseline capture or an inline
expected-value table.

### Work package B — Carry the ownership values in the compatibility config

Add the four fields with the direct crate's exact defaults, and change
`direct_h1_config()` to project the carried values.

Acceptance evidence: the default projection is value-identical to the
baseline capture (Work package A test).

### Work package C — Plumb the setters through the compatibility builder

Add the four setters to the compatibility builder, delegating to the
direct builder where the direct builder already has the setter.

Acceptance evidence: each setter is reachable through
`eggserve_core::server::config::RuntimeConfigBuilder` and takes effect in
the projected direct config.

### Work package D — Converge the duplicated builder setters

Replace the 28 duplicated setter bodies in the compatibility builder with
delegation to the direct builder's setters.

For the 6 that genuinely differ (`build`, `date_policy`, `error_policy`,
`response_policy`, `trusted_proxy`, `trusted_proxy_peer`), either
delegate them too — if the direct builder offers an equivalent — or keep
them explicit in core with a recorded reason for the difference.

Do not delete a setter whose core-only variant has different semantics.

Acceptance evidence: the compatibility builder contains no setter body
that duplicates a direct setter body; a guard proves the duplication does
not return.

### Work package E — Guard against builder re-divergence

Add a topology-gate rule (or an equivalent static check) that fails when a
compatibility builder setter re-implements a direct builder setter body
instead of delegating. Include a `--self-test` mutation.

The rule must compare normalized bodies so that a comment-only or
path-only difference does not trip it.

Acceptance evidence: `python3 scripts/check-crate-topology.py` passes;
`python3 scripts/check-crate-topology.py --self-test` passes with the new
mutation included.

### Work package F — Reachability and default-preservation tests

Add tests that:

1. Default compatibility path: absolute-form rejected; `Date`/`Server` are
   runtime-owned; admission is EggServe-owned; no rejection presenter —
   matching the baseline capture exactly.
2. `http1_request_target_mode = OriginOrAbsolute` through the
   compatibility path: an absolute-form request target is dispatched
   through the direct H1 driver as origin-form, and the compatibility H2
   path is unaffected.
3. `policy_ownership` configured for external `Date`/`Server`: a
   *successful* service response may carry its own `Date`/`Server`; a
   runtime error still carries runtime-owned metadata.
4. `admission_ownership` configured: the caller's admission decision is
   honored.
5. `runtime_rejection_presenter` configured: the presenter is invoked for
   a typed runtime rejection.

Acceptance evidence: all five tests are green and the default-preservation
test matches the baseline byte for byte.

### Work package G — Documentation

Write the compatibility-vs-direct reach matrix into
`architecture/configuration.md` and update `architecture/eggserve-core.md`
and `docs/public-api-boundary.md` to match the implemented reachability.

Acceptance evidence: no document claims the knobs are unreachable from
core, and none claims the CLI or the Python facade exposes them.

## 8. Failure, cancellation, restart, and contention semantics

- No change under defaults.
- With external admission ownership enabled, admission behavior is the
  caller's policy; the runtime's own ceilings still apply where the
  ownership contract says they do.
- With external `Date`/`Server` ownership enabled, a failure after
  commitment still closes or truncates the connection rather than
  synthesizing a second response; runtime errors still use runtime-owned
  metadata.
- No new lock, task, or channel is introduced on the projection path.
- The builder convergence must not change any validation error, panic, or
  clamping behavior of a setter.

## 9. Compatibility and migration

- **Defaults are unchanged**, so no existing consumer's behavior changes.
  This is the central acceptance criterion.
- The compatibility `RuntimeConfig` gains public fields. A consumer that
  constructs it with an exhaustive struct literal will break at compile
  time — the same class of change recorded in Plan 285 for the direct
  crate. Classify the semver accordingly and record it in
  `docs/migration-guide.md`.
- The compatibility builder gains setters — additive.
- The builder convergence is internal; it must not change any public
  builder method signature.

## 10. Required tests

### Focused unit tests

- Default compatibility config projection equals the default direct config
  projection.
- Each of the four setters round-trips through the compatibility builder
  into the projected direct config.
- The six non-duplicated setters preserve their exact semantics.

### Integration tests

- The default compatibility server rejects an absolute-form request target.
- A compatibility server configured for `OriginOrAbsolute` accepts it.
- A successful response under the default carries EggServe-owned `Date` and
  `Server`.
- A successful response under external ownership may carry the service's own
  `Date`/`Server`; a runtime error does not.
- The typed rejection presenter is invoked when configured.

### Restart and recovery tests

None required; no lifecycle change.

### Contention and cancellation tests

- External admission ownership under concurrent load behaves as the
  caller's policy specifies.

### Security and negative tests

- Runtime error responses keep runtime-owned metadata under every ownership
  configuration.
- The stripped-header denylist is honored under every ownership
  configuration.
- Absolute-form remains rejected for static-served paths regardless of the
  H1 request-target mode.
- A malformed external `Date` still fails closed to a generic 500, per the
  documented `H1ConnectionPolicy` contract.

### Migration and compatibility tests

- The default compatibility path's wire output is identical to the baseline
  capture.
- Existing consumers that construct the compatibility `RuntimeConfig` with a
  struct literal are documented as needing the new fields, and the
  migration guide records it.

## 11. Required verification commands

```bash
cargo test -p eggserve-core --lib
cargo test -p eggserve-core --features http2,tls --lib
cargo test -p eggserve-server --lib
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo clippy -p eggserve-core --features http2,tls --lib --tests -- -D warnings
python3 scripts/check-crate-topology.py
python3 scripts/check-crate-topology.py --self-test
python3 scripts/verify-conformance-matrix.py
cargo fmt --all -- --check
./scripts/verify.sh fast
```

## 12. Documentation updates

- `architecture/configuration.md` — the compatibility-vs-direct reach
  matrix.
- `architecture/eggserve-core.md` — the projection carries values rather
  than hardcoding them.
- `docs/public-api-boundary.md` — the implemented reachability.
- `docs/migration-guide.md` — the exhaustive-struct-literal semver note.
- `plans/subsystems/direct-h1-runtime-roadmap.md` — Milestone 14 status.
- `plans/registry.md` — the Milestone 14 row.
- `plans/closure/direct-h1-runtime/308-compatibility-config-projection-parity.md`
  (new).

## 13. Acceptance criteria

- A compatibility consumer can set each of the four ownership values and
  observe the effect, through the compatibility config and builder.
- With no configuration, every observable behavior is identical to the
  baseline capture, byte for byte.
- The compatibility builder contains no setter body that duplicates a
  direct builder setter body; the gate fails if one reappears.
- The CLI and the Python facade are unchanged and behave exactly as at the
  baseline.
- `architecture/configuration.md` states the reach matrix, and no document
  claims the knobs are unreachable from core.
- The semver impact of the added public fields is classified and recorded.
- No ownership move, no tier change, and no wire-visible default change.

## 14. Stop conditions

Stop and report rather than improvise if:

- making the knobs reachable would require changing any default value;
- the compatibility builder cannot delegate a setter without changing its
  semantics — keep the duplication and record why, rather than forcing a
  delegation;
- plumbing `http1_request_target_mode` turns out to require the
  compatibility H2 request path to change absolute-form handling — that is
  a separate concern and must not be smuggled in;
- the maintainer wants the CLI or the Python facade to expose these knobs.
  That crosses into a CLI-grammar and Python-API surface change and must be
  planned separately (see §5) rather than folded in here. Report the
  request and stop.
- the semver impact of the added compatibility-config fields would require
  a major version — re-classify and escalate rather than proceeding.

## 15. Closure evidence required

Create `plans/closure/direct-h1-runtime/308-compatibility-config-projection-parity.md`
containing:

- the baseline capture and the post-change default comparison;
- the five reachability/default-preservation test results;
- the builder-convergence diff summary and the guard rule text with its
  `--self-test` mutation result;
- the semver classification and the migration-guide entry;
- confirmation that the CLI and Python surfaces are untouched;
- the verification command/outcome table;
- residual findings and the roadmap/registry disposition.

## 16. Handoff notes

The hard requirement is "no observable change under defaults". The
reachability is additive; the temptation is to also flip a default or add
a CLI flag, and both are out of scope. If a consumer asks for the knobs on
the CLI or in Python, that is a good request with its own plan — capture it
in the closure record as a follow-up candidate and stop at the Rust
boundary.

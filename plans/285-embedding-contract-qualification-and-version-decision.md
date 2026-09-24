# Plan 285 — Embedding-contract qualification and version decision

## Status

**LOCAL QUALIFICATION COMPLETE; hosted CI and final artifact closure pending. Version decision: 0.3.0 for incompatible public API changes.**

Plan 279 source qualification is complete. Per maintainer direction, its
registry-only closure is consolidated into Plan 286 with the pending Plan 277
publication.

## Purpose

Qualify the combined direct-H1 embedding contract created by Plans 280–283,
consume the final tunnel decision from Plan 284, prove that hardened defaults
remain unchanged, and determine the correct release version from actual API
compatibility evidence.

This plan does not publish crates. Plan 286 owns publication.

## Qualification target

The target is a generic policy-owning host:

```text
caller-owned TCP listener
  -> caller-owned TLS handshake / ALPN http/1.1
  -> established TlsStream
  -> eggserve-server caller-owned H1 driver
       mandatory parser/framing authority
       selected external deadline/limit ownership
       selected external service/tunnel admission
       custom runtime-rejection presentation
  -> canonical Service
```

The fixture is intentionally gateway/application-server shaped, but must not
import code or configuration from any downstream project.

## Track A — Combined leaf-only fixture

Add a standalone fixture under `release/fixtures/` or the established
downstream-embedding fixture area.

Its EggServe dependency graph must use:

- `eggserve-server`;
- `eggserve-primitives` only if the public server API does not re-export the
  needed types;
- optional `tower` only in a separate Tower variant.

It must not depend on:

- `eggserve-core`;
- `eggserve-static`;
- PHF/static MIME packages;
- EggServe TLS compatibility orchestration.

The fixture may use ordinary downstream `rustls` / `tokio-rustls` to prove
caller-owned encrypted streams.

## Track B — Real caller-owned TLS-H1 proof

Use a local self-signed test certificate and a client that trusts only that
test root/leaf.

Required proof:

1. TCP connect;
2. real Rustls handshake;
3. ALPN `http/1.1` asserted on both ends;
4. server passes the resulting `TlsStream` directly into the new direct H1
   policy API;
5. normal request/response succeeds;
6. no EggServe TLS/listener compatibility layer is involved.

This proves the API works at the exact post-handshake transport seam expected
of embedders.

## Track C — Default regression fixture

Run the same direct runtime with no advanced ownership overrides.

Prove the current secure/default behavior remains:

- finite handler timeout;
- finite body timeout;
- finite idle keep-alive timeout;
- finite response-write no-progress timeout;
- global request-body ceiling;
- semantic request-target ceiling;
- bounded service admission;
- bounded tunnel admission;
- mandatory parser buffer/header count/header timeout;
- default runtime error representation;
- shutdown/drain behavior.

Existing compatibility/static tests remain authoritative too; this fixture is
a concise direct-path regression, not a replacement.

## Track D — External policy ownership fixture

Enable the Plan-280 external policies one at a time and together.

Required behavioral proof:

- handler executes beyond the configured EggServe handler duration when
  handler ownership is External;
- request body may remain active beyond the configured EggServe body duration
  when body-deadline ownership is External;
- a body larger than `max_request_body_bytes` succeeds when the global body
  ceiling is External **and** the service's request-body policy allows it;
- the same path still rejects above the service-selected body limit;
- an idle keep-alive connection survives past the configured EggServe idle
  timeout when idle ownership is External;
- write no-progress is not terminated by EggServe when write ownership is
  External;
- semantic request-target ceiling is not applied by EggServe when External,
  while parser/authority/target-form validation still applies;
- enabled `connection_total_timeout` still wins as the hard connection
  ceiling;
- disabled connection-total mode still composes;
- caller shutdown still terminates all work.

All test-level waits must have independent bounded harness deadlines.

## Track E — External admission fixture

Prove Plan 281 in the combined runtime:

- concurrent service calls can exceed configured
  `max_in_flight_requests` without an EggServe-generated admission 503 when
  service admission is External;
- a host/service-owned semaphore can still bound/reject work deterministically;
- accepted tunnels can exceed configured `max_active_tunnels` without an
  EggServe admission 503 when tunnel admission is External;
- host/service-owned tunnel admission still works;
- no internal saturation counters increment for externally-owned decisions;
- tunnel tasks remain tracked and drain on shutdown.

Default/internal admission is tested in Track C.

## Track F — Runtime rejection presentation fixture

Install a custom Plan-283 presenter.

Prove:

- a runtime-selected rejection status is unchanged;
- custom safe body/header metadata is emitted;
- forbidden/framing headers cannot override EggServe framing;
- response privacy policy remains final authority;
- presenter panic or invalid output falls back safely;
- lifecycle close/keep-alive consequences are unchanged.

Use at least 414/431, 413 or body rejection, 503 admission, and 504 handler
timeout where those categories are owned by the runtime under the tested
configuration.

## Track G — Tunnel decision integration

If Plan 284 is KEEP:

- run its retained direct-transport tunnel implementation through this fixture;
- include immediate post-upgrade read-ahead bytes;
- include shutdown of an idle handler;
- record retained performance/resource evidence reference.

If Plan 284 is NO-GO:

- run the existing duplex bridge through the same correctness cases;
- record the NO-GO evidence reference;
- do not reopen the optimization.

Plan 285 cares about the final supported contract, not whether optimization won.

## Track H — Tower/http-interop composition

Run a second fixture with
`eggserve-server = { ..., features = ["tower"] }`.

Prove that external policy/admission ownership and runtime rejection
presentation do not require `eggserve-core` and do not break:

- incremental request body;
- response streaming;
- trailers;
- duplicate headers;
- lifecycle cancellation;
- Axum 0.8 composition already qualified by Plans 274–277.

Do not force tunnel one-shot capabilities into clonable `http::Extensions`;
native tunnel-aware service remains authoritative.

## Track I — Dependency/package footprint

Record for the native and Tower fixtures:

- `cargo tree -e no-dev`;
- `cargo metadata --format-version 1`;
- package count;
- executable size under the repository's standard release/dist profile;
- confirmation that direct path has no core/static/PHF ancestry.

Compare to the post-277 direct adapter baseline where available.

Do not claim general performance from package-size measurements.

## Track J — Runtime overhead qualification

Measure a focused same-machine H1 A/B:

- legacy/default direct driver before the program;
- new driver using default ownership;
- new driver using external ownership with no host work.

Use small-request throughput and tail latency plus one streaming response case.

Acceptance target is primarily "no material regression." Any consistent
regression must be explained and either corrected or explicitly accepted with
evidence before publication.

Plan 284 owns tunnel-specific performance.

## Track K — API compatibility/version decision

Run source/API compatibility tooling available in the repo
(`cargo-semver-checks` if installed by the repository tooling, package API
diffs, compile fixtures).

Decision:

- if the retained API is additive/source-compatible under the current 0.2
  contract, select the next unused 0.2.x patch at Plan 286 execution time;
- if implementation requires removing/changing existing public fields,
  constructors, trait methods without defaults, or other incompatible source
  changes, do **not** hide that in a patch. Record a 0.3.0 requirement.

Adding new optional types/functions/builders should be preferred so a 0.2.x
patch remains possible.

Do not hard-code a version number in this plan; Plans 278–279 may consume the
next patch first.

## Track L — Full verification

At minimum:

```bash
cargo fmt --all -- --check
cargo +1.89 check --workspace --all-targets
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo test -p eggserve-server
cargo test -p eggserve-server --features http-interop
cargo test -p eggserve-server --features tower
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
bash scripts/install-cargo-tools.sh
bash scripts/check-supply-chain.sh
bash scripts/verify-cargo-packages.sh --mode all
```

Run current Python/core/static/H2/H3 lanes required by repository CI; this
program must not regress them even though they are not the feature target.

Require hosted CI green for the proof-bearing implementation SHA.

## Evidence artifact

Create:

`release/plan-285-embedding-contract-qualification.md`

Record:

- implementation SHAs for Plans 280–284;
- final Plan-284 decision;
- exact public API additions;
- default-regression matrix;
- real TLS-H1 embedding matrix;
- external policy/admission matrix;
- presenter matrix;
- dependency graphs;
- runtime/package measurements;
- local verification;
- hosted CI run;
- version compatibility decision;
- any accepted residuals.

## Acceptance criteria

- [ ] Plans 280–283 implemented and locally qualified.
- [ ] Plan 284 has a final KEEP/NO-GO decision.
- [ ] real caller-owned TLS-H1 fixture passes.
- [ ] hardened default behavior remains unchanged.
- [ ] all selected external policy modes behave exactly as documented.
- [ ] external admission removes duplicate EggServe admission without
      weakening shutdown/lifecycle.
- [ ] custom rejection presentation cannot change status/framing/lifecycle.
- [ ] direct native and Tower graphs exclude core/static/PHF.
- [ ] caller-owned path contains no raw Hyper public API.
- [ ] combined path has no material unexplained performance regression.
- [ ] API compatibility is measured and release version class is recorded.
- [ ] full local gates and hosted CI pass.
- [ ] durable qualification evidence exists.

## Non-goals

- No crates.io publication.
- No downstream repository modification.
- No H2/H3 support-tier promotion.
- No static/Python feature work beyond regression verification.

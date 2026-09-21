# Plan 256 — Post-convergence maintenance and interop fidelity qualification/closure

## Purpose

Close Plans 251–255 with evidence that the maintenance work corrected the
review findings without changing EggServe's public Rust/Python API, capability
set, security invariants, protocol support tiers, package topology, or
wire-visible behavior.

Planning baseline for the campaign:

```text
0ee02acd69f1c63d32134f8265283fff04e4630c
```

Baseline remote CI:

```text
run 35620987177 — success
```

Plan 256 is an evidence/closure gate. It does not authorize unrelated
production work. If qualification exposes a new defect outside Plans 252–255,
open a separate corrective plan rather than absorbing it here.

## Preconditions

Required before entering closure:

- Plan 252 Python typing/public-surface fidelity work complete;
- Plan 253 connection-overlap classification/convergence complete, including
  explicit DEFER records for any overlap that cannot safely be shared under the
  frozen API/package constraints;
- Plan 254 async-Python lifecycle/stream parity hardening complete;
- Plan 255 migration-residue/module/topology-checker cleanup complete or each
  optional large-module decomposition candidate explicitly closed NO-CHANGE
  with rationale;
- no open regression introduced by those plans.

## Closure artifact

Create a durable release record, preferably:

```text
release/plan-256-post-convergence-maintenance-interop-closure.md
```

Record:

- campaign baseline SHA;
- each implementation-plan landing SHA;
- final evidence candidate SHA;
- exact Rust/Python/mypy/maturin toolchain versions;
- public API/type evidence;
- connection-overlap classification summary;
- async-Python parity results;
- topology-checker rule/self-test results;
- package/dependency graph results;
- local command/results summary;
- exact remote CI run ID/URL/conclusions;
- any later metadata-only documentation SHA separately.

Do not claim a metadata-only SHA was independently qualified when CI ran on an
earlier candidate.

## Track A — Python public typing fidelity closure

Against a built and installed wheel, prove the corrected type surface matches
runtime behavior.

### Static typing

Run the strict installed-wheel typing fixture and ensure it covers:

- `AsyncRequest.headers`;
- `*_addr` versus `*_address`;
- proxy source/destination;
- query representation;
- body/lifecycle/tunnel properties;
- sync and async response constructors;
- `BaseHTTPRequestHandler` logging override hooks;
- `HTTPServer.server_bind` / `server_activate` override hooks.

Record checker version and result.

### Runtime shape

Run focused runtime assertions for:

- no-query/query values;
- text/tuple address forms;
- trusted PROXY metadata;
- header dictionary and duplicate-preserving item views.

### Package composition

Verify the installed wheel contains `py.typed` and every intended `.pyi`
artifact.

No public runtime representation may have been changed solely to satisfy
typing.

## Track B — Rust public API/source compatibility

Compile the existing public-path fixtures and representative downstream usage
for:

- `eggserve-primitives`;
- `eggserve-server`;
- `eggserve-static`;
- `eggserve-core::primitives`;
- `eggserve-core::server`;
- optional `http-interop` / Tower paths;
- current TLS/H2/H3 feature-gated compatibility paths.

Retain compile-time type identity checks for compatibility re-exports where
identity is part of the contract.

If public-api tooling is already available, capture a baseline/candidate diff;
otherwise use the established explicit fixture inventory. Do not add a heavy
production dependency for API diffing.

Any prohibited source break blocks closure.

## Track C — connection authority and overlap closure

Record the final Plan 253 overlap ledger.

For each previously parallel core/server connection responsibility, record one
of:

- direct authority/delegated;
- core H2-specific;
- compatibility/composition adapter;
- accepted bounded duplication with rationale and drift guard.

Prove mechanically:

- `eggserve-server` remains the only executable H1 Hyper authority;
- core cannot execute Hyper H1;
- Auto/explicit/TLS/PROXY/Unix H1 delegate to direct authority;
- core H2 execution remains feature-gated and unchanged in capability;
- direct `http2`/`tls` compatibility feature names remain inert;
- no new internal-sharing crate/public transport API was introduced merely to
  deduplicate source.

If Plan 253 retained exact/protocol-neutral duplication under DEFER, closure
must treat it as an explicit architectural constraint rather than falsely
claiming one physical copy.

## Track D — H1/H2 wire and semantic parity

Run representative behavior through direct and compatibility construction.

H1 cases:

- cleartext GET/HEAD;
- request body reject/buffer/stream;
- response bytes/stream/trailers;
- interim responses;
- response privacy/errors;
- keep-alive/max requests;
- tunnel accept/deny;
- prebound listener;
- caller-owned stream;
- shutdown/drain;
- response write stall;
- peer disconnect.

Composition cases:

- PROXY-prefixed H1;
- Unix H1 on Unix;
- TLS ALPN H1;
- H2 prior knowledge;
- TLS ALPN H2;
- trusted forwarded metadata.

Any Plan 253 change to canonical conversion must also pass the full canonical
and wire-correctness suites.

## Track E — async-Python lifecycle/resource closure

Run the deterministic Plan 254 matrix and record:

- application permit count/ownership before/during/after buffered handlers;
- permit transfer/release for streaming responses;
- producer task count before/after repeated requests;
- handler timeout races;
- stream no-progress/backpressure cases;
- producer error/drop/cancel paths;
- HEAD/body-forbidden suppression;
- peer disconnect and server shutdown lifecycle observations;
- tracked tunnel/long-lived task shutdown.

For a moderate repeated sequential workload, bridge-owned task/admission state
must return to baseline rather than scale with historical request count.

No absolute throughput/RSS gate is required.

## Track F — migration-residue/module cleanup closure

Record what Plan 255 actually changed.

At minimum verify:

- broad unused-import suppressions removed where intended;
- no new broad suppressions were added as substitutes;
- Python bridge imports are ownership-specific;
- any `lib.rs`/CLI/H3 private decomposition preserved public exports and
  behavior;
- any candidate closed NO-CHANGE has a documented reason;
- production comments still retain necessary security/compatibility rationale.

Do not claim line-count reduction as correctness evidence.

## Track G — topology-checker semantic equivalence

If Plan 255 reorganized `check-crate-topology.py`, prove the stable command
still works:

```sh
python3 scripts/check-crate-topology.py
```

Record the baseline/candidate rule inventory.

Run positive current-tree checks and representative negative fixtures for:

- core H1 authority resurrection;
- static resolver duplication in core;
- primitives transport dependency leakage;
- unclassified core production source;
- prohibited direct capability dependency;
- detached per-connection shutdown forwarder;
- typed-package artifact loss where applicable.

Every rule family present at baseline must either remain or have an explicit
equivalent replacement. Any unexplained lost rejection blocks closure.

## Track H — documentation truthfulness

Reconcile current-state documentation only.

Ensure docs state clearly:

### Direct H1 leaf profile

```text
eggserve-primitives
eggserve-server
(+ eggserve-static when needed)
```

### Rich compatibility/multiprotocol profile

```text
eggserve-core::server
```

for the existing H2/TLS/listener/proxy/H3 composition.

Also retain:

- H2/H3 experimental status;
- Python H1-only status;
- EggServe is not an ASGI/WSGI runtime/framework/proxy;
- canonical application types remain Hyper-free;
- `eggserve-core` remains a supported compatibility/composition umbrella.

Do not rewrite historical plan/release artifacts as though the post-250
findings were known earlier.

## Track I — dependency and feature graph closure

Capture/compare dependency and feature graphs for:

- `eggserve-primitives`;
- `eggserve-server` default;
- accepted direct `http2` and `tls` feature names;
- `eggserve-static`;
- `eggserve-core` default;
- core `http2,tls`;
- core `http3,tls`;
- `eggserve-bin`;
- excluded Python wheel crate.

Confirm:

- primitives remains transport/runtime neutral;
- server has no upward core/static dependency;
- direct H2/TLS names remain inert;
- static remains the sole filesystem authority;
- H3 dependency family remains isolated from default/H1/H2 builds;
- Python wheel still names intended leaf crates directly where Plan 221
  requires it;
- no broad new production dependency landed.

## Track J — full local qualification

Run the current routine matrix on the final candidate:

```sh
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
python3 scripts/check-python-release-metadata.py
cargo fmt --all -- --check
cargo +1.89 check --workspace --all-targets
cargo +1.89 check --workspace --all-targets --features http2,tls
cargo +1.89 check --workspace --all-targets --features http3,tls
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
cargo clippy -p eggserve-core --features http2,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http2,tls
cargo clippy -p eggserve-bin --features http2,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http2,tls
cargo clippy -p eggserve-core --features http3,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http3,tls
bash scripts/check-supply-chain.sh
bash scripts/verify-cargo-packages.sh --mode all
bash scripts/test-python-wheel.sh
```

Also run focused suites from Plans 252–255 and any module-specific tests for
code actually decomposed.

Manual platform/H2/H3 qualification is required only if the implementation
plans touched code covered by those manual gates; otherwise record why routine
feature qualification is sufficient for this maintenance closure.

## Track K — remote CI provenance

Push the exact evidence candidate and require successful normal GitHub Actions
for the repository's:

- rust job;
- supply-chain job;
- python job.

Record:

- exact candidate SHA;
- run ID;
- URL;
- creation/completion timestamp;
- each job conclusion.

Do not close on local evidence alone.

## Track L — roadmap/agent-state reconciliation

After exact-SHA remote CI passes:

- mark Plans 251–256 complete in `plans/ROADMAP.md`;
- add a concise current-state summary to `AGENTS.md` only if ownership or
  workflow guidance materially changed;
- link the Plan 256 release evidence record;
- preserve Plan 250 historical closure and supersession statements;
- record any Plan 253 DEFER/Plan 255 NO-CHANGE findings as current constraints,
  not open defects unless a separate future plan exists.

## Acceptance criteria

- [ ] Python stubs and runtime public shapes agree for all Plan 252 findings.
- [ ] strict installed-wheel typing and runtime-shape tests pass.
- [ ] no prohibited Rust/Python public API difference.
- [ ] connection overlap is completely classified.
- [ ] H1 remains single executable authority in `eggserve-server`.
- [ ] H2/TLS/H3 capability ownership and support tiers are unchanged.
- [ ] accepted duplication, if any, is explicit and mechanically guarded.
- [ ] async-Python admission/task/stream state returns to baseline after
      completion/error/timeout/disconnect/shutdown.
- [ ] no migration cleanup weakens a security/compatibility invariant.
- [ ] topology-checker rule inventory has no unexplained lost rule.
- [ ] direct-H1 versus compatibility-multiprotocol usage is documented clearly.
- [ ] dependency/package topology remains policy-compliant.
- [ ] full local matrix is green.
- [ ] exact closure candidate passes remote rust/supply-chain/python CI.
- [ ] release evidence and roadmap state distinguish candidate and metadata
      commits accurately.

## Closure rule

If any acceptance criterion fails, Plans 251–256 remain open.

Do not downgrade a failing criterion into documentation-only wording. A newly
discovered unrelated defect should receive its own corrective plan; a defect
introduced by Plans 252–255 may be corrected narrowly before re-running this
closure gate.

# Plan 244 — H1 runtime authority convergence

## Purpose

Make the architectural statement “`eggserve-server` is the generic H1
runtime/service authority” true at the implementation level while preserving
all existing `eggserve-core::server` APIs and capabilities.

Plans 215–225 moved service identity, errors, response policy, runtime-limit
authority, tunnel execution, canonical request/response values, static
confinement, and H3 into direct authorities. However,
`eggserve-core/src/server/connection/` still contains a large H1 runtime
pipeline with substantial code overlap with
`eggserve-server/src/connection/`.

At the Plan 242 review baseline, normalized non-comment source comparison
showed very high overlap in several pairs, including connection context,
request conversion, lifecycle, deferred body, transport, and substantial
pipeline/driver overlap. This means H1 correctness fixes can still require
reasoning about two implementations.

Plan 243 must land first so the direct runtime has correct durable shutdown and
task ownership before core delegates more execution to it.

## Desired end state

```text
eggserve-primitives
        |
eggserve-server
  canonical H1 connection execution
  request/body policy
  response finalization
  timeout/cancellation
  activity/lifecycle kernel
  generic listener/task ownership
        ^
        |
eggserve-core
  compatibility public paths
  composed RuntimeConfig
  H2 selection/glue
  TLS accept/composition
  PROXY protocol preamble
  Unix/systemd listener composition
  compatibility lifecycle/readiness facade
  static-service wrapper
        |
eggserve-h3
  H3 adapter over shared kernel
```

Core may retain genuinely richer composition, but it must not retain a second
implementation of behavior that is protocol-generic or H1-identical.

## Compatibility freeze

The following existing public namespaces remain valid:

- `eggserve_core::server::*`;
- `eggserve_core::server::connection::*`;
- `eggserve_core::server::config::*`;
- `Server`, `ServerBuilder`, `ServerHandle`, `RuntimeConfig`;
- caller-owned transport entry points;
- H2/TLS/Tower/static/listener/systemd/proxy features and current feature names.

Type identity should be preserved through re-export where possible. Where core
has an extended type that cannot be identical to the direct type, use explicit
projection/adapters rather than parallel algorithms.

## Work

### 1. Produce an ownership matrix before edits

Classify every production file under:

- `crates/eggserve-core/src/server/connection/`;
- `crates/eggserve-core/src/server/{accept,runtime,handle,lifecycle,listener,proxy}.rs`;
- `crates/eggserve-server/src/connection/` and high-level server runtime.

For each function/type mark:

- DIRECT AUTHORITY — move/reuse in `eggserve-server`;
- CORE COMPOSITION — H2/TLS/proxy/listener compatibility glue;
- COMPAT FACADE — re-export/delegate only;
- PROTOCOL-SPECIFIC — retain in owning protocol adapter.

Store the matrix in the Plan 244 release/evidence record.

### 2. Collapse identical/shared connection modules first

Start with the lowest-risk near-identical modules:

- transport wrappers;
- connection context;
- deferred-body mechanics;
- request conversion helpers;
- lifecycle/request counters where semantics are identical.

Move any missing generic capability into `eggserve-server` privately or
additively, then convert core copies to `pub use`, narrow adapters, or
delegation.

Do not copy functionality in the opposite direction.

### 3. Converge pipeline and driver behavior

The direct H1 driver becomes the only H1 algorithm for:

- Hyper H1 builder setup;
- header/body/target limits;
- request canonicalization;
- body-policy selection;
- service admission and panic/timeout containment;
- request lifecycle/cancellation;
- canonical response normalization/finalization;
- file/stream body conversion and write-progress timeout;
- max-requests/keep-alive decisions;
- tunnel execution hooks already owned by `eggserve-server`.

Core's H1 path must enter that implementation.

Where H2 needs different Hyper builder/stream semantics, split protocol-neutral
kernels from H1/H2 transport glue rather than keeping a second H1 copy solely
because H2 is colocated with it.

### 4. Preserve compatibility-only composition

Keep or refactor in core as needed:

- H2 connection selection and H2-specific configuration;
- TLS handshake orchestration over `eggnet-tls`;
- PROXY protocol pre-read and trusted-source policy;
- Unix/systemd/socket-activation listener adoption;
- extended readiness/lifecycle facade required by current
  `eggserve_core::server::ServerHandle`;
- H3 delegation;
- Tower compatibility paths until/unless a direct owner is justified.

These layers should produce the same canonical
`ConnectionContext`/`RuntimeConfig` inputs and then delegate to direct
kernels.

### 5. Configuration projection

Keep the existing extended core `RuntimeConfig` API unchanged.

Create/retain one audited projection from the compatibility configuration to
the direct H1 subset. Validation of shared values must continue through
`eggserve_server::runtime_limits::SharedRuntimeValues`.

No duplicated default table or cross-field validator may return.

### 6. Topology gates

Strengthen `scripts/check-crate-topology.py` so Plan 244's result cannot
regress.

Prefer semantic markers over line counts:

- core connection facade modules must re-export/delegate to direct ownership;
- no second `serve_http1_connection*` implementation in core;
- no second H1 Hyper builder in core;
- no second body-policy/service-invocation kernel;
- protocol-specific H2/TLS/proxy glue remains explicitly classified;
- direct server still has no upward core/static dependency.

Do not encode brittle exact file-size thresholds.

### 7. Remove redundant dependencies only after convergence

After implementation, run `cargo tree` for direct/core feature sets and
remove direct core dependencies that are no longer used. Do not remove a
dependency merely for graph aesthetics if compatibility glue still owns a
valid use.

## Tests

Retain and expand `direct_h1_parity.rs` during migration. It is the principal
wire-level guard while old implementation code is deleted.

At minimum preserve parity for:

- GET/HEAD;
- buffered/streaming request bodies;
- chunked bodies/trailers;
- malformed framing;
- target/header limits;
- handler/header/body timeouts;
- service panic sanitization;
- max requests/keep-alive close;
- invalid config.

Add parity for any H1 behavior found in core but not currently represented,
especially:

- shutdown during active request/body/response;
- tunnel accept/deny;
- trusted-proxy-derived connection metadata where H1 delegation touches it;
- response write-stall behavior;
- prebound TCP/caller-owned streams.

The direct crate should own unit/integration tests for its algorithms; core
keeps compatibility/wire-parity and H2/TLS composition tests.

## Deletion rule

Do not delete a compatibility implementation until:

1. the direct implementation exposes the required private/additive hook;
2. parity is demonstrated;
3. every old public path resolves to the new authority or adapter;
4. topology tests prevent reintroduction.

Small staged commits are preferred so a regression can be bisected.

## Qualification

Run:

```sh
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
cargo fmt --all -- --check
cargo +1.89 check --workspace --all-targets
cargo +1.89 check --workspace --all-targets --features http2,tls
cargo +1.89 check --workspace --all-targets --features http3,tls
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo test -p eggserve-core --features http2,tls
cargo test -p eggserve-core --features http3,tls
```

Plan 248 owns final package/wheel/platform/remote-CI closure.

## Acceptance criteria

- [ ] Plan 243 lifecycle corrective is complete;
- [ ] one H1 connection execution algorithm is authoritative;
- [ ] core H1 compatibility execution delegates to `eggserve-server`;
- [ ] H2/TLS/proxy/listener behavior remains available through current APIs;
- [ ] shared config validation has one authority;
- [ ] no new core/static upward dependency appears in direct server;
- [ ] all existing core/direct Rust paths compile;
- [ ] direct H1 wire parity passes for the full inventory;
- [ ] topology CI prevents a second H1 implementation from returning;
- [ ] dependency graph is no larger without a documented reason;
- [ ] no protocol support tier or public API changes.

## Non-goals

Do not redesign public `Service` or response types, promote H2/H3, merge H3
into the server crate, add a framework API, or move static filesystem concerns
into the generic runtime.

## Executed-result note (Plan 249 corrective)

Single H1 authority was not fully closed by the initial 244 implementation:
explicit `Http1` delegated to `eggserve-server`, but `WireProtocol::Auto`
classified to `Http1` after core had committed to its private Hyper
service/driver pipeline, so ordinary cleartext (plus PROXY-replayed and Unix)
H1 still executed core Hyper H1. Accepted connections also spawned detached
broadcast-shutdown forwarders that outlived normal connection completion.
Plan 249 completed the corrective: `Auto` classifies before any Hyper service
exists and H1 delegates the replayable stream to the direct driver; core keeps
only H2-specific execution; per-connection shutdown is structured under the
connection task. See `plans/249-core-auto-h1-authority-and-shutdown-forwarder-corrective.md`
and the Plan 250 closure record.

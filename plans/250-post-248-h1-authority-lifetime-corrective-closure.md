# Plan 250 — Post-248 H1 authority and connection-lifetime corrective closure

## Purpose

Requalify the Plans 242–248 maintainability campaign after Plan 249 corrects
the residual core Auto→H1 execution path and detached per-connection shutdown
forwarder.

Plan 250 is evidence/closure work. It must not become another architecture
campaign. Production changes are allowed only to correct regressions directly
introduced by Plan 249.

## Historical context

Plan 248 closed candidate
`3fb59e4560b74407b7faed3a09aaae5974d3d36a` with successful CI run
`35602644725`, and current metadata record
`4b2af07991d20234d5167d08311ba6b18006025a` is also green.

Post-closure review found that the closure evidence was incomplete in two
specific ways:

1. the topology gate proved that direct H1 delegation existed, but did not
   prove that normal compatibility `WireProtocol::Auto` connections could
   not still execute core's private Hyper H1 pipeline;
2. the test/closure matrix did not detect that accepted compatibility
   connections created detached broadcast-forwarder tasks whose lifetime could
   extend until whole-server shutdown after the connection had already
   completed.

Plan 250 supersedes Plan 248 only for these two closure claims. It does not
invalidate the successful Plan 243 direct-server lifecycle fix, Plan 245 static
authority convergence, Plan 246 Python typing work, Plan 247 orphan-source and
feature cleanup, or their prior evidence.

## Preconditions

- Plan 249 implementation is complete.
- No core production path can execute a Hyper HTTP/1 connection directly.
- Accepted connection shutdown forwarding is structured under the connection
  task rather than detached.
- Focused Plan 249 tests are green.

## Closure artifact

Create/update a durable release record, preferably:

```text
release/plan-250-h1-authority-lifetime-corrective-closure.md
```

The record must link back to the Plan 248 closure and explicitly state which
claims are superseded.

Record:

- Plan 249 baseline SHA:
  `4b2af07991d20234d5167d08311ba6b18006025a`;
- Plan 249 implementation SHA;
- Plan 250 evidence-content/final candidate SHA;
- toolchain versions;
- focused test counts/results;
- full local matrix results;
- exact remote CI run ID/URL/conclusions;
- any final metadata-only record SHA separately.

## Track A — structural H1 authority proof

Retain a mechanically reviewable inventory of core connection execution after
Plan 249.

The closure must prove:

- `eggserve-server` contains the only production HTTP/1 Hyper builder/
  connection driver;
- core H1 public functions are facades/projections;
- normal compatibility accept paths resolve Auto before H1 execution;
- Auto→H1 delegates to
  `eggserve_server::connection::serve_http1_connection_with_id`;
- explicit TLS ALPN H1 delegates to the same direct authority;
- H2 execution remains core-owned and feature-gated;
- no direct crate gains an H2/TLS capability merely because core delegates H1.

Include a short before/after call graph in the release record.

## Track B — normal accept-path wire parity

Run both baseline-compatible and candidate behavior for representative normal
server construction, not only caller-owned direct functions.

Required H1 cases:

- cleartext TCP GET/HEAD;
- buffered and streaming body handling;
- trailers/framing rejection;
- response streaming/write-stall behavior;
- max requests/keep-alive close;
- service panic/error privacy;
- tunnel accept/deny;
- server shutdown during active request;
- prebound listener.

Composition cases:

- PROXY-prefixed cleartext H1;
- Unix-domain H1 on Unix;
- TLS ALPN H1;
- H2 prior knowledge over cleartext;
- TLS ALPN H2;
- trusted proxy metadata;
- caller-owned multiprotocol stream.

For every applicable H1 case, the normal compatibility path and direct H1
path must retain equivalent wire-visible semantics.

Do not introduce a separate benchmark framework.

## Track C — connection-lifetime resource proof

Prove the detached-forwarder defect is closed.

### Receiver/task lifetime

Use the deterministic Plan 249 helper/unit regression to show:

- starting one active connection adds only the expected connection-scoped
  shutdown receiver/future;
- normal connection completion drops it without waiting for server shutdown;
- N completed sequential connections do not leave N forwarder receivers/tasks;
- server shutdown still wakes active connections.

If `broadcast::Sender::receiver_count()` is used, record before/during/after
counts. If another test-owned counter is used, record the equivalent evidence.

### Runtime resources

For a moderate repeated short-connection loop, record:

- active connection gauge before/after;
- available connection permits before/after where test-visible;
- task/receiver count used by the regression;
- server shutdown/wait completion.

No absolute latency/RSS performance gate is required; the defect is structural
resource retention.

## Track D — topology-gate negative tests

Demonstrate that the strengthened topology check fails when representative
forbidden constructs are reintroduced.

At minimum verify detection of:

- a core H1 `hyper_builder`;
- a direct core
  `hyper::server::conn::http1::Connection`/UpgradeableConnection owner;
- a resolved Auto/Http1 branch that drives Hyper instead of delegating;
- a detached `tokio::spawn` shutdown forwarder in the accept path.

This may be implemented as checker unit fixtures or documented temporary
mutation tests. Do not leave intentionally failing source in the repository.

The positive current tree must pass the same checker.

## Track E — API and feature compatibility

Re-run source/API fixtures from Plan 248 for the touched public namespaces.

At minimum compile existing usage of:

- `eggserve_core::server::connection::serve_connection_with_runtime_state`
  if public;
- `serve_http1_connection`;
- `serve_http1_connection_with_id`;
- `serve_http_connection`;
- `serve_http_connection_with_id`;
- `RuntimeConfig`, `RuntimeState`, `ConnectionContext`,
  `ConnectionShutdown`;
- normal `ServerBuilder` TCP/TLS/Unix/proxy construction.

Confirm accepted inert direct feature names remain accepted:

- `eggserve-server/http2`;
- `eggserve-server/tls`;
- `eggserve-primitives/http-interop`.

No new direct capability is implied by those names.

## Track F — full repository qualification

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
bash scripts/check-supply-chain.sh
bash scripts/verify-cargo-packages.sh --mode all
bash scripts/test-python-wheel.sh
```

Also run focused H1/H2/TLS/PROXY/Unix/tunnel/lifecycle suites.

The Python wheel should not require behavior changes, but it remains part of
the closure because the compatibility server is consumed by first-party
frontends.

## Track G — remote CI provenance

Push the exact evidence-content candidate and require successful normal GitHub
Actions for:

- rust;
- supply-chain;
- python.

Record the run ID, URL, SHA, timestamp, and job conclusions.

If a final documentation-only commit is needed to record that successful run,
distinguish:

- implementation/evidence candidate SHA verified by CI;
- final metadata-record SHA.

Do not claim the metadata-only SHA was independently requalified if it was not.

## Track H — reconcile Plans 244/248/ROADMAP truthfully

Update only current-state records.

Required changes:

- Plan 244 executed-result note: single H1 authority was not fully closed by
  the initial 244 implementation because Auto→H1 remained executable in core;
  Plan 249 completed that corrective;
- Plan 248 closure note: original CI result remains valid for its candidate,
  but the two closure claims identified above were superseded and corrected by
  Plans 249–250;
- `plans/ROADMAP.md`: mark Plans 249–250 complete only after exact-SHA CI;
- `release/plan-248-maintainability-convergence-closure.md`: add a concise
  supersession pointer rather than erasing historical evidence;
- architecture/topology docs: state clearly that core owns H2 selection/
  execution while direct server owns all H1 execution.

Do not rewrite old evidence as though the defect was known at execution time.

## Acceptance criteria

- [ ] normal cleartext compatibility H1 cannot execute a core Hyper H1 driver;
- [ ] PROXY/Unix/TLS H1 composition delegates to the same direct authority;
- [ ] H2 Auto/ALPN execution remains unchanged and green;
- [ ] topology checker structurally forbids a second core H1 execution path;
- [ ] no detached per-connection shutdown forwarder remains;
- [ ] connection-scoped shutdown receiver/task lifetime ends on normal
      connection completion;
- [ ] repeated short connections show no historical forwarder accumulation;
- [ ] connection gauges/permits return to baseline;
- [ ] existing Rust/Python public API and feature names remain compatible;
- [ ] full routine/security/package/wheel matrix passes;
- [ ] exact final candidate SHA has successful remote CI evidence;
- [ ] Plan 244/248/ROADMAP/release records accurately reflect the corrective.

## Closure decision

If all criteria pass, Plans 242–250 may be considered closed for the current
API-preserving H1/static/Python maintainability campaign.

If Auto→H1 cannot be delegated without changing public semantics, stop and
record the precise blocker rather than weakening the single-authority claim.

If H2 behavior regresses, correct the protocol-selection boundary; do not move
H2 wholesale into `eggserve-server` under this plan.

## Non-goals

No feature expansion, H2/H3 tier promotion, new public lifecycle API, new
runtime dependency, performance campaign, static-serving redesign, or Python
surface change is authorized.

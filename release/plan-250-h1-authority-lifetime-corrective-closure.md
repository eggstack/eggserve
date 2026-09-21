# Plan 250 — Post-248 H1 authority and connection-lifetime corrective closure

Status: COMPLETE.

This record supersedes exactly two claims from the Plan 248 closure
(`release/plan-248-maintainability-convergence-closure.md`, candidate
`3fb59e4560b74407b7faed3a09aaae5974d3d36a`, CI run `35602644725`), which
remains valid for its candidate and for all other Plan 243–248 results:

1. the topology gate proved direct H1 delegation *existed* but did not prove
   normal compatibility `WireProtocol::Auto` connections could not still
   execute core's private Hyper H1 pipeline;
2. the test/closure matrix did not detect accepted compatibility connections
   creating detached broadcast-forwarder tasks whose lifetime extended until
   whole-server shutdown after the connection had completed.

Plan 249 (`plans/249-core-auto-h1-authority-and-shutdown-forwarder-corrective.md`)
corrects both; this record (Plan 250) requalifies and closes Plans 242–250
for the API-preserving H1/static/Python maintainability campaign. No feature,
tier, API, default, or behavior change.

## Candidate SHAs

- Plan 249 baseline: `4b2af07991d20234d5167d08311ba6b18006025a`
  (`docs: close plans 242-248 with CI evidence`).
- Implementation/evidence candidate: `e38d12d7d177888e7fc38fea42cc51f5a0ee5169`
  (`fix(core): route Auto H1 to direct authority; structure shutdown (plan 249)`).
- Remote CI verifying the exact candidate: run `35618901331`
  (`https://github.com/eggstack/eggserve/actions/runs/35618901331`) —
  rust / supply-chain / python all `success` (completed 2026-09-21T15:42:08Z).
- Any later metadata-only commit recording this run is identified separately
  and is not claimed as independently requalified beyond the routine CI it
  triggers on push.

## Toolchains

- Rust stable 1.98.1 / MSRV lane 1.89.0 (pinned `cargo +1.89` checks).
- CPython 3.14.6 building the CPython 3.11 abi3 wheel; local Python 3.12.3
  for repo scripts.

## Track A — structural H1 authority proof

`eggserve-server` contains the only production HTTP/1 Hyper builder /
connection driver. Core H1 public functions are facades/projections; H2
execution remains core-owned and feature-gated; no direct crate gained an
H2/TLS capability.

Before (baseline `4b2af07`):

```text
accept.rs (Auto)
  -> serve_http_connection_with_id_and_protocol(Auto)
     -> build canonical Hyper service          # H1 service built in core
     -> serve_hyper_with_token_auto
        -> serve_selected_with_token(Auto)
           -> classify_cleartext -> Http1
           -> serve_selected_resolved_with_token(Http1)
              -> core hyper_builder(...).serve_connection(...)   # core H1 exec
              -> core drive_connection(...)
```

After (candidate `e38d12d`):

```text
accept.rs (Auto, structured shutdown via run_with_connection_shutdown)
  -> serve_http_connection_with_id_and_protocol(Auto)
     -> [http2] classify_cleartext BEFORE any Hyper service exists
        -> Http1  => eggserve_server::serve_http1_connection_with_id  # direct
        -> Http2  => build H2-only Hyper service -> serve_h2_with_token  # core
     -> [no http2] delegate directly (Auto is cleartext H1, no sniffing)
explicit Http1 (incl. TLS ALPN H1) delegates immediately to the direct driver.
serve_connection_with_runtime_state keeps its signature; adapts broadcast to
ConnectionShutdown in-task and delegates to the direct driver.
```

Removed from core production code: `hyper_builder` (H1),
`hyper::server::conn::http1::{Connection, UpgradeableConnection}` impls,
`serve_connection` / `serve_hyper_with_token` /
`serve_selected_with_token` / `serve_selected_resolved_with_token` /
`serve_hyper_with_token_auto`, the resolved `WireProtocol::Http1` Hyper
branch, the `Http1Config` second projection (module retained as an inventory
placeholder with no authority), and both detached `tokio::spawn` shutdown
forwarders in `accept.rs`. Remaining core connection modules classify as:
H2-specific execution (`driver.rs` H2 parts, `activity`/`lifecycle`/
`pipeline`/`request`/`response`/`transport`/`deferred_body`, all
`http2`-gated); protocol-selection/replay composition (`WireProtocol`,
`PrefixedIo`, `classify_cleartext`); direct H1 facade/projection (`mod.rs`).

## Track B — normal accept-path wire parity

New suite `crates/eggserve-core/tests/auto_h1_delegation.rs` drives the
normal compatibility accept path (`ServerBuilder`, i.e. `Auto`): 8 tests
default / 9 with `http2` (cleartext GET control vs explicit direct entry,
buffered POST echo, HEAD length/no-body, prebound TCP, PROXY-prefixed
cleartext with replay, Unix-domain H1, 32× sequential-connection drain with
gauge-to-zero, multiprotocol-entry H1-bytes resolution). TLS ALPN H1
delegates through the unchanged explicit-`Http1` branch and stays covered by
the existing TLS suites. Existing parity fixtures re-run green:
`direct_h1_parity` (16), `direct_service_convergence` (3),
`tunnel_upgrade`, `trusted_proxy`, `listener_ownership`, `http2_runtime`,
`tls_identity`, cross-protocol conformance (routine 18).

## Track C — connection-lifetime resource proof

`run_with_connection_shutdown` (in `accept.rs`, `pub(super)`) owns the
broadcast receiver for exactly the connection-body future: normal completion
drops it immediately; server shutdown signals the level-triggered
`ConnectionShutdown` and drains the body in-task. Pre-signaled shutdown is
not lost (idempotent token observed on first poll).

Deterministic unit proof (no task counting):

- `structured_shutdown_drops_receiver_on_normal_completion`:
  `receiver_count()` 1 → 0 after normal completion; token not signaled.
- `structured_shutdown_signals_active_connection`: shutdown sent while the
  body parks on the token wakes it, drains it, signals the token, and drops
  the receiver (1 → 0).
- `structured_shutdown_receiver_count_is_stable_over_many_connections`:
  32 sequential connections leave exactly the 1 guard receiver.
- `repeated_short_connections_drain_cleanly` (accept-path): 32 short
  connections, active gauge returns to 0, shutdown/wait completes.

Flake note (honest record): during local `http2,tls` full-matrix runs, the
pre-existing timing-sensitive `lifecycle_integration` suite (52 tests, ~30s,
50–500ms sleep/timeout races around 100ms grace / 50ms force shutdown)
failed twice (1 then 3 failures, names not captured) with zero code change
between red and green runs — the only intervening edit was a test-only
clippy fix. It then passed 6 consecutive times (2 full-matrix + 4 isolated).
No production-code change occurred between the red and green runs. The suite
is green in the remote CI run cited above.

## Track D — topology-gate negative tests

`check_plan249_h1_authority()` (in `scripts/check-crate-topology.py`)
rejects in production core connection code: `fn hyper_builder`,
`http1::Builder` / `http1::Connection` / `UpgradeableConnection`,
the removed H1 driver helpers, a second `fn serve_http1_connection` in the
driver, a `WireProtocol::Http1 => {` execution block (classifier returns,
H2 logging, and test asserts still allowed), and any `tokio::spawn` /
`forwarder_*` state in production `accept.rs` (test modules excluded via
`#[cfg(test)]` split); it requires `run_with_connection_shutdown` and the
public H1 facade signatures. Temporary-mutation verification: reintroducing
each of `fn hyper_builder`, `http1::Connection`, an `Http1 => {` block, or a
`tokio::spawn` in `accept.rs` fails the gate; the current tree passes.

## Track E — API and feature compatibility

Public surface unchanged and compiling: `serve_connection_with_runtime_state`
(same signature, now delegates), `serve_http1_connection`,
`serve_http1_connection_with_id`, `serve_http_connection`,
`serve_http_connection_with_id`, `RuntimeConfig`, `RuntimeState`,
`ConnectionContext`, `ConnectionShutdown`, normal `ServerBuilder`
TCP/TLS/Unix/proxy construction (`server_integration.rs`,
`public_api_consumers.rs`, `api_stability.rs` green). Accepted inert direct
features unchanged (`eggserve-server/http2`, `eggserve-server/tls`,
`eggserve-primitives/http-interop`); the gate asserts their reserved-empty
declarations. Doc-only: `Http1Config` removed (was `pub(crate)`); the
`server/config/http1.rs` inventory path is retained as a placeholder — the
Plan 225 classified inventory is unchanged in membership.

## Track F — full repository qualification (final candidate)

Local, all green:

- `verify-conformance-matrix.py` (51 entries, 55 app-server scenarios,
  17 Plan 213 H3 scenarios), `check-crate-topology.py`,
  `check-python-release-metadata.py` (0.2.0), `cargo fmt --all -- --check`.
- `cargo +1.89 check --workspace --all-targets` (default, `http2,tls`,
  `http3,tls`).
- `cargo clippy --workspace --lib --bins --tests -- -D warnings`;
  per-feature clippy for core (`http2,tls`; `http3,tls`) and bin
  (`http2,tls`; `tls`; `http3,tls`).
- `cargo test --workspace`: 1957 passed, 4 ignored (78 suites).
- `cargo test -p eggserve-core --features http2,tls`: 59 suites, 0 failures
  (final run; see flake note above).
- `cargo test -p eggserve-core --features http3,tls`: 0 failures.
- `cargo test -p eggserve-bin --features http2,tls` / `--features tls`:
  141 passed each; `--features http3,tls`: green.
- `cargo check` on the excluded Python manifest (locked).
- `check-supply-chain.sh`: advisories/bans/licenses/sources ok (both
  lockfiles).
- `verify-cargo-packages.sh --mode all`: layered packaging green.
- `test-python-wheel.sh`: 804 Python tests OK (no behavior change; the
  compatibility server is consumed by first-party frontends).

## Track G — remote CI provenance

Run `35618901331`
(`https://github.com/eggstack/eggserve/actions/runs/35618901331`) on exact
SHA `e38d12d7d177888e7fc38fea42cc51f5a0ee5169` (pushed 2026-09-21T15:25:57Z,
run created 2026-09-21T15:26:03Z): **rust — success** (conformance, topology,
metadata, fmt, MSRV checks, clippy, workspace tests, excluded-crate check,
TLS/HTTP-2/HTTP-3 lanes), **supply-chain — success**, **python — success**
(metadata sync, maturin build/install/smoke/tests). Completed
2026-09-21T15:42:08Z.

## Track H — reconciliation

- Plan 244 (`plans/244-h1-runtime-authority-convergence.md`): executed-result
  note records that single H1 authority was not fully closed by the initial
  244 implementation (Auto→H1 remained executable in core) and that Plan 249
  completed the corrective.
- Plan 248 (`release/plan-248-maintainability-convergence-closure.md`):
  supersession pointer added; historical evidence preserved.
- `plans/ROADMAP.md`: Plans 249–250 marked complete with exact-SHA CI; the
  242–248 program status notes the corrective closure.
- Architecture/topology docs (`architecture/crate-topology.md`,
  `architecture/runtime.md`, `architecture/eggserve-core.md`, `README.md`,
  `AGENTS.md`, skill): state that core owns H2 selection/execution while the
  direct server owns all H1 execution, with structured per-connection
  shutdown. No old evidence rewritten.

## Closure decision

All Plan 250 acceptance criteria pass. Plans 242–250 are closed for the
current API-preserving H1/static/Python maintainability campaign.

Non-goals honored: no feature expansion, H2/H3 tier promotion, new public
lifecycle API, new runtime dependency, performance campaign, static-serving
redesign, or Python surface change.

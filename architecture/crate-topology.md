# Crate topology

Plans 211–247 establish dependency layers while preserving the historical
`eggserve-core` 0.x source contract. Plan 214 moves the qualified canonical
and static implementations into their direct crates; Plan 215 moves the
mature generic H1 connection runtime into `eggserve-server` with a
direct-vs-compatibility parity suite; Plan 216 moves tunnel authority to the
direct crates; Plan 217 finishes service/request convergence with a single
`Service` contract and canonical request types plus a downstream fixture;
Plan 219 collapses the remaining static/path/filesystem duplication onto
`eggserve-static`, leaving `eggserve-core` with compatibility facades only;
Plan 220 moves the H3/QUIC transport adapter into `eggserve-h3`, leaving
`eggserve-core` with a thin facade only. Plan 221 migrates the first-party
frontends (`eggserve-bin`, `eggserve-python`) onto those leaf crates for
every neutral path, leaving `eggserve-core` for the extended server
orchestration until Plan 225 closes the facade. Plan 224 closes as NO-GO:
no capability-filesystem crate is created and `eggserve-static` remains the
single confinement authority (see
`release/plan-224-capability-filesystem-evaluation.md`).
Protocol-specific compatibility glue
(H2 wire mechanics, extended listener/proxy/TLS paths) remains in
core as explicit transport glue, with topology-gate ownership rules marking the
boundary (see `release/plan-215-direct-runtime-parity.md`).

Plans 243–247 finish the next authority/maintainability pass. The direct
server's shutdown state is durable and its accept loop owns and drains accepted
connection tasks. Core H1 entry points project configuration and shared runtime
state into the direct H1 driver; core remains the owner of H2/TLS/proxy
composition. Plan 249 completes the single-H1-authority corrective: normal
compatibility `Auto` classification resolves before any Hyper service exists
and every H1 path (cleartext, PROXY-replayed, Unix, TLS ALPN) delegates the
replayable stream to `eggserve-server`; core constructs no Hyper H1
connection and drives H2 only. Per-connection shutdown is structured under
the connection task (`run_with_connection_shutdown`) with no detached
forwarder. The direct static crate now also owns `StaticService` request
planning and rendering, while core's `StaticService` is a compatibility wrapper.
The Python wheel carries maintained stubs and `py.typed`, and native PyO3
registration is isolated from implementation modules. The topology checker
walks production Rust module reachability, rejects orphan sources, and asserts
that accepted direct-crate compatibility features remain inert.

```text
eggnet-tls             (neutral rustls identity/trust/reload substrate)

eggserve-primitives   (canonical values; transport-neutral dependencies)
          │
          ▼
eggserve-server       (generic HTTP runtime; Hyper/Tokio)
          │
          ▼
eggserve-static       (filesystem and static specialization)

eggserve-h3           (experimental Quinn/H3/H3-Quinn dependency boundary)

eggserve-core         (compatibility/composition umbrella and protocol glue)
   ├── eggserve-primitives
   ├── eggserve-server
   ├── eggserve-static
   ├── eggnet-tls (optional `tls` feature)
   └── eggserve-h3 (optional `http3` feature)
```

## Ownership

`eggserve-primitives` is the leaf crate. It owns the extracted canonical
request, response, header, method/version, lifecycle, generic policy, limits,
proxy provenance, and tunnel-intent values. Its small `bytes`/`futures-util` dependencies are
transport-neutral; it must not acquire Hyper, Hyper-util, Tokio, TLS, QUIC, or
filesystem dependencies.

`eggserve-server` owns the mature generic H1 connection runtime: per-runtime
observability (`ops`), the `ServerError` taxonomy, response privacy policy,
the shared runtime-limit authority, the single service contract (`Service`
with additive `call_with_tunnel`), connection
vocabulary (`ConnectionContext`/`ConnectionShutdown`/`ConnectionOutcome`),
H1 configuration/state, the H1 connection driver (request conversion, body
policy, admission, panic containment, timeouts, normalization, Hyper
conversion), the listener/prebound TCP `Server`, and the
generic tunnel/upgrade execution (`tunnel`: one-shot capability, bounded
`TunnelIo`, H1 detection, shared `run_tunnel` future). It also exposes the
small shared kernel the H3 adapter needs (`select_body_policy`,
`contain_service_panic`, `invoke_canonical_service`,
`finalize_canonical_response`, lifecycle registry) without gaining QUIC
types. Its direct path
preserves one-shot request bodies, response streams, opened-file streaming,
normalization, and bounded request timeouts. It may depend on the primitives
crate and transport dependencies, but never on `eggserve-core` or
`eggserve-static`. H2 dispatches through the same canonical contract as
explicit transport glue; H3 mechanics live once in `eggserve-h3` (Plan 220).

`eggserve-static` is the sole implementation owner of static path parsing
(`path`: `ConfinedPath`/`PathPolicy`/`PathRejection`/decode/platform),
the pinned root, Unix descriptor-relative and Windows handle-relative
traversal (`fs`, crate-internal), symlink/reparse/dotfile enforcement,
resolved file/directory capabilities (`SecureRoot`/`ResolvedFile`/
`ResolvedDirectory`/`ResolvedResource`), MIME selection, conditional/range
planning (`planner`), and directory listing construction. It depends on the
two lower layers. Static policy and confinement do not belong
in the generic server crate. The `python-bindings-internal` feature carries
the narrow capability bridge (`ResolvedFile::from_parts`/`into_parts`/
`into_std_file`), which moves the already-opened handle without
reconstructing provenance.

Plan 224 evaluated extracting the platform confinement machinery
(`PinnedRoot`/`RootGuard`/fd-relative/handle-relative traversal/child
open/listing) into a neutral `eggserve-capfs`/`eggcapfs` crate and closed
NO-GO: the resolver consumes `ConfinedPath`/`StaticPolicy`, returns
`BodySource` with MIME planning, intentionally duplicates parse-level
validation as defense in depth, already isolates production unsafe to
`fs/windows.rs`, and has no second consumer — so a new crate would leak
eggserve policy, mostly re-export internal types, and split the audited
validation without reducing complexity (see
`release/plan-224-capability-filesystem-evaluation.md`).

`eggserve-core` is the compatibility and composition umbrella for EggServe's
direct primitives, runtime, static-serving, TLS, and optional protocol
adapters. Plan 226 executes the `0.2.0` version transition and the Rust
1.89 MSRV move with no ownership change.
Existing top-level paths retain compatibility glue for Python, H2/H3, TLS,
and legacy configuration; request/service/tunnel/canonical types are facades
over the direct authorities (no second envelope, taxonomy, normalization, or
state machine), and static/path/filesystem types are facades over the
`eggserve-static` authority (no second parser, resolver, planner, or MIME
table; `src/fs`, `src/path`, and `src/mime.rs` are deleted and the
topology gate rejects their return). H2 dispatches through the single contract as transport glue,
and `server/connection/tunnel.rs` stays deleted. The `eggserve_core::layers` module exposes
the direct crates for migration; new consumers should name the direct leaf
crate they need. The downstream fixture
(`crates/eggserve-core/tests/direct_service_convergence.rs`) proves one
direct `Service` drives direct H1 and compatibility H2, and the authority
fixture (`crates/eggserve-core/tests/static_authority_conformance.rs`)
proves core static paths resolve to the static implementation.

Plan 225 closes the 217–224 program by proving `eggserve-core` is a
compatibility facade rather than an implementation authority. The closure
removes the last duplicated implementation (the unreferenced
`primitives/canonical/` response-vocabulary copy next to the `canonical.rs`
facade) and the last leftover implementation dependency (`phf`, whose MIME
table lives once in `eggserve-static`). What remains in core besides
facades and adapters is documented compatibility orchestration, not a
second security/protocol authority: `ServeConfig`/`ServeState`/`Limits`
bridges, the full TLS/H2/H3 `Server`/`ServerHandle`/`RuntimeConfig`
runtime, the static-service compatibility wrapper with extra-header/error-policy
composition, and the H2/listener/proxy/TLS transport glue. Every
production module is in the classified inventory enforced by the
topology gate; new modules fail until explicitly classified, and any core
removal/deprecation requires a separate explicit migration plan (see
`release/plan-225-compatibility-facade-closure.md`).

Plan 221 makes the first-party frontends prove the direct architecture.
`eggserve-bin` names `eggserve-primitives` (policy), `eggserve-server`
(observability, shared limits), `eggserve-static` (direct H1 tests), and
`eggnet-tls` (neutral loading) directly; its unit tests drive the leaf
`Server` + leaf `StaticService` with no compatibility import.
`eggserve-python` names the same leaves plus `eggserve-bin` (the
extension-backed CLI via `run_cli`, confirmed used and retained): neutral
policy/primitives, ops, service, response policy, shared-limit validation
(`SharedRuntimeValues`, one Rust authority), static planning/capabilities
(`eggserve-static`, including the `python-bindings-internal` capability
bridge), neutral tunnel execution (`eggserve-server::tunnel`), and neutral
TLS loading (`eggnet-tls`). The narrow remaining compatibility uses are
explicit blockers for Plan 225, not second implementations: the extended
`Server`/`ServerHandle`/`RuntimeConfig` with TLS, `LifecycleState`,
`ServeConfig` + `validate_static_metadata`, and static listing budgets
(`DEFAULT_MAX_LISTING_ENTRIES`). The Python bridge keeps those uses confined
to `runtime.rs`, `static_responder.rs`, `lifecycle.rs`, and one listing call;
all other bridge modules are core-free in code.

`eggnet-tls` is the neutral TLS security substrate. It depends only on
`rustls` and `rustls-pki-types` at runtime and owns bounded PEM parsing, SNI
identity selection, explicit WebPKI client authentication, trust/CRL bounds,
neutral ALPN hooks (`alpn_protocols` for non-HTTP transports alongside the
HTTP-only `http2` convenience), and atomic reload snapshots. It must not acquire EggServe, Eggress, EggFetch,
HTTP, proxy, tracing, Tokio, or QUIC dependencies. EggServe keeps only the
HTTP/3-specific QUIC configuration assembly in its compatibility facade and
re-exports the neutral API from `eggserve_core::tls`.

Plan 223 draws the complementary CONNECT boundary without adding a crate:
eggserve owns only inbound server-side `CONNECT`/tunnel acceptance
(neutral intent in `eggserve-primitives::tunnel`, execution in
`eggserve-server::tunnel`, compatibility H1/H2 delegation, H3 bridging in
`eggserve-h3`). The shared outbound H1 CONNECT wire primitive for
eggfetch/eggress (caller-owned-stream authority/request-head encode,
bounded response-head parse, read-ahead preservation; dialing, DNS, TLS,
timeout/retry, routing, lifecycle caller-owned) lives outside this
workspace. Eggserve must never depend on that neutral crate, on an HTTP
client stack, or on eggfetch/eggress as products; eggress inbound
`handle_connect`/auth/forwarding/relay stays locally owned there. See
`plans/223-http-connect-cross-repo-consolidation.md`.

`eggserve-h3` owns the experimental H3/QUIC transport adapter and the
coordinated direct production dependencies on `h3`, `h3-quinn`, and Quinn.
It depends downward on `eggserve-primitives`, `eggserve-server`, and
`eggnet-tls` (canonical types, shared kernel, identity parsing); those
crates never depend upward. Its narrow public surface is the adapter entry
(`accept_loop`), H3-owned config (`Http3Config`), QUIC endpoint helpers,
and the version record; Quinn/H3 transport types stay crate-internal or
doc-hidden. `eggserve-core` consumes it only behind `http3`, so the default and
HTTP/1/H2 core graphs do not compile the QUIC stack. The compatibility
`server::http3` path is a thin facade projecting core config/state into the
adapter with no second state machine.

## Enforcement

Run `python3 scripts/check-crate-topology.py` to inspect Cargo metadata. The
check rejects forbidden direct dependencies in the primitives leaf and neutral
TLS crate, rejects
core/static edges from the generic server, and requires static to consume
primitives plus server. The Plan 215 rules additionally assert direct
ownership of the H1 runtime vocabulary (ops/errors/policy/authority/service/
driver markers), compatibility re-export facades, single-contract shape, and
no upward source references. The Plan 216 rules assert neutral
tunnel vocabulary (no Hyper/Tokio/H2/H3/QUIC in primitives tunnel code),
direct tunnel execution ownership, deletion of the compatibility H1 tunnel
transport, core tunnel facades, and a dev-only WebSocket codec. The Plan 217
rules assert canonical request/service facades, Hyper/TLS/QUIC-free
primitives, single `Service` re-export, H1/H2 `call_with_tunnel` dispatch
with the shared `run_tunnel` future, and the downstream convergence fixture.
The Plan 219 rules assert the single static/path/filesystem authority:
`src/fs`, `src/path`, and `src/mime.rs` absent from core, core
secure-root/planner modules as `eggserve_static` re-exports, the static
authority exposing `ConfinedPath`/`SecureRoot`/planner/`resolve_and_plan`
surface, no `rustix::fs` use (or `fs` feature) in core, and the authority
conformance fixture. The Plan 220 rules assert the single H3/QUIC adapter
authority: `server/http3/` absent from core, core `http3.rs` delegating to
`eggserve_h3::accept_loop`, `Http3Config` and QUIC assembly owned once in
`eggserve-h3` with core facades, H3 endpoint assembly via H3-owned helpers,
no second canonical helpers in core, and downward-only H3 deps. The Plan 221
rules assert first-party leaf consumption: frontend manifests name the leaf
crates directly, binary neutral paths (policy, ops, TLS, direct H1 tests)
use the leaf, Python neutral bridge modules are core-free in code outside
the documented extended-orchestration blockers, the extension CLI stays via
`eggserve_bin::run_cli`, and Python validation projects through canonical
`SharedRuntimeValues`. The Plan 225 rules close the facade: no
`primitives/canonical/` second implementation, no leftover `phf` MIME
dependency in core, a classified production-module inventory that rejects
unclassified new modules, and facade discipline (`pub use eggserve_...`)
for every `primitives/*.rs` compatibility file except the documented Plan
200 `http-interop` adapters. It is part of the Rust CI preflight and
`scripts/verify.sh fast`. The Plan 224 rule is a narrow NO-GO guard: the
resolved workspace graph must contain no `eggserve-capfs`/`eggcapfs`/`capfs`
crate, so a future split requires an explicit plan and gate update.

The check also verifies that the mature static resolver is present and the old
pathname-based topology fixture is absent. It is a dependency/source-ownership
boundary, not a line-count rule; remaining core compatibility glue must not
become a new implementation of the direct contracts.

The Plan 243–247 rules additionally require durable direct-server shutdown and
runtime-owned task draining, direct delegation at compatibility H1 entry points,
direct static-service ownership with no core renderer, wheel typing artifacts
plus a decomposed registration module, zero orphan production Rust sources, and
inert accepted `http2`/`tls`/`http-interop` feature declarations where those
direct leaves are intentionally H1/transport-neutral. The Plan 249 rules close
the single-H1-authority gap the 244 gate missed: core must contain no Hyper H1
builder/connection/driver machinery (`fn hyper_builder`, `http1::Connection` /
`UpgradeableConnection` execution, a resolved `WireProtocol::Http1` Hyper
branch, a second `serve_http1_connection`, or the removed H1 driver helpers),
and the accept path must contain no detached per-connection shutdown forwarder
(`tokio::spawn` / `forwarder_*` state; `run_with_connection_shutdown` owns the
receiver inline). Allowed: H2 Hyper ownership, the bounded H2 prior-knowledge
classifier, `PrefixedIo` replay composition, and direct calls into
`eggserve_server::connection::*`.

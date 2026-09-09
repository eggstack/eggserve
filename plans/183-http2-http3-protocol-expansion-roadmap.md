# Plan 183 — HTTP/2 and HTTP/3 Protocol Expansion Roadmap

## Status

**IMPLEMENTED — explicit product-scope/API transition gate for optional HTTP/2 and HTTP/3 support.**

This plan is the umbrella and sequencing authority for Plans 184–188. It does not itself implement a second protocol. Implementation of this plan first changes the repository contract so the later plans are permitted to proceed.

Current baseline: `main` after Plans 179–182, with the canonical service/runtime boundary, shared runtime-limit authority, decomposed HTTP/1 connection pipeline, per-runtime observability context, and synchronized Python ownership/release metadata in place. Re-verify those invariants at implementation time rather than repeating their work.

## Purpose

Reopen EggServe's intentionally frozen protocol scope in a controlled way so the project can add:

- native HTTP/2 over the existing TCP/TLS runtime;
- native HTTP/3 over a separate QUIC/UDP transport;
- one canonical request/service/response model shared by HTTP/1, HTTP/2, and HTTP/3;
- protocol-specific resource and lifecycle policy without weakening the existing hardened HTTP/1 behavior;
- optional protocol features that do not force HTTP/2/3 dependencies into the minimal HTTP/1 build.

The objective is not to turn EggServe into an edge platform. Static serving remains the primary end-user product, and the Rust runtime remains a reusable substrate for downstream HTTP application servers. Routing, middleware, reverse proxying, uploads, WebSockets, ACME, application workers, ASGI/WSGI adapters, WebTransport, CONNECT tunneling, and generic QUIC applications remain outside this program.

## Why a scope gate is required

`docs/non-goals.md` currently states both:

- HTTP/2 is unsupported/out of scope; and
- the product-surface freeze rejects HTTP/2/3 capability expansion without a new explicit product decision.

The current architecture and documentation therefore correctly prohibit an implementation from simply enabling Hyper's `http2` feature or adding QUIC dependencies. Before code changes begin, the repository contract must explicitly authorize the narrow protocol expansion described by this plan.

This plan is that implementation roadmap, but the source-of-truth product documents must still be amended during Track A so future contributors and automated agents do not see contradictory instructions.

## Program dependency graph

```text
Plan 183  Scope/API transition and protocol architecture decision
    |
    v
Plan 184  Protocol-neutral runtime preparation and overlap cleanup
    |
    v
Plan 185  HTTP/2 runtime, TLS/ALPN, multiplexing, and policy
    |
    v
Plan 186  HTTP/2 conformance, interoperability, and frontend closure
    |
    v
Plan 187  HTTP/3 QUIC transport and canonical H3 adapter
    |
    v
Plan 188  HTTP/3 interoperability and multi-protocol release closure
```

Do not start Plan 187 merely because an H3 prototype can answer requests. HTTP/3 depends on the protocol-neutral lifecycle and metadata work established by Plans 184–186; otherwise it will create a second request pipeline and duplicate the same policy fixes.

## Current-state findings that drive the program

### 1. The service boundary is already the correct reuse seam

`eggserve-core::server::Service` consumes EggServe's canonical `Request` and returns a canonical `Response`. Static serving likewise converges on the canonical response model before transport conversion. No public service is expected to write raw HTTP/1 bytes or own Hyper connection state.

Preserve this boundary. HTTP/2 and HTTP/3 must be transport adapters around the same canonical service, not new service traits.

### 2. The HTTP/1 driver still owns several protocol-specific policies

The current runtime expresses some lifecycle decisions through HTTP/1 mechanisms such as `Connection: close`, a connection-wide request count, aggregate TCP write progress, and one hard connection lifetime. These mechanisms cannot simply be reused under multiplexed protocols.

Plan 184 must separate the policy decision from its protocol-specific wire action before H2/H3 are enabled.

### 3. Stable canonical `HttpVersion` is currently HTTP/1-only

The stable exhaustive `HttpVersion` enum contains only `Http10` and `Http11`. Repository API-stability rules state that adding a variant to a stable exhaustive enum is a breaking change.

Protocol expansion therefore requires an explicit pre-1.0 API transition. Do not smuggle `Http2`/`Http3` into a patch release.

### 4. HTTP/2 can remain within the Hyper family

The current dependency graph already uses Hyper and Hyper-Util with HTTP/1-only feature selection. HTTP/2 should be added through Hyper/Hyper-Util rather than introducing a second H2 implementation.

The target is one TCP/TLS byte-stream driver capable of HTTP/1 and HTTP/2, with protocol selection driven by TLS ALPN for HTTPS and an explicitly documented cleartext policy.

### 5. HTTP/3 is a separate transport implementation

HTTP/3 is HTTP semantics over QUIC/UDP, not a Hyper HTTP/1/2 parser mode. It requires a QUIC endpoint, TLS 1.3/QUIC configuration, an H3 request/response adapter, per-stream lifecycle/cancellation, and explicit QUIC resource policy.

The H3 stack must remain feature-gated and internal so pre-1.0 transport crates do not leak into EggServe's stable API.

## Track A — Reopen the product contract narrowly

### A1. Amend explicit non-goals

Update `docs/non-goals.md` so it no longer says all HTTP/2/3 work is categorically forbidden. Replace the old prohibition with a narrow statement such as:

- HTTP/2 and HTTP/3 are optional transport/runtime capabilities governed by Plans 183–188;
- HTTP/1.1 remains the minimal/default compatibility baseline;
- protocol expansion does not authorize reverse proxying, routing, WebSockets, WebTransport, CONNECT tunnels, server push, ACME, middleware, or application-server behavior in-tree;
- Python `http.server` compatibility classes remain HTTP/1.1-shaped unless a later explicit compatibility decision changes them.

Do not weaken unrelated product-surface freeze clauses.

### A2. Update roadmap and capability documents

Synchronize at least:

- `plans/ROADMAP.md`;
- `docs/library-capability-matrix.md`;
- `docs/api-stability.md`;
- `docs/release-contract.md` or the current equivalent;
- `docs/http-primitives.md` where version semantics are described;
- `architecture/tls.md` after ALPN behavior actually changes;
- downstream app-server documentation where protocol assumptions are currently HTTP/1-only.

During Plan 183 implementation, documents should distinguish **planned** protocol capability from implemented capability. Do not claim H2/H3 support before their respective acceptance suites pass.

### A3. Preserve the Python compatibility decision

Record that `eggserve.server` and its `BaseHTTPRequestHandler`/`HTTPServer`-shaped compatibility semantics remain HTTP/1.1-oriented during this program. The first supported consumers of H2/H3 are:

1. the native Rust runtime;
2. the native CLI/static service;
3. downstream Rust consumers.

The Python low-level runtime may be reconsidered after H2 stabilizes, but no Python surface expansion is required by Plans 183–188.

## Track B — Define the version/API transition

### B1. Reserve a minor-version transition

Treat the stable canonical version expansion as a pre-1.0 breaking API change. Target the next suitable minor release (nominally `0.2.0` if no intervening release changes the numbering).

Do not bump versions merely when writing these plans. The bump belongs to the implementation/release closure once the changed API is coherent and documented.

### B2. Make future version growth survivable

Plan 184 should change the stable version type deliberately rather than repeating an exhaustive-enum break for each later protocol. Preferred direction:

```rust
#[non_exhaustive]
pub enum HttpVersion {
    Http10,
    Http11,
    Http2,
    Http3,
}
```

If a different representation is selected during implementation, it must preserve straightforward matching and stable display semantics without silently coercing unknown versions.

### B3. Eliminate lossy version conversion

The current best-effort conversion from Hyper's version type maps unsupported versions to HTTP/1.1. That behavior must disappear before H2 is enabled. Unsupported or unknown transport versions must be rejected/fallible, never relabeled.

## Track C — Establish protocol layering and ownership

### C1. Protocol-neutral core

The following remain shared across all protocols:

- `Service` and service admission;
- canonical method, target/URI metadata, headers, request body, lifecycle, connection info;
- canonical status/headers/response body;
- static filesystem confinement and planning;
- response privacy policy;
- server-wide application/file-stream admission;
- process/runtime observability context.

### C2. Protocol adapters

Own protocol-specific logic behind internal adapters/drivers:

- HTTP/1 framing/parser policy and close semantics;
- HTTP/2 stream/connection limits, flow control, GOAWAY/reset behavior;
- HTTP/3 QPACK/H3 request stream semantics, QUIC stream limits, STOP_SENDING/reset/GOAWAY behavior;
- protocol-specific transport negotiation and listener ownership.

No H2/H3 implementation type should become necessary for a downstream `Service` implementation.

### C3. Keep runtime configuration layered

Do not append every H2 and QUIC tuning field to the current flat `RuntimeConfig` indefinitely. Plan 184 must establish a protocol-configuration structure or equivalent internal authority that separates:

- shared runtime limits/timeouts;
- HTTP/1 parser/framing knobs;
- HTTP/2 stream/flow-control knobs;
- HTTP/3/QUIC transport knobs.

Stable compatibility configuration may continue to project into that model. Avoid breaking stable `ServeConfig`/`Limits` solely for aesthetic cleanup.

## Track D — Feature/dependency policy

### D1. Preserve the minimal build

HTTP/2 may add code to the existing Hyper dependency graph, but HTTP/3 must be optional. A minimal HTTP/1 build must not pull QUIC/H3 dependencies.

Choose feature names only after checking current crate conventions. A plausible shape is:

```text
default: current minimal behavior
http2: Hyper/Hyper-Util H2 support
http3: QUIC + H3 support (and required TLS primitives)
tls: existing direct TLS termination
```

Whether `http2` becomes default after qualification is a separate release decision; do not assume it in the implementation plan.

### D2. H3 implementation candidates

At implementation time, re-evaluate current maintained Rust H3/QUIC crates. The intended architecture is Hyperium H3 interfaces over Quinn (for example `h3` + `h3-quinn` + `quinn`) unless maintenance/security evidence favors a different stack.

Pin compatible versions deliberately, run `cargo audit`/`cargo deny`, and keep their types internal. Do not copy a stale version number from this plan without rechecking the ecosystem.

### D3. No optional-protocol dependency leakage

Tests must prove that disabling H3 removes the QUIC/H3 graph from the normal core build. If HTTP/2 is feature-gated, the same principle applies to H2-specific dependencies/features.

## Track E — Security invariants for all protocol work

Protocol support is not accepted merely because browsers can fetch a file. Each later plan must preserve these invariants:

- request metadata is bounded before service work;
- request bodies remain bounded and one-shot;
- application work remains bounded by the server-wide in-flight limit;
- file streams remain bounded by file-stream admission;
- malformed or rejected traffic cannot create unbounded per-connection/per-stream state;
- transport-generated headers and privacy policy remain runtime-owned;
- no service can inject protocol-forbidden hop-by-hop/framing fields;
- one rejected H2/H3 stream does not unnecessarily terminate unrelated streams;
- connection shutdown/drain is deterministic and bounded;
- cancellation wakes downstream waiters;
- H3 0-RTT is disabled until replay safety has a separate explicit policy;
- no protocol-specific implementation error text leaks to clients.

## Track F — Observability model

Extend observability only where protocol support creates genuinely new operator states. At minimum later plans should be able to distinguish:

- negotiated protocol (`http/1.0`, `http/1.1`, `h2`, `h3`);
- protocol handshake/negotiation failures;
- stream admission or transport-stream limit rejection;
- stream reset/cancellation versus whole-connection failure;
- GOAWAY/drain initiation;
- H3/QUIC handshake and endpoint failures.

Do not add a metrics exporter or tracing framework. Reuse the per-runtime `OpsContext` established by Plan 181.

## Track G — Documentation and release semantics

Before either new protocol is called supported, document:

- which features enable it;
- plaintext versus TLS behavior;
- ALPN behavior;
- default limits and their security rationale;
- graceful shutdown behavior;
- interaction with reverse proxies (deployment guidance only; EggServe does not become a proxy);
- Python compatibility limitations;
- platform/interop qualification status;
- what remains intentionally unsupported.

## Verification

Plan 183 is primarily a product/API architecture gate. Its implementation verification is documentation/API consistency rather than H2/H3 wire testing.

Run the ordinary repository verification after contract edits:

```bash
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-python-release-metadata.py
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
```

Also search the repository for stale categorical statements such as `HTTP/1.1 only`, `No HTTP/2`, `HTTP/2 is out of scope`, and `HTTP/2/3` in product-freeze prose. Preserve statements that remain intentionally true for the Python compatibility facade; update only the global runtime/product statements authorized here.

## Acceptance criteria

- [ ] `docs/non-goals.md` explicitly authorizes the narrow optional HTTP/2/3 program and still rejects unrelated edge/framework scope.
- [ ] repository roadmap/capability/API documents distinguish planned H2/H3 capability from currently implemented capability.
- [ ] a pre-1.0 minor API transition is reserved for stable `HttpVersion` expansion rather than shipping an exhaustive-enum break in a patch.
- [ ] the intended protocol layering is documented: one canonical service boundary, separate transport adapters.
- [ ] shared versus H1/H2/H3-specific configuration ownership is documented before new fields are added.
- [ ] HTTP/3 dependencies are required to remain optional/internal.
- [ ] Python `http.server` compatibility remains HTTP/1.1-shaped during this program.
- [ ] no WebSocket, WebTransport, reverse proxy, CONNECT tunnel, ACME, routing, middleware, upload, or ASGI/WSGI feature is implicitly authorized.
- [ ] Plans 184–188 are referenced as the required execution chain.
- [ ] ordinary Rust/Python/supply-chain verification remains green after the contract edits.

## Suggested implementation order

1. Update `docs/non-goals.md` and the roadmap with the narrow protocol expansion decision.
2. Record the API/minor-version transition for `HttpVersion` and protocol metadata.
3. Define the shared/protocol-specific configuration and lifecycle ownership model.
4. Update capability/stability/downstream documentation to show planned—not yet supported—H2/H3.
5. Run documentation/conformance searches for contradictory protocol claims.
6. Begin Plan 184 only after the repository contract is internally consistent.

## Handoff

Plan 183 is complete as the product/scope and API-transition gate. Plans 184–188
implemented the narrowly authorized protocol adapters, and Plans 189–190 own
the corrective qualification pass. H2 and H3 remain experimental; completion
of this gate never promoted either protocol or authorized unrelated edge-server
features.

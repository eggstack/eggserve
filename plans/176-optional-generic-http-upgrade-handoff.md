# Plan 176 — Optional Generic HTTP Upgrade Handoff

## Status

**PLANNED / OPTIONAL.**

Prerequisites: Plan 172 present. Prefer Plans 173–175 implemented/closed before this plan begins so upgraded-protocol support cannot distort the ordinary HTTP application-server boundary.

This plan is required only if a downstream application-server project needs WebSockets or another HTTP/1 upgrade protocol. HTTP-only downstream servers do not need it.

## Goal

Expose a narrow, transport-neutral way for a canonical EggServe `Service` to negotiate an HTTP/1 upgrade and receive the post-upgrade bidirectional byte stream, while EggServe continues to own HTTP parsing, validation, response framing, lifecycle limits, and the transition out of HTTP mode.

EggServe must not implement WebSocket framing, ASGI WebSocket events, ping/pong, fragmentation, close-code policy, permessage-deflate, or application protocol state machines.

The target architecture is:

```text
HTTP/1 request
     |
 EggServe parser/policy
     |
 canonical Service
     |
 accept/deny upgrade
     |
 EggServe sends validated 101 handshake
     |
 canonical upgraded-IO handoff
     |
 downstream protocol codec
 (WebSocket is one consumer)
```

## Current state

The connection executor currently drives Hyper with `.with_upgrades()`, so the transport implementation is already capable of upgrade processing internally. However, canonical `Request` deliberately contains only request head/body/connection metadata, and `Service` returns only canonical HTTP `Response`.

There is no public way for a canonical service to:

- retain an upgrade capability associated with the parsed request;
- accept or deny it through an EggServe-owned API;
- receive the post-handshake bidirectional IO without naming Hyper's `OnUpgrade`/`Upgraded` types;
- bind that IO to EggServe shutdown/cancellation accounting.

A downstream WebSocket-capable app server would otherwise need to bypass the canonical boundary and depend directly on Hyper, defeating the purpose of Plans 172–175.

## Design principles

1. **Upgrade is a separate outcome, not a strange response body.** A WebSocket connection is no longer an HTTP response byte stream after the handshake. Do not overload `ResponseBody::Stream` to represent duplex upgraded IO.
2. **Hyper remains internal.** Public APIs expose EggServe-owned upgrade capability and IO wrappers/traits only.
3. **HTTP handshake remains validated.** EggServe must not let downstream code write arbitrary raw handshake bytes or framing headers.
4. **The downstream protocol owns post-upgrade bytes.** EggServe does not parse WebSocket frames or know ASGI event semantics.
5. **Lifecycle remains bounded.** Shutdown, hard connection lifetime if configured, and cancellation must be able to terminate upgraded connections; no detached raw stream may escape runtime accounting indefinitely.
6. **Ordinary services pay little or no complexity cost.** Upgrade support should be optional in API usage and feature/dependency cost where practical.

## Phase 0 — Upgrade semantics spike

Before choosing a public API, build an internal proof using current Hyper 1.11 behavior.

Verify:

- which request forms produce a usable `OnUpgrade` capability;
- when Hyper requires `101 Switching Protocols` and which handshake headers it validates versus leaves to the application;
- whether the post-upgrade IO includes unread buffered bytes and how Hyper exposes them;
- how cancellation/shutdown interacts with `OnUpgrade` before and after completion;
- how current `ProgressIo`/connection activity instrumentation behaves after upgrade;
- whether HTTP connection driver completion currently waits for upgraded tasks or considers the HTTP connection complete at handoff;
- TLS and caller-owned transport behavior.

Record these facts in implementation notes/tests. Do not assume `.with_upgrades()` alone supplies the lifecycle ownership EggServe needs.

## Track A — Request-side upgrade capability

Expose upgrade eligibility/capability without exposing Hyper.

Possible model:

```rust
pub struct Request {
    // existing head/body/connection
    upgrade: Option<UpgradeRequest>,
}

pub struct UpgradeRequest { /* opaque one-shot capability */ }
```

or an equivalent separate request context.

Requirements:

- capability is one-shot;
- cloning ordinary `RequestHead` does not clone upgrade ownership;
- malformed/non-upgrade requests have no capability;
- dropping/ignoring the capability leaves ordinary HTTP behavior safe;
- a service can inspect headers itself for application-specific protocol/subprotocol policy, but cannot fabricate a transport capability;
- capability remains associated with the same request/connection lifecycle token from Plan 174.

Avoid embedding HTTP header-name-specific WebSocket helpers in core. Generic convenience such as `request.upgrade().is_some()` is enough.

## Track B — Service outcome model

The current `Service::call()` returns `Result<Response, ServiceError>`. An upgrade requires both a handshake response and a continuation/handoff.

Evaluate two approaches.

### Option 1 — Additive upgraded response type

Add a canonical variant/wrapper such as:

```rust
pub enum ServiceOutcome {
    Response(Response),
    Upgrade(UpgradeResponse),
}
```

with a compatibility adapter/default trait path so existing `Service` implementations need not change immediately.

### Option 2 — Upgrade embedded in `Response`

Attach an optional one-shot upgrade continuation to `Response` metadata while keeping `ResponseBody` ordinary/empty for the handshake.

This can minimize trait churn but risks making every HTTP response carry upgrade-specific machinery.

### Decision criteria

Choose the design that:

- preserves straightforward ordinary `Service` implementations;
- cannot pair a continuation with an invalid/non-101 handshake accidentally;
- does not represent duplex IO as `ResponseBody`;
- maintains normalization/framing authority;
- makes one-shot ownership obvious to the type system;
- avoids a source break to semver-considered primitives if an additive experimental-server type is cleaner.

Do not introduce a broad generalized protocol enum (`Http`, `WebSocket`, `H2`, etc.). This plan is only HTTP/1 upgrade handoff.

## Track C — Handshake validation and normalization

EggServe should continue owning generic HTTP invariants while downstream code owns protocol-specific handshake policy.

The runtime must at minimum ensure:

- the status is valid for an accepted HTTP/1 upgrade (normally 101);
- forbidden framing headers/body semantics cannot be smuggled into the handshake;
- hop-by-hop handling does not strip headers that are required specifically for the validated upgrade transition after it has been deliberately accepted;
- ordinary responses to an upgrade request can deny/reject without entering upgrade mode;
- HEAD/body-forbidden normalization does not create invalid upgrade behavior;
- an upgrade continuation cannot be used after a normal response has committed.

This likely requires a narrowly scoped response-normalization path for an accepted upgrade. Do not globally weaken hop-by-hop stripping for normal responses.

Protocol-specific validation remains downstream. For example, a WebSocket implementation is responsible for `Sec-WebSocket-Key`, version, subprotocol negotiation, and `Sec-WebSocket-Accept` correctness unless a future separate generic helper is justified.

## Track D — Transport-neutral upgraded IO

The post-upgrade object must not expose `hyper::upgrade::Upgraded` publicly.

Preferred public semantics are an EggServe-owned type implementing standard Tokio IO traits:

```rust
pub struct UpgradedIo { /* opaque */ }

impl AsyncRead for UpgradedIo { ... }
impl AsyncWrite for UpgradedIo { ... }
```

If trait-object erasure is used internally, keep concrete dependencies hidden.

Requirements:

- `Send + 'static` so downstream protocol tasks can own it;
- single-owner duplex IO; no implicit cloning;
- buffered bytes supplied by Hyper are preserved correctly;
- standard AsyncRead/AsyncWrite backpressure applies;
- shutdown/cancellation can force termination;
- no API to recover Hyper internals;
- caller-owned transports remain supported if Hyper supports upgrade over them.

Do not promise `Sync` unless there is a concrete need. A single task or explicit downstream split is the natural ownership model.

## Track E — Lifecycle and shutdown accounting

Upgraded connections outlive the HTTP request/response transaction, so they need explicit runtime accounting.

Define:

- whether an upgraded connection continues to count against `max_connections` (expected: yes);
- whether it counts against ordinary service admission after handshake (expected: no; downstream protocol/application admission is separate);
- which EggServe timeouts remain active after upgrade;
- how graceful shutdown treats upgraded sessions;
- how forced shutdown cancels them;
- how `ServerHandle::wait()` accounts for upgraded tasks;
- how caller-owned `ConnectionShutdown` propagates after handoff.

### Timeout recommendation

Do not apply HTTP keep-alive idle timeout or HTTP response write timeout blindly to arbitrary upgraded protocols; their activity semantics differ.

Prefer:

- hard connection lifetime, if configured, remains an outer bound;
- server shutdown remains an outer bound;
- downstream protocol implementation owns protocol heartbeat/read/write idle semantics;
- EggServe optionally exposes transport byte-progress observability but does not impose HTTP-specific idle policy after transition.

Document the exact boundary.

## Track F — Cancellation integration

Integrate with Plan 174's request/connection lifecycle primitive.

Before upgrade completes:

- peer disconnect/runtime cancellation wakes request lifecycle observers and fails the upgrade capability.

After upgrade completes:

- expose either the same underlying connection cancellation token or a dedicated `UpgradeLifecycle` sharing the same cancellation source;
- downstream protocol tasks can await cancellation without probing the IO;
- local drop/close of the upgraded IO terminates the transport and marks lifecycle terminal.

Do not attempt to encode WebSocket close codes/reasons in this generic lifecycle object.

## Track G — Denial path

A service receiving an upgrade request must be able to return a normal canonical HTTP response instead of accepting the upgrade.

Requirements:

- denial remains ordinary HTTP and uses existing response normalization/policy;
- the unused upgrade capability is dropped safely;
- request-body policy remains explicit (WebSocket handshakes normally have no body, but the generic API must not assume that for all protocols);
- connection reuse follows normal HTTP rules when denial response/request framing permits it.

This is useful for ASGI WebSocket denial extensions downstream, but EggServe should not name or implement that extension.

## Track H — Reference echo-protocol test only

To prove genericity without implementing WebSockets, create a tiny test-only upgraded protocol.

Example:

1. client sends a syntactically valid generic `Upgrade: eggserve-test` request;
2. service accepts with a valid 101 transition;
3. downstream task receives `UpgradedIo`;
4. client sends raw bytes;
5. task echoes/transforms bytes;
6. shutdown/disconnect terminates both ends.

This test proves the handoff while avoiding a WebSocket dependency and protocol implementation in core.

If useful, a separate downstream/fixture test may use a WebSocket crate to prove real-world interoperability, but that dependency must remain dev-only and should not cause EggServe to own WebSocket semantics.

## Security review points

Upgrade support creates a sharp boundary. Explicitly test:

- no upgrade smuggling from malformed `Connection`/`Upgrade` headers;
- request header duplicate/casing behavior remains correct;
- accepted upgrade cannot retain HTTP request bytes as misframed application data except documented Hyper buffered bytes that belong after the boundary;
- normal response hop-by-hop stripping remains unchanged;
- upgrade-specific hop-by-hop exceptions apply only to a deliberate accepted upgrade;
- no response body is sent with 101;
- no second HTTP response is attempted after upgrade commitment;
- cancellation cannot leave an untracked raw transport alive;
- logging does not dump arbitrary upgraded payload bytes;
- admission permits are recovered at handoff;
- malicious downstream services cannot call upgrade twice.

## Verification

Run the standard workspace/API/transport suites plus upgrade tests. At minimum:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p eggserve-core --test public_api_consumers
cargo test -p eggserve-core --test api_stability
cargo test -p eggserve-core --test transport_driver
cargo check -p eggserve-core --examples
cargo test --doc -p eggserve-core
bash scripts/verify-cargo-packages.sh --mode all
```

Add deterministic tests for:

- successful generic upgrade over TCP;
- denial with normal HTTP response;
- capability dropped without use;
- duplicate/double acceptance rejected;
- peer disconnect before handshake completion;
- disconnect after handoff;
- graceful and forced shutdown with active upgraded IO;
- hard lifetime expiry if enabled;
- TLS upgrade;
- caller-owned IO upgrade if supported;
- buffered post-handshake bytes preserved;
- ordinary static/native responses unchanged.

Use a WebSocket interoperability smoke only as optional dev evidence; it must not be required to define the core API.

## Compatibility and feature policy

The `server` module is experimental, so an additive `UpgradeRequest`, `UpgradeResponse`, `UpgradedIo`, or service-outcome abstraction can evolve there without destabilizing the semver-considered primitive facade unnecessarily.

Prefer keeping upgrade ownership types in `server` rather than adding them to `primitives` unless a canonical transport-independent type truly needs stable application-facing status.

Do not add a default-on WebSocket dependency. Ideally this feature uses only existing Hyper/Tokio capabilities already in the runtime; if no new dependency is required, a Cargo feature may be unnecessary. If additional protocol dependencies appear, stop—the scope has likely crossed into downstream territory.

## Acceptance criteria

- [ ] a canonical service can determine that an HTTP/1 request has a real one-shot upgrade capability without importing Hyper;
- [ ] a service can explicitly accept or deny the upgrade;
- [ ] an accepted upgrade produces a validated HTTP handshake without weakening normal response normalization;
- [ ] post-handshake IO is exposed through an EggServe-owned `AsyncRead + AsyncWrite` type with no Hyper type leakage;
- [ ] buffered transition bytes are preserved correctly;
- [ ] upgraded connections remain counted/tracked for connection lifetime and server shutdown;
- [ ] HTTP-specific service admission is released after handshake while downstream protocol admission remains downstream-owned;
- [ ] request/connection cancellation propagates before and after handoff;
- [ ] shutdown cannot leave an untracked upgraded transport alive;
- [ ] a test-only generic echo protocol proves duplex operation;
- [ ] denial remains ordinary canonical HTTP;
- [ ] TCP/TLS/caller-owned parity is documented and tested where supported;
- [ ] no WebSocket codec, ASGI events, framework semantics, or protocol heartbeat policy enters EggServe core.

## Non-goals

Do not add:

- WebSocket frame parsing/serialization;
- WebSocket ping/pong or fragmentation;
- permessage-deflate;
- ASGI `websocket.*` events;
- HTTP CONNECT tunneling;
- arbitrary reverse-proxy tunneling;
- HTTP/2 extended CONNECT;
- HTTP/3/WebTransport;
- protocol-specific subprotocol registries;
- application authentication/authorization;
- raw Hyper `OnUpgrade`/`Upgraded` in public APIs.

## Handoff

Once this plan closes, a separate downstream application-server project can add WebSocket support by pairing EggServe's generic HTTP upgrade handoff with a WebSocket codec and its own application-protocol adapter.

If a real downstream WebSocket implementation reveals a missing generic lifecycle capability, correct the narrow upgrade abstraction here; do not move WebSocket semantics into EggServe.
# Plan 184 — Protocol-Neutral Runtime Preparation and Overlap Cleanup

## Status

**IMPLEMENTED — HTTP/1 regression-closed prerequisite refactor; no HTTP/2 or
HTTP/3 listener is enabled by this plan.**

Prerequisite: Plan 183's scope/API contract must be accepted and implemented first. This plan must preserve HTTP/1 wire behavior and the current Python compatibility surface while removing protocol-specific assumptions that would otherwise be duplicated by Plans 185 and 187.

## Purpose

Prepare the current HTTP/1-focused runtime for multiplexed protocols by removing the remaining feature overlap and by separating protocol-neutral request/service policy from HTTP/1-specific wire actions.

The desired end state is not a generic framework. It is a smaller and clearer internal kernel in which:

- request-target classification has one authority;
- canonical request metadata can represent HTTP/1, HTTP/2, and HTTP/3 without pseudo-header leakage;
- service admission/invocation/error conversion exists once rather than once per request-body mode;
- lifecycle decisions such as "this request cannot keep using the transport" are represented independently of `Connection: close`;
- connection/stream activity can later support multiplexing without changing the `Service` trait;
- HTTP/1 parser settings are recognized as HTTP/1 settings instead of global transport settings;
- unused upgrade capability is removed;
- stable compatibility types remain available through adapters where removal would create needless churn.

## Current-state findings

### 1. Request-target parsing has two authorities

There are currently two origin-form validators:

- `crates/eggserve-core/src/primitives/request_target.rs::RequestTarget::parse()`;
- `crates/eggserve-core/src/path/request_target.rs::parse_origin_form()` used by `ConfinedPath`.

They already disagree. Canonical `RequestTarget` rejects network-path/`//...` forms as authority-like input, while direct `ConfinedPath` parsing accepts repeated leading slashes and normalizes them. They also expose different error taxonomies.

This is protocol syntax duplication in a security-sensitive path and must be resolved before H2/H3 introduce `:path`/`:authority` inputs.

### 2. Service invocation is repeated across body-policy branches

`server/connection/pipeline.rs` must keep different body acquisition behavior for `Reject`, `Buffer`, and `Stream`, but all three eventually repeat the same core sequence:

1. application/service admission;
2. `Request` construction;
3. `Service::call()`;
4. panic containment;
5. handler timeout;
6. service-error logging/translation;
7. canonical response normalization/conversion;
8. final response tracking.

That sequence should have one implementation before a second protocol driver is added.

### 3. HTTP/1 lifecycle decisions leak into shared response handling

Body rejection, body-consumption failure, max-requests-per-connection, and several other paths directly add `Connection: close`. This is safe for HTTP/1, but HTTP/2/3 require stream reset/cancel or GOAWAY/drain semantics instead.

The shared pipeline must decide *what lifecycle outcome is required*; the active protocol driver/adapter must decide *how to encode it*.

### 4. Write-progress and activity tracking are connection-centric

`ProgressIo` and `ConnectionActivity` currently observe aggregate socket reads/writes. That is sufficient for one serial HTTP/1 connection, but aggregate connection writes cannot prove that every multiplexed response stream is making progress.

This plan does not implement H2 stream flow control, but it must make room for per-request/per-stream activity ownership rather than baking all response progress into one connection timestamp.

### 5. HTTP/1 parser configuration is mixed into the flat runtime model

Fields such as `max_buf_size` and `max_headers` are explicitly Hyper HTTP/1 parser knobs, while `max_header_bytes`, request-target limits, handler/body timeouts, and service admission are broader application/runtime policy.

Adding H2/H3 fields to the same flat namespace would create long-term ambiguity and front-end drift.

### 6. Upgradeable HTTP/1 connections are enabled with no public upgrade capability

The HTTP/1 driver calls `.with_upgrades()` and therefore drives Hyper's upgradeable connection type, but the product contract explicitly provides no WebSocket/generic-upgrade handoff and no `OnUpgrade` capability escapes the canonical boundary.

Remove this latent capability unless implementation evidence shows Hyper requires it for an existing supported behavior. HTTP/2 prior knowledge/ALPN must not be implemented by reviving the obsolete HTTP/1 Upgrade path.

### 7. Canonical version conversion is lossy

`HttpVersion` contains only HTTP/1.0 and HTTP/1.1, while `From<&hyper::http::Version>` currently maps any other version to HTTP/1.1 as a best-effort fallback. That can mislabel metadata as soon as H2 is accepted.

### 8. Static planning retains a second response vocabulary

The stable static planning API (`ResponseStatus`, `HeaderMapPlan`, `StaticResponsePlan`, `BodyPlan`) coexists with canonical `StatusCode`, `HeaderBlock`, `Response`, and `ResponseBody`.

This overlap is compatibility debt, not an emergency. Do not break the stable planner API merely to remove type names. New internal runtime/static-service code should converge on canonical values and keep one explicit compatibility adapter for the planner surface.

## Design constraints

- Preserve all currently accepted HTTP/1 wire behavior unless an explicit 0.2 migration note authorizes a correction.
- Do not enable HTTP/2 or add QUIC/H3 dependencies in this plan.
- Do not add middleware, routing, raw response writers, upgrade handoff, or application framework abstractions.
- Keep Hyper types inside transport/adaptation modules.
- Keep `Service` transport-independent.
- Preserve one final response privacy boundary.
- Preserve Plan 179's canonical shared-limit authority; refactor ownership without recreating duplicated defaults/validation.
- Preserve Plan 181's per-runtime `OpsContext`; do not reintroduce process-global runtime state.
- Keep Python `http.server` compatibility behavior unchanged.

## Track A — Make canonical request-target parsing authoritative

### A1. Choose one HTTP syntax classifier

Use canonical `RequestTarget` as the authority for request-target syntax/classification. Filesystem confinement should operate on a validated path component and should not independently classify origin/absolute/authority/asterisk forms.

A preferred shape is:

```text
transport parser / URI adapter
          |
          v
canonical RequestTarget
          |
          +--> application RequestHead
          |
          +--> validated path component
                    |
                    v
              ConfinedPath
```

### A2. Separate path-component confinement from request-target syntax

Introduce an internal or appropriately scoped `ConfinedPath` constructor that accepts the already-selected path component rather than reparsing HTTP syntax. Keep percent decoding, repeated-encoding handling, separator ambiguity, dot/dotdot rejection, Windows prefix/name/ADS policy, dotfile policy, and component normalization in the confinement layer.

Do not move filesystem security logic into `RequestTarget`.

### A3. Resolve stable direct-`ConfinedPath::parse` semantics

`ConfinedPath` is a documented stable primitive. If its public `parse(raw, policy)` function currently accepts forms canonical `RequestTarget` rejects, use the Plan 183 minor-version transition deliberately:

- either make `parse` a compatibility wrapper through canonical target parsing and document the behavior correction;
- or preserve its historical path-component semantics under a clearly named path-only API while ensuring static/runtime code never has two HTTP classifiers.

Do not leave two subtly different "request target" parsers after this track.

### A4. Add one shared conformance corpus

Add table-driven tests that exercise both canonical request parsing and the confinement handoff for:

- `/`;
- normal path/query;
- empty query;
- repeated `/` within a path;
- leading `//` and `///` according to the chosen 0.2 contract;
- `*`;
- absolute-form URI;
- authority-form;
- controls/whitespace/NUL;
- `#` behavior;
- encoded separators;
- malformed percent encoding;
- over-limit target length at the runtime layer.

A future H2/H3 `:path` adapter must consume the same canonical parser/corpus.

## Track B — Complete the `HttpVersion` API transition

### B1. Remove fallback coercion

Replace the infallible best-effort Hyper-version conversion with a fallible conversion or explicit transport match. No unsupported version may silently become `Http11`.

### B2. Expand stable canonical versions under the minor transition

Implement the version representation approved by Plan 183. Preferred enum:

```rust
#[non_exhaustive]
pub enum HttpVersion {
    Http10,
    Http11,
    Http2,
    Http3,
}
```

Update display/parse/major/minor behavior and conformance tests. HTTP/2's conventional display should be specified consistently (`HTTP/2` versus internal enum naming); HTTP/3 likewise. Do not invent an HTTP/3 request-line representation—`Display` is descriptive canonical metadata, not proof that the protocol has a textual request line.

### B3. Keep wire acceptance separate

Adding canonical variants does not mean the HTTP/1 driver accepts H2/H3 bytes. The current H1 parser remains H1-only until Plan 185 adds H2 negotiation; H3 remains unavailable until Plan 187.

## Track C — Add canonical authority/effective request metadata

### C1. Define authority independently of raw headers

HTTP/2 and HTTP/3 carry `:authority`; HTTP/1 commonly carries `Host`. Downstream application-server consumers need one canonical, validated notion of request authority without seeing pseudo-header names.

Add a canonical value/type or request-head accessor that can represent the effective authority as bytes/text under a defined validation domain. The design must cover:

- host name;
- optional port;
- IPv6 literal form;
- absence where protocol rules permit it;
- validation/conflict handling between HTTP/1 request target authority and `Host` where relevant;
- H2/H3 `:authority` mapping.

Do not turn forwarded headers into trusted authority metadata.

### C2. Define scheme ownership

Continue to derive trustworthy `http`/`https` semantics from `ConnectionContext`/transport knowledge. H2/H3 pseudo-header `:scheme` must be validated against the active transport/protocol contract rather than blindly trusted as application metadata.

If the canonical request needs a per-request effective scheme accessor, make it derive from validated transport/request metadata without duplicating `ConnectionInfo` truth.

### C3. Keep pseudo-headers internal

Services should not receive literal `:method`, `:path`, `:scheme`, or `:authority` entries in `HeaderBlock`. Protocol adapters convert pseudo-fields into canonical method/target/scheme/authority fields before service invocation.

## Track D — Extract one service invocation kernel

### D1. Isolate body preparation from service invocation

Keep body-mode-specific logic responsible for:

- whether a body is accepted;
- buffering versus deferred streaming;
- declared-length validation;
- body read timeout;
- deferred-body watchdog/lifecycle tracking.

After a complete canonical `Request` is ready, call one shared helper for service admission/invocation.

### D2. Centralize service execution semantics

The helper should own exactly once:

- server-wide in-flight permit acquisition;
- handler timeout calculation supplied by the caller;
- panic containment;
- service-error classification/logging;
- generic error response creation;
- canonical response normalization/conversion entry;
- release of service admission on every exit.

Do not hide request-body lifetime behavior inside this helper; deferred-body policy remains explicit in the surrounding branch.

### D3. Add branch-equivalence tests

For equivalent services/responses, prove `Reject` without a body, `Buffer`, and `Stream` all use the same service error, panic, timeout, privacy, and response normalization behavior.

## Track E — Introduce protocol-neutral lifecycle dispositions

### E1. Stop using response headers as the internal decision API

Represent lifecycle needs as internal typed state/disposition rather than immediately inserting `Connection: close` in generic pipeline code.

Exact type design is implementation-dependent, but it must be able to express at least:

- response may keep the transport active;
- response completes, then this HTTP/1 connection must close;
- inbound request body/stream must be cancelled after the response decision;
- whole connection should begin graceful drain after a configured request threshold;
- unrecoverable transport/framing state requires termination.

### E2. Map dispositions at the HTTP/1 adapter

For this plan, only HTTP/1 behavior is implemented. Map neutral dispositions back to the exact existing HTTP/1 outcomes, including `Connection: close` where currently required.

The result should be wire-equivalent HTTP/1 behavior with no generic service/policy helper hard-coded to HTTP/1 connection headers.

### E3. Preserve final-boundary ownership

`finalize_runtime_response` or its successor remains the one place that applies runtime-owned response privacy/header policy. Protocol-forbidden headers must be stripped/rejected before the final protocol response is emitted.

## Track F — Split connection activity from future stream activity

### F1. Retain connection-level state

Connection-level activity still owns:

- connection start/lifetime;
- aggregate transport read/write progress where meaningful;
- active request count;
- graceful shutdown state;
- connection-level idle detection;
- connection-level observability/correlation.

### F2. Introduce request/response activity identity

Create an internal request/stream activity record or hook that can later track one response independently of aggregate socket writes. HTTP/1 may have only one active response stream at a time, so its mapping can remain trivial.

The future H2/H3 adapters need a place to record:

- response body progress;
- request body progress;
- cancellation/reset;
- per-stream timeout state;
- completion exactly once.

Do not implement H2 stream IDs in the stable service API. Internal protocol correlation IDs are sufficient.

### F3. Preserve RAII/exactly-once accounting

Any refactor must retain the current exactly-once permit/counter/body-completion guarantees. Add regression tests for timeout, dropped body, disconnect, service panic, cancellation, and shutdown.

## Track G — Separate shared and protocol-specific runtime configuration

### G1. Inventory each current field by owner

Classify `RuntimeConfig` fields as:

- server-wide/shared;
- HTTP/1-specific;
- TLS transport-specific;
- static-service-specific (which should not be in runtime config);
- frontend-only.

Do not move stable `ServeConfig`/`Limits` fields solely for symmetry.

### G2. Introduce protocol subconfiguration or equivalent internal authority

Establish a structure that allows Plans 185/187 to add H2/H3 controls without flattening them into the existing namespace. A plausible experimental/internal model is:

```text
RuntimeConfig
  shared runtime policy
  http1: Http1Config
  http2: Option<Http2Config>     # added Plan 185
  http3: Option<Http3Config>     # added Plan 187
```

The exact public migration must respect the Plan 183 0.2 transition and compatibility adapters.

### G3. Preserve Plan 179 validation authority

Do not recreate multiple default tables. Shared values remain validated once. Each protocol config owns only its protocol-specific defaults/bounds/cross-field validation.

Add projection tests proving legacy `Limits`/`ServeConfig` still produce the same HTTP/1 runtime values.

## Track H — Remove unused upgrade machinery

Remove `.with_upgrades()` from the supported HTTP/1 driver if the existing test suite confirms no supported capability depends on it.

Update comments/types that currently mention `UpgradeableConnection`. Use the ordinary connection future and keep graceful shutdown support.

Add a regression test proving ordinary keep-alive/shutdown behavior remains unchanged. Do not expose any new upgrade API.

## Track I — Reduce static response/config overlap without breaking compatibility

### I1. Prefer canonical response values internally

Where static service internals construct a planner result only to immediately convert it into canonical response types, consider moving the internal path toward canonical `StatusCode`/`HeaderBlock`/`ResponseBody` while retaining stable planner functions as adapters.

Do this only where it materially removes duplicate conversion/validation logic. The goal is one runtime representation, not deleting useful pure planner APIs.

### I2. Decouple `StaticServiceBuilder` from full runtime compatibility configuration

`StaticServiceBuilder` currently constructs a full `ServeConfig` even though the service needs static root/policy/metadata/error representation rather than listener/runtime configuration.

Introduce a small internal static-service configuration/state if this can be done without stable API churn. `ServeConfig` should adapt into it. Do not create a new public config type unless a consumer need is demonstrated.

This track is lower priority than A–H and may be deferred if it threatens the protocol prerequisite schedule; if deferred, record the remaining overlap explicitly rather than partially refactoring it.

## Track J — MSRV and maintenance verification

The workspace currently declares Rust 1.87 while ordinary CI tests `stable`. Add a cheap MSRV `cargo check` path or remove/adjust the claim if 1.87 is no longer supported.

Prefer one lightweight check in an existing workflow over a new matrix/job explosion. Ensure protocol feature additions in later plans are also checked against the supported MSRV or explicitly revise MSRV during the minor release.

## Implementation closure

Tracks A–H and J are implemented. `RequestTarget` is now the sole HTTP syntax
classifier, with `ConfinedPath::from_path_component` as the path-only security
handoff. Canonical version metadata is non-exhaustive and fallible at the
Hyper boundary; `Authority` is the protocol-neutral effective-host field.
Body-policy branches converge on one service invocation kernel, lifecycle
requirements use typed `LifecycleDisposition` state, request/response activity
has an internal per-request identity, HTTP/1 parser knobs project through
`Http1Config`, and Hyper upgrade machinery is disabled.

Track I2 is deliberately deferred: `StaticService` still accepts
`ServeConfig` through its existing compatibility adapter because the current
consumer-facing `ServeConfig`/`ServeState` ownership is useful and the
refactor would otherwise expand this protocol prerequisite. No new coupling
was added; future static-service work should introduce a small internal static
state only when it removes a measured conversion/ownership cost. Track I1 is
closed by retaining planner APIs as compatibility adapters while the runtime
continues to consume canonical responses.

## Verification

Run the full existing regression set with HTTP/2/3 still disabled:

```bash
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-python-release-metadata.py
cargo fmt --all -- --check
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features tls
bash scripts/test-python-wheel.sh
```

Add focused HTTP/1 wire regression coverage for:

- keep-alive reuse;
- body rejection and close behavior;
- handler timeout;
- response-write timeout;
- max requests per connection;
- graceful shutdown;
- TLS HTTP/1.1 ALPN behavior;
- caller-owned `serve_http1_connection`;
- static file GET/HEAD/range/conditional responses;
- request-target corpus before/after parser consolidation.

If an MSRV gate is added, run it explicitly as part of closure.

## Acceptance criteria

- [x] there is one authoritative HTTP request-target classifier; confinement no longer independently reimplements HTTP target-form classification.
- [x] canonical/path direct-use behavior changes are documented under the planned 0.2 transition and covered by a shared corpus.
- [x] `HttpVersion` no longer silently coerces unsupported versions to HTTP/1.1.
- [x] canonical version metadata is ready to represent HTTP/2 and HTTP/3 without enabling either wire protocol prematurely.
- [x] canonical request metadata has a defined authority/scheme model suitable for H1/H2/H3; pseudo-headers do not leak into ordinary headers.
- [x] service admission/invocation/panic/timeout/error-response logic exists in one shared path after body preparation.
- [x] generic pipeline code no longer uses `Connection: close` itself as the lifecycle decision representation.
- [x] HTTP/1 maps neutral lifecycle dispositions back to current close/keep-alive behavior exactly.
- [x] connection-level versus request/stream-level activity ownership is explicit and ready for multiplexed protocols.
- [x] runtime configuration has a clear shared/HTTP1/protocol-extension ownership model without duplicating Plan 179 defaults.
- [x] `.with_upgrades()` and upgradeable connection machinery are removed unless a documented supported dependency requires them.
- [x] stable static planner APIs remain functional; new internal work does not deepen duplicate response vocabularies.
- [x] `StaticServiceBuilder` no longer needs full runtime compatibility config internally, or the deferral is explicitly documented with no new coupling added.
- [x] the declared MSRV is checked or deliberately revised.
- [x] all HTTP/1, TLS, Python, and supply-chain regressions remain green.
- [x] no HTTP/2 or HTTP/3 listener is enabled by this plan.

## Suggested implementation order

1. Consolidate request-target parsing and lock the chosen 0.2 semantics with corpus tests.
2. Complete the `HttpVersion` transition and authority/scheme canonical metadata.
3. Extract the shared service invocation kernel from body-policy branches.
4. Introduce protocol-neutral lifecycle dispositions and map them back to H1.
5. Refactor activity tracking to distinguish connection and request/response progress identities.
6. Separate protocol-specific runtime configuration ownership while preserving compatibility projections.
7. Remove unused upgradeable connection machinery.
8. Perform the optional static planner/config overlap cleanup if it remains low-risk.
9. Add/verify the MSRV gate and run the complete H1/TLS/Python regression suite.
10. Begin Plan 185 only after HTTP/1 behavior is demonstrably unchanged.

## Handoff

After Plan 184, adding HTTP/2 should require a new protocol driver/configuration and H2-specific lifecycle mappings, not a fork of request validation or service execution. If Plan 185 still needs to copy `pipeline.rs`, re-parse request targets, or insert H1 connection headers in shared code, this plan is not complete.

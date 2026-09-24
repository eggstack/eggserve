# Plan 280 — Direct H1 explicit external policy ownership

## Status

**CLOSED — implementation, local qualification, and exact-SHA hosted CI passed. Registry artifact qualification is Plan 286.**

Hosted CI: run 36067050590, SHA `c62faf59b19913eb49b97d371435122c5a8fb6ac` (success).

## Purpose

Allow a direct H1 embedder to explicitly retain ownership of selected
application/runtime policy that EggServe currently enforces unconditionally,
while leaving the existing bounded EggServe behavior unchanged by default.

This plan is generic substrate work. It must not mention or encode any
downstream project's configuration names, response types, metrics, or routing
logic.

## Problem statement

The current direct H1 driver consumes one `RuntimeConfig` and always applies
all of these policies:

- `handler_timeout`;
- `body_read_timeout`;
- `keep_alive_idle_timeout`;
- `response_write_timeout`;
- `max_request_body_bytes`;
- `max_request_target_bytes`.

For an embedding server that already owns one or more of those policies, the
only current approximation is to choose very large values. That is not a
truthful ownership contract: it leaves two policy authorities and can still
silently narrow behavior.

`connection_total_timeout` already has an explicit disabled state through
`Duration::ZERO`; this plan generalizes the *ownership concept* without
reusing zero where zero already has another meaning.

## API direction

Preserve the existing public `RuntimeConfig` field types and the existing
`serve_http1_connection(...)` entry point.

Add an explicit policy-ownership object used only by new opt-in embedding
paths. Suggested vocabulary:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyOwner {
    EggServe,
    External,
}

#[derive(Debug, Clone)]
pub struct H1PolicyOwnership {
    pub handler_deadline: PolicyOwner,
    pub request_body_deadline: PolicyOwner,
    pub keep_alive_idle_deadline: PolicyOwner,
    pub response_write_progress_deadline: PolicyOwner,
    pub global_request_body_ceiling: PolicyOwner,
    pub request_target_ceiling: PolicyOwner,
}
```

Exact naming may change, but requirements are fixed:

- the type must be explicit and non-boolean at the public boundary;
- `Default` must select `EggServe` for every field;
- there must be a named all-default constructor/profile;
- an embedder must opt out one policy at a time;
- no field may become externally owned merely because a numeric value is
  unusually large;
- no existing caller changes behavior unless it chooses the new API.

Preferred additive entry point:

```rust
serve_http1_connection_with_policy(
    io,
    service,
    config,
    ownership,
    context,
    runtime_state,
    shutdown,
)
```

The existing `serve_http1_connection` remains a wrapper using
`H1PolicyOwnership::default()`.

If a builder-level hook is added for `Server`, it must require an explicit
method such as `.policy_ownership(...)`; the ordinary server builder keeps all
EggServe ownership.

## Track A — Handler deadline ownership

Current authority:
`connection::response::invoke_canonical_service` always wraps
`Service::call` in `tokio::time::timeout(handler_timeout, ...)`.

When handler ownership is `EggServe`:

- behavior is unchanged;
- timeout still maps to the existing sanitized 504 path;
- panic containment remains active.

When ownership is `External`:

- EggServe must not create a handler deadline;
- panic containment remains active;
- request lifecycle cancellation from transport/shutdown remains active;
- no artificial duration is substituted;
- the external service may still choose its own deadline.

Refactor the shared H1/H3 invocation helper carefully. Do not silently change
H3 semantics. If H3 cannot consume the new ownership contract without
broadening Plan 280, keep its existing EggServe-owned path and expose a narrow
H1-specific wrapper rather than changing H3 policy accidentally.

## Track B — Request-body deadline ownership

Current authority includes buffered body reads and the deferred-body watchdog.

When ownership is `EggServe`:

- current `body_read_timeout` behavior remains exact.

When `External`:

- EggServe must not create a total body-consumption deadline/watchdog;
- body framing, decoded-byte accounting, lifecycle state, trailer validation,
  and disconnect/shutdown cancellation remain EggServe-owned;
- the service can still observe body transport errors;
- no body task may become detached or immortal solely because the watchdog is
  disabled.

Audit both buffered and streamed/deferred body paths. A partial implementation
that disables only the eager read timeout but leaves the deferred watchdog
active is not acceptable.

## Track C — Keep-alive idle deadline ownership

When `EggServe`:

- current activity-aware idle timeout remains unchanged.

When `External`:

- the H1 driver must omit only the idle deadline;
- connection total lifetime, if enabled, still applies;
- caller shutdown still applies;
- request/response/deferred activity accounting remains correct because other
  runtime features use it;
- do not replace the idle timeout with a total-lifetime timeout.

Parser header-read timeout is intentionally **not** externalized here. It
remains a mandatory parser defense.

## Track D — Response write-progress deadline ownership

When `EggServe`:

- current no-progress semantics remain unchanged.

When `External`:

- EggServe does not close solely because no response bytes progressed for
  `response_write_timeout`;
- transport errors, peer disconnect, total connection lifetime, and shutdown
  still terminate;
- response body producers remain backpressured;
- there is no unbounded internal buffering.

The external mode must not disable final framing/normalization.

## Track E — Global request-body ceiling ownership

`max_request_body_bytes = 0` already means "reject all bodies." Do **not**
reinterpret zero.

When ownership is `EggServe`:

- current global hard ceiling remains;
- service `RequestBodyPolicy` may only lower the limit.

When `External`:

- the global EggServe ceiling is not consulted;
- the service's explicit `RequestBodyPolicy::Buffer/Stream { max_bytes }`
  remains authoritative for that request;
- `RequestBodyPolicy::Reject` still rejects;
- framing validation, content-length consistency, decoded-byte accounting,
  trailer policy, and one-shot consumption rules remain active.

An externally owned *global* ceiling does not mean "unbounded body." It means
EggServe defers the limit decision to the service policy.

Add tests proving a service-selected body limit still fails closed when the
global ceiling is external.

## Track F — Request-target ceiling ownership

Plan 278 may have introduced target-form policy by execution time. Rebase this
track on the post-279 public target model.

When ownership is `EggServe`:

- current semantic target ceiling remains;
- absolute-form, if enabled by Plan 278, is bounded exactly as Plan 278
  specifies.

When `External`:

- EggServe skips the semantic `max_request_target_bytes` rejection only;
- mandatory Hyper parser-buffer bounds remain;
- URI/authority/Host validation remains;
- target-form permission remains;
- static confinement remains origin-only;
- no raw unbounded pre-parser is added.

Document clearly that the H1 parser buffer can still reject a sufficiently
large request line even when semantic target ownership is external.

## Track G — Validation semantics

Do not weaken `RuntimeConfig::validate()` for legacy/default users.

Preferred approach:

- keep ordinary config scalar validation unchanged;
- policy-aware execution validates only values that will actually be consumed
  by the selected path;
- irrelevant externally-owned values may retain valid defaults and simply not
  be consulted.

Do not require embedders to write invalid/zero values into public fields to
signal external ownership.

If a separate effective-policy validation function is needed, keep it in
`eggserve-server` and return typed/actionable `ServerError::Config` rather
than panicking.

## Track H — Observability

Events/counters must remain truthful.

For external ownership:

- EggServe must not increment its handler/body/idle/write timeout counter for
  a deadline it did not enforce;
- no event may say "EggServe timeout" when the external layer caused
  cancellation;
- ordinary disconnect/shutdown/transport events continue.

Consider a bounded startup/debug event or snapshot field describing ownership,
but do not emit one event per request merely to restate configuration.

## Track I — Documentation

Update:

- direct-server embedding guide;
- timeout reference;
- runtime config docs;
- public API boundary;
- relevant architecture docs/skills.

Document the security model explicitly:

> External ownership is an advanced embedding mode. It removes one EggServe
> policy authority; it does not remove parser/framing validation and does not
> provide an unbounded-data guarantee.

## Tests

Add deterministic direct-H1 tests for every ownership toggle.

Required matrix includes:

- legacy/default path unchanged;
- external handler deadline permits a handler longer than configured
  `handler_timeout`;
- external body deadline permits body progress beyond the configured duration
  while service/body policy still works;
- external idle deadline permits an otherwise-idle keep-alive connection;
- external write-progress deadline does not trigger EggServe's write timeout;
- external global body ceiling allows a body above the global configured value
  when the service policy allows it, while a smaller service limit still
  rejects;
- external target ceiling bypasses only the semantic target bound, not parser
  or authority validation;
- mixed ownership works field-by-field;
- shutdown still terminates all externally-owned modes;
- total connection timeout, when enabled, remains a hard ceiling;
- explicit connection-total disabled mode continues to compose.

Use bounded test-level timeouts so test failures cannot hang CI.

## Acceptance criteria

- [ ] Plan 279 is closed before production changes.
- [ ] existing direct/server APIs retain all-EggServe policy by default.
- [ ] ownership is explicit; no magic-large-value convention.
- [ ] handler deadline can be externally owned.
- [ ] body deadline can be externally owned across eager and deferred paths.
- [ ] keep-alive idle deadline can be externally owned.
- [ ] response write-progress deadline can be externally owned.
- [ ] global request-body ceiling can be service/external owned without
      changing `0 == reject bodies`.
- [ ] semantic request-target ceiling can be externally owned while parser
      bounds remain mandatory.
- [ ] parser `max_buf_size`, `max_headers`, framing checks, canonical
      response normalization, and shutdown remain mandatory.
- [ ] observability does not claim enforcement EggServe did not perform.
- [ ] H2/H3 behavior is unchanged unless separately and explicitly proven.
- [ ] no downstream-specific API or feature is added.
- [ ] Rust 1.89 + full repository CI pass.

## Non-goals

- No service/tunnel semaphore ownership change; Plan 281 owns admission.
- No response-body/error customization; Plan 283 owns presentation.
- No config-structure cleanup for its own sake; Plan 282 owns projection.
- No tunnel transport optimization; Plan 284 owns that evidence.
- No publication; Plans 285–286 own qualification/release.

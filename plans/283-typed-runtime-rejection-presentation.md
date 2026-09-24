# Plan 283 — Typed runtime-rejection presentation hook

## Status

**IMPLEMENTED; local presenter and integration qualification passed. Hosted CI closure remains under Plan 285.**

Plan 279 must already be closed.

## Purpose

Allow embedders to customize the *presentation* of EggServe-generated HTTP
rejections while EggServe retains status selection, protocol consequences,
framing, privacy, and transport authority.

This solves a generic integration problem for gateways, application servers,
WAFs, and reverse proxies that already own branded/error response bodies,
request IDs, or response metadata. It is not a downstream-specific adapter.

## Current authority

Runtime failures currently converge through fixed internal response helpers and
`ErrorRepresentationPolicy`. Examples include:

- request target too long;
- aggregate headers too large;
- request-body policy/limit failures;
- handler timeout/panic/internal service failure;
- internal service admission saturation;
- tunnel admission saturation;
- canonical response conversion failures.

The fixed minimal/empty representation is a strong standalone default, but an
embedder cannot provide its own bounded response body/headers without replacing
more of the H1 runtime.

## Architecture decision

Keep **status and lifecycle consequence EggServe-owned**.

Expose a typed, non-exhaustive rejection description and a synchronous,
bounded presentation hook that may choose body/application headers only.

Suggested shape:

```rust
#[non_exhaustive]
pub enum RuntimeRejectionKind {
    RequestTargetTooLong,
    RequestHeadersTooLarge,
    RequestBodyRejected,
    RequestBodyTooLarge,
    RequestBodyTimeout,
    ServiceAdmissionSaturated,
    TunnelAdmissionSaturated,
    HandlerTimeout,
    ServicePanic,
    Internal,
}

pub struct RuntimeRejection {
    kind: RuntimeRejectionKind,
    status: StatusCode,
    // bounded non-sensitive metadata only
}

pub trait RuntimeRejectionPresenter: Send + Sync + 'static {
    fn present(&self, rejection: &RuntimeRejection) -> RuntimeErrorPresentation;
}
```

Exact names may differ.

Do **not** pass raw request targets, header values, body bytes, panic payloads,
filesystem paths, or transport errors into the public rejection value.

## Track A — Fixed status authority

The hook must not be able to turn a protocol/runtime rejection into success.

Preferred presentation result:

```rust
pub struct RuntimeErrorPresentation {
    pub headers: HeaderBlock,
    pub body: ResponseBody,
}
```

EggServe then applies the already-selected status.

If implementation instead accepts a canonical `Response`, it must overwrite
or validate the status deterministically before commit.

Status mapping remains owned by existing typed errors and protocol policy.

## Track B — Lifecycle/disposition remains internal

The presenter must not control:

- keep-alive versus close after malformed/incomplete body;
- request cancellation reason;
- tunnel commitment;
- retry/reset semantics;
- connection shutdown;
- H2/H3 stream consequences.

Presentation is not protocol policy.

For example, a body framing failure that currently forces close must still
force close even if the presenter supplies a custom 400 body.

## Track C — Final response normalization/privacy

Every presented response must still pass the canonical EggServe finalization
boundary:

- hop-by-hop/framing headers remain runtime-owned;
- `Content-Length` / transfer coding remain canonical;
- response denylist/privacy remains effective;
- `Server` and `Date` remain subordinate to `ResponsePolicy`;
- HEAD/body-forbidden statuses remain bodyless as required;
- invalid presenter headers/body construction fail closed to the default
  internal representation.

The hook cannot bypass `finalize_canonical_response` /
`finalize_runtime_response`.

## Track D — Default presenter

Provide a built-in presenter that reproduces current behavior exactly from
`ErrorRepresentationPolicy`.

Existing users that configure only `ErrorRepresentationPolicy` must observe
no change.

The default presenter is the only presenter installed by ordinary
`RuntimeConfig` / `Server::builder()`.

## Track E — Configuration surface

Prefer attaching the presenter to the effective H1 policy introduced by Plan
282 or to a direct-server builder option, rather than placing a trait object in
`eggserve-primitives`.

The primitives crate may own the neutral rejection enum only if it is genuinely
shared across transports. Do not move server/runtime implementation concerns
into primitives just to avoid a server-local type.

A caller-owned direct driver must be able to install a presenter without
depending on `eggserve-core`.

## Track F — Rejection taxonomy

Inventory every runtime-generated pre-commit error path before coding.

At minimum classify:

- target 414;
- headers 431;
- request-body 400/408/413 families;
- handler 504;
- internal/panic 500;
- service admission 503;
- tunnel admission 503;
- response-construction 500/503.

Distinguish service-provided `ServiceError::rejected(status, ...)` from
runtime rejection where useful, but never expose the private service message to
the presenter unless it is already explicitly public/safe. Prefer categorical
metadata.

Parser errors produced entirely inside Hyper before the canonical service
boundary may remain outside the hook if EggServe cannot safely intercept them.
Document the exact coverage rather than claiming "all HTTP errors."

## Track G — Boundedness and failure behavior

The presenter is synchronous by design unless implementation proves an async
hook can be bounded without inventing another timeout/queue.

Requirements:

- no network or filesystem IO requirement;
- no unbounded allocation mandated by the API;
- presenter panic is contained and falls back to the built-in generic
  representation;
- invalid response construction falls back safely;
- presenter invocation occurs at most once per committed rejection;
- no recursive presenter invocation when presenter output fails.

## Track H — Observability

Counters/event kind remain tied to the underlying rejection, not the
presentation.

Do not log custom response bodies/headers.

If presenter failure occurs, record only a bounded categorical event/counter.

## Track I — Tests

Required tests:

- default presenter byte/status behavior matches current defaults;
- custom presenter changes body and safe application headers but not status;
- framing/hop-by-hop header attempts are stripped/rejected by canonical policy;
- HEAD rejection remains bodyless;
- 204/205/304 invariants remain;
- body-framing failure retains close disposition under a custom presenter;
- handler timeout remains 504;
- admission saturation remains 503;
- presenter panic falls back safely;
- invalid presenter output falls back safely;
- response privacy policy still wins;
- direct native and optional Tower service paths both reach the same
  presentation authority where applicable.

## Documentation

Update:

- downstream app-server guide;
- public API boundary;
- response/error policy docs;
- extension contract;
- architecture ownership docs.

Explicitly state that this is response *presentation*, not error/status policy.

## Acceptance criteria

- [ ] Plan 282 is implemented.
- [ ] runtime rejection categories are typed and non-sensitive.
- [ ] presenter cannot change protocol-selected status.
- [ ] presenter cannot change close/reset/lifecycle consequences.
- [ ] default behavior is byte/status compatible where currently specified.
- [ ] presenter output passes canonical framing/privacy normalization.
- [ ] presenter panic/invalid output fails closed.
- [ ] no raw Hyper type enters the public hook.
- [ ] direct crate works without core/static.
- [ ] H2/H3 behavior is unchanged unless separately proven against the same
      generic hook.
- [ ] full Rust 1.89/platform/feature CI passes.

## Non-goals

- No async middleware framework.
- No arbitrary exception/error object reflection.
- No application routing fallback.
- No parser replacement.
- No weakening of response privacy/framing.
- No publication.

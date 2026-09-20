# Plan 238 — Request-scoped shared-state allocation consolidation

## Prerequisites

- Plan 234 allocation evidence is available.
- Plan 235 has landed or closed, so its lazy body/trailer changes define the
  new baseline.

## Purpose

Evaluate and, only when justified, consolidate request-scoped heap/sync state
that is currently allocated independently for lifecycle/body ownership and
interim-response capability.

This is intentionally a higher-risk micro-optimization than Plan 235. It may
close as NO-GO.

## Current candidate

Runtime-generated requests always attach an `InterimSender`. The sender owns
an `Arc<Mutex<InterimInner>>` even though ordinary requests never send an
interim response. The request body/lifecycle already owns an
`Arc<RequestShared>`.

The opportunity is to avoid a second mandatory allocation, either by:

1. making interim state lazy within existing per-request shared storage; or
2. having the runtime-created `InterimSender` share the request's existing
   allocation while standalone public senders preserve their current
   constructor behavior.

Do not assume that merging structures is automatically better. An inline
`Mutex<Vec<...>>` or large state block can increase every request's footprint
enough to erase the allocation win.

## Design gate

Before implementation, record:

- `size_of` relevant request/lifecycle/interim structures before/after;
- allocation count for ordinary requests;
- interim-heavy allocation/lock behavior;
- whether a lazy holder such as `OnceLock` keeps the ordinary state compact;
- whether public constructor/storage semantics force awkward coupling.

Prefer lazy state over permanently embedding large interim vectors.

## Required semantics

Preserve:

- `RequestContext::interim()` behavior;
- cloneable `InterimSender` sharing one per-request logical state;
- count and aggregate-byte limits;
- duplicate-100 handling;
- HTTP/1.0 suppression;
- post-final-commit rejection;
- recorded interim ordering and snapshot behavior;
- lifecycle/body cancellation independence;
- transport-neutral `eggserve-primitives` dependency graph.

Interim and lifecycle state may share an allocation; they must not accidentally
share locks in a way that lets a slow/debug interim operation delay body
completion or cancellation.

## Implementation guidance

Acceptable designs include a compact common request allocation with separate
lazy synchronization cells. Avoid:

- one giant mutex for lifecycle + body + interim;
- generic `Any`/extension maps;
- Tokio primitives in `eggserve-primitives`;
- unsafe intrusive allocation tricks;
- hidden global pools.

The public standalone `InterimSender::new` and
`InterimSender::with_limits` constructors must keep working. If supporting
both standalone and runtime-shared modes requires complex enum plumbing or
larger common objects with no measured win, close NO-GO.

## Tests

- ordinary request with no interim;
- one/multiple informational responses;
- duplicate 100;
- limits and oversize;
- final commitment race;
- HTTP/1.0 suppression;
- clone sharing;
- lifecycle cancellation/body completion concurrent with interim operations;
- H1/H2/H3 existing interim suites.

## Measurement

Compare to the post-Plan-235 baseline:

- allocations/request for bodyless H1 request;
- object sizes;
- custom 1 KiB c1/c16/c64;
- an interim-heavy synthetic workload;
- RSS under many idle/live requests.

## Keep/revert rule

KEEP only if the ordinary request removes a real allocation or measurable
resource cost without increasing common object size/locking/complexity
materially. REVERT/NO-GO if the design couples unrelated state, broadens the
supported API solely for optimization, or merely moves the allocation.

## Acceptance criteria

- [ ] A before/after allocation and object-size record exists.
- [ ] Ordinary no-interim requests pay less fixed allocation cost if the track
      lands.
- [ ] Interim semantics and transport neutrality remain unchanged.
- [ ] Lifecycle/body operations do not share an unnecessarily broad mutex with
      interim state.
- [ ] Public constructors/accessors remain source-compatible.
- [ ] A NO-GO outcome is explicitly acceptable and documented.

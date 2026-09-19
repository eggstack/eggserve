# Plans 227–231 — Performance hot-path optimization program

## Objective

Run an evidence-led optimization pass over the current post-Plan-226 EggServe
architecture without weakening the security, framing, lifecycle, or crate-
ownership invariants established by Plans 164, 170, and 211–226.

The planning review at `adecd0b` found several plausible costs in the
current H1/static hot paths:

- response-body frame tracking performs producer-progress bookkeeping that is
  not used by the H1 write-stall decision;
- file responses still allocate and fill one owned buffer per 8 KiB chunk;
- the H1 service wrapper clones many independent shared handles and boxes an
  internal dispatch closure/future per request;
- runtime response conversion creates a default `Date` only for the final
  response-policy boundary to replace it;
- request/static metadata paths contain repeated small allocations and repeated
  linear header scans;
- structured debug events are fully materialized even for no-op or filtered
  sinks;
- the newest comprehensive performance evidence is Plan 170 and predates the
  direct-crate/compatibility-facade restructuring.

These are hypotheses until current-HEAD measurements confirm them. The program
therefore starts with profiling and ends with explicit keep/revert decisions.

## Sequence

### Mandatory evidence gate

**Plan 227 — Current-HEAD performance baseline and hot-path profiling**

Run first. Reproduce the useful Plan 170 workload families against current
`main`, add a native client/body microbenchmark path that is not capped by the
CPython loopback driver, and record CPU/allocation/profile evidence. No runtime
optimization lands under Plan 227.

### Parallel implementation tracks after Plan 227

**Plan 228 — H1 activity tracking and dispatch-state optimization**

Remove or simplify producer-frame bookkeeping only after proving it is not part
of the H1 write-stall semantics, and collapse per-request shared-state cloning
behind one connection-level state object. Preserve public `Service` behavior.

**Plan 229 — Static/file response streaming optimization**

Use the Plan 227 evidence to select a better file-stream chunk regime, avoid
avoidable buffer initialization, and remove small static-serving metadata
allocations/scans. Preserve resolver-opened file capabilities and bounded
stream admission.

**Plan 230 — Response finalization, request metadata, and observability hot-path cleanup**

Collapse duplicate `Date` work, add a lazy disabled-event path, and remove
small request/metadata temporary allocations where profiling justifies it.
Keep canonical types transport-neutral and observability semantics intact.

Plans 228–230 are intentionally separate so any optimization can be reverted
without coupling unrelated behavior. They may execute in parallel after Plan
227 if they do not edit the same internal modules at the same time.

### Closure

**Plan 231 — Post-optimization performance and regression qualification**

Run only after the candidate changes from 228–230 are available. Perform
same-machine A/B qualification, retain only reproducible improvements or
clear code-simplification wins, run the full correctness/security/package
matrix, and publish new evidence without marketing-style universal claims.

## Global invariants

- No change may weaken path/filesystem confinement or reopen pathname
  check-then-open behavior.
- `eggserve-static` remains the sole static/path/filesystem authority.
- `eggserve-server` remains the H1 runtime/service/transport authority.
- `eggserve-primitives` remains free of Hyper/Tokio/runtime dependencies.
- Runtime framing remains single-authority; applications do not control
  `Content-Length`, transfer coding, or connection framing.
- Write-stall protection remains based on real forward transport progress.
- Existing connection/service/file-stream/tunnel admission limits remain
  bounded and recover permits on every terminal path.
- No new broad dependency is added for optimization or benchmarking.
- H2/H3 support tiers do not change.
- No `sendfile`/`splice`/io_uring/custom-allocator/buffer-pool fast path is
  authorized by this program; those require separate evidence and design.
- Absolute RPS/latency values remain non-gating and machine/profile specific.

## Completion definition

The program is complete when:

1. a post-226 current-HEAD baseline exists with source SHA, environment,
   profiles, resource data, and profiler evidence;
2. each candidate optimization has a before/after result tied to the workload
   it is intended to improve;
3. no retained change regresses correctness, bounded-resource behavior,
   cancellation, timeout, or confinement semantics;
4. no retained optimization introduces a new dependency or public API burden
   without explicit justification;
5. the final evidence records which hypotheses were confirmed, rejected, or
   deferred;
6. routine and feature-gated CI, package, supply-chain, Python-wheel, and
   topology checks pass on the closing commit.

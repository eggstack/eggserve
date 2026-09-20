# Plans 234–240 — Fixed-cost performance optimization program

## Objective

Run a second, narrower performance campaign over the post-Plan-233 EggServe
baseline. Plans 227–233 already addressed the large, evidence-backed costs in
file streaming, H1 activity tracking, response metadata, and benchmark
provenance. The remaining opportunities identified by the planning review at
`07b8a843b86076b9fd6c79c76d195842544a9f5e` are predominantly fixed per-request
costs: small allocations, reference-count traffic, one avoidable Unix
descriptor duplication, and frontend-specific object/thread overhead.

This program is deliberately conservative. It does not authorize a new public
service model, a new runtime, a cache, a zero-copy transport fast path, or
security-policy relaxation. The goal is to reduce work that the current
contracts do not require while preserving all public Rust/Python behavior.

## Sequence

### Mandatory evidence gate

**Plan 234 — Current-HEAD fixed-cost baseline and profiling**

Run first. Capture allocation/syscall/resource evidence for the current small
response path, static path resolution, request primitive construction,
established TLS metadata, and Python callback/streaming paths. Reuse the
Plan-227/231 native harnesses rather than creating another benchmark framework.
No production optimization lands under Plan 234.

### Native implementation tracks after Plan 234

**Plan 235 — Request-target and request-body common-path allocation optimization**

Reduce mechanically unnecessary allocations in canonical request-target and
body construction while keeping all public methods and semantics unchanged.
Primary candidates are single-buffer request-target storage, direct construction
of already-complete lifecycle state, lazy wire-trailer state for bodies that
cannot receive wire trailers, and small iterator-only header lookup cleanup.

**Plan 236 — Static resolver and path fixed-cost optimization**

Remove the per-request Unix root-descriptor duplicate from non-root hardened
resolution and reduce temporary path-processing allocations. Preserve
descriptor-relative confinement, pre-open special-file/symlink checks,
post-open identity/metadata validation, and every public static/path API.

**Plan 237 — H1 dispatch and connection-metadata optimization**

Finish the internal Plan-228 dispatch simplification where current Rust/Hyper
bounds permit: replace the remaining `Arc<dyn Fn>` service indirection with a
named generic Hyper service, add a zero-tunnel fast path if measured, and
evaluate immutable per-connection metadata sharing. No public `Service`
redesign is allowed.

**Plan 238 — Request-scoped shared-state allocation consolidation**

After Plan 235 establishes the new primitive baseline, evaluate whether
lifecycle/interim/request-scoped synchronization can share an allocation or be
lazily materialized. This track must be driven by allocation evidence and may
close as NO-GO if the size/complexity tradeoff is worse than the saved
allocation.

### Frontend-specific track

**Plan 239 — Python bridge allocation and stream-resource optimization**

Profile and reduce eager duplicate Python request representations while
preserving the existing Python property/API behavior. Separately qualify the
one-native-thread-per-stream response bridge under slow-client concurrency.
A stream-execution redesign is authorized only if it preserves GIL isolation,
backpressure, cancellation, and shutdown semantics without introducing
unbounded queues or blocking Tokio workers.

### Closure

**Plan 240 — Fixed-cost optimization qualification and closure**

Freeze the candidate, compare it against the Plan-234 baseline on the same
machine/profile, run correctness/security/package/Python qualification, and
retain only reproducible wins or mechanically simpler equivalent behavior.

## Global invariants

- No public Rust or Python API removal, rename, signature change, semantic
  narrowing, or support-tier change.
- Additive public runtime-adapter plumbing is strongly disfavored. If an
  optimization requires widening the supported API solely for internal
  sharing, defer it unless there is no cleaner crate-boundary design and the
  benefit is compelling.
- `eggserve-primitives` remains transport/runtime neutral.
- `eggserve-server` remains the generic H1 runtime/service authority.
- `eggserve-static` remains the sole path/filesystem/static authority.
- Hardened Unix resolution remains descriptor-relative with no pathname
  check-then-open fallback.
- Pre-open symlink/special-file rejection and post-open object validation are
  not removed merely to save syscalls.
- Runtime-owned framing, timeout, cancellation, admission, and observability
  semantics remain unchanged unless a plan explicitly proves an internal
  representation can change without observable behavior.
- H2/H3 behavior and support tiers are not optimization targets in this
  campaign; shared primitive changes must still pass their existing suites.
- No new production dependency is added.
- No descriptor/path cache, mmap cache, sendfile/splice/io_uring path, custom
  allocator, global buffer pool, or executor replacement is authorized.
- No absolute RPS/latency threshold becomes a CI gate.

## Completion definition

The program is complete when:

1. Plan 234 records a current baseline with allocation/syscall/resource
   evidence and a source SHA;
2. each implementation track has a GO/NO-GO decision tied to that evidence;
3. retained changes preserve the public Rust/Python surface and security
   invariants;
4. the final A/B evidence includes small native/static responses, established
   TLS, path-heavy static requests, and representative Python callback/stream
   workloads;
5. correctness, confinement, timeout, lifecycle, package, supply-chain, and
   Python-wheel verification pass; and
6. the repository records KEEP/REVERT/DEFER decisions rather than treating
   every plausible micro-optimization as mandatory.

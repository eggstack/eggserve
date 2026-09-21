# Plans 242–248 — API-preserving maintainability and authority-convergence program

## Objective

Follow the completed Plans 217–241 with a narrowly scoped maintenance campaign
that closes the remaining implementation duplication and library-surface
defects discovered in the current-repository review at
`673b6c60dab09d728b05d9e979be91bfc5417050`.

This program is not a feature campaign. It must preserve the existing Rust and
Python public API surface, capability set, defaults, security invariants, and
protocol support tiers. Its purpose is to make the documented ownership model
truer in code, fix one direct-runtime lifecycle correctness defect, reduce
maintenance cost, and improve the direct-crate/Python developer experience.

The review found five actionable classes of work:

1. the direct `eggserve-server::Server` uses an edge-trigger-like
   `Notify::notify_waiters()` shutdown signal and detached connection tasks,
   allowing shutdown signals to be missed and making `wait()` weaker than
   the compatibility runtime;
2. `eggserve-core` still contains a large H1 connection/runtime pipeline that
   substantially overlaps `eggserve-server`, despite the direct crate being
   the documented H1 authority;
3. `eggserve-core::server::StaticService` and
   `eggserve-static::StaticService` remain separate service implementations,
   with the direct leaf missing some existing compatibility-service behavior;
4. Python runtime behavior is strong, but `eggserve.lowlevel` lacks a
   dedicated stub surface and the package does not yet claim typed-package
   status; native binding code also remains unnecessarily concentrated;
5. an orphaned, uncompiled
   `eggserve-primitives/src/primitives/runtime_limits.rs` remains from an
   earlier authority move, and some direct-crate feature declarations/leaf
   qualification evidence do not accurately communicate the actual direct
   capability boundary.

## Sequence

```text
243  direct Server shutdown/lifecycle correctness
 |
244  H1 runtime authority convergence
 |\
 | 245 static-service authority convergence
 | 246 Python interop typing/internal maintainability
 | 247 leaf-crate surface, orphan-source, and qualification cleanup
 |/
248  API/capability-preserving qualification and closure
```

Plan 243 is immediate and blocks Plan 244 because the direct runtime must have
correct lifecycle semantics before compatibility orchestration is moved onto
it. Plans 245–247 may proceed in parallel after conflicts are checked; none
may weaken the public compatibility surface. Plan 248 is the final gate.

## Plan summaries

### Plan 243 — Direct-server shutdown and lifecycle correctness

Replace the lossy shutdown signaling in the direct `eggserve-server::Server`
path with durable lifecycle state/cancellation and track accepted connection
tasks through drain. Preserve all existing public signatures and observable
success-path behavior.

### Plan 244 — H1 runtime authority convergence

Make `eggserve-server` the actual implementation authority for shared H1
connection execution and lifecycle machinery. Convert matching
`eggserve-core::server::connection` code to compatibility adapters around the
direct implementation while retaining H2/TLS/proxy/listener-specific
composition in core where it is genuinely protocol/front-end glue.

### Plan 245 — Static-service authority convergence

Move reusable extended static-serving behavior into `eggserve-static` so
there is one service implementation authority. Keep
`eggserve_core::server::StaticService` and its builder as source-compatible
wrappers/facades, including current extra-header, error-policy, listing-budget,
ops, and `ServeConfig` behavior.

### Plan 246 — Python interop typing and internal maintainability

Complete the typing story for `eggserve.lowlevel`, validate the installed
wheel with static type checkers, add `py.typed` only after coverage is
credible, and split oversized private binding modules without moving or
renaming any public Python object.

### Plan 247 — Leaf-crate surface, orphan-source, and qualification cleanup

Delete the orphaned uncompiled runtime-limit source, add an orphan-source
structural check, clarify/normalize inert feature declarations without
removing feature names, and migrate direct-authority qualification tests into
the owning leaf crates while retaining cross-layer parity tests in core.

### Plan 248 — API-preserving qualification and closure

Freeze the candidate, prove source/API/capability preservation, run the full
Rust/Python/security/platform matrix, verify crate/package dependency graphs,
and record exact-SHA remote CI evidence. No architecture work is complete
until this gate passes.

## Global invariants

- No existing public Rust item may be removed, renamed, moved without a
  compatibility re-export, have its signature narrowed, or have documented
  successful behavior removed.
- No existing public Python import path, class, method, function, constructor
  parameter, or documented behavior may be removed or renamed.
- `eggserve-core` remains available as the compatibility/composition umbrella.
- `eggserve-primitives` remains transport/runtime neutral.
- `eggserve-server` remains the generic server/service authority and must not
  acquire static-filesystem policy.
- `eggserve-static` remains the sole path/filesystem confinement authority.
- `eggnet-tls` remains the neutral TLS identity/trust authority.
- `eggserve-h3` remains the H3/QUIC adapter authority.
- No eggfetch/eggress product dependency may be added.
- H2 and H3 support tiers do not change.
- Framing, request-body, timeout, cancellation, admission, tunnel, proxy,
  confinement, and response-privacy semantics remain bounded and fail closed.
- No new broad production dependency is authorized merely to simplify code.
- No benchmark-only API redesign or performance threshold is introduced.
- Existing deprecated/compatibility feature names remain accepted unless a
  separate breaking-release plan explicitly authorizes removal.

## Compatibility method

Before implementation, each plan must inventory the public paths it touches.
When implementation moves ownership:

1. implement/delegate behind the existing public path;
2. retain the existing type identity where callers rely on it;
3. prefer `pub use` or a transparent adapter over duplicated behavior;
4. add compile-time identity/source-compatibility fixtures where practical;
5. compare wire-visible behavior before deleting the old implementation.

This campaign is allowed to add private helpers, tests, documentation,
additional type stubs, and additive internal feature plumbing. It is not
allowed to exploit the pre-1.0 version to make unrelated breaking changes.

## Completion definition

The program is complete only when:

- the direct server cannot lose an immediate/concurrent shutdown request and
  `wait()` accounts for accepted connection tasks;
- one shared H1 implementation authority remains for behavior common to direct
  and compatibility runtimes;
- one static-service implementation authority remains for behavior common to
  direct and compatibility static serving;
- Python low-level public typing is complete enough to ship typed-package
  metadata, or Plan 246 explicitly closes NO-GO with recorded checker gaps;
- the orphan runtime-limit implementation is gone and structural CI rejects
  future unreferenced Rust implementation files;
- direct crates carry their own authority tests while core retains
  compatibility/cross-protocol parity tests;
- all old Rust/Python public paths compile/import and retain behavior;
- the full routine, feature, package, supply-chain, wheel, platform, and remote
  CI evidence is green for the exact closure SHA.

## Non-goals

This program does not authorize ASGI/WSGI product work, routing, middleware,
reverse proxying, new protocol families, sendfile/io_uring work, a public
`Service` redesign, async-trait adoption, executor replacement, Python raw
socket exposure, or capability-filesystem extraction.

Any newly discovered feature request belongs in a separate plan.

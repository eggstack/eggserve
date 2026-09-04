# Public API boundary

This document defines the public API surface of `eggserve-core` and the rules for evolving it.

## Overview

`eggserve-core` exposes a deliberate, narrow public boundary through the `primitives` module. This module is the **intended integration point** for Rust consumers that want to embed eggserve's hardened path validation and policy enforcement without pulling in the full HTTP service layer. Canonical application-facing request/response types, `Service`, and the caller-owned connection API do not require downstream code to import Hyper.

## Public modules

| Module | Visibility | Stability | Purpose |
|--------|------------|-----------|---------|
| `primitives` | `pub` | Stable (semver-considered) | Core types for embedding: path validation, policy enforcement, rejection taxonomy |
| `config` | `pub` | Stable-ish | `ServeConfig`, `ServeState`, `StartupSummary` |
| `limits` | `pub` | Stable-ish | `Limits` (connections, streams, timeouts) |
| `policy` | `pub` | Stable-ish | `StaticPolicy`, `DirectoryListingPolicy`, `SymlinkPolicy`, `DotfilePolicy`, `StaticMetadataPolicy`, `ErrorRepresentationPolicy` |
| `server::service` | `pub` | Experimental | Explicit-context `handle_request` adapter; use `server::Server` for new integrations |

## Internal modules (not public API)

`fs`, `path`, `response`, MIME detection, and the error taxonomy are `pub(crate)`. External callers must not depend on them. Types from these modules are re-exported through `primitives` where appropriate.

## Primitives module

`primitives::ResolvedResource::IoError` reports an operating-system
resolution failure and must not be treated as `NotFound`. The feature-gated
`ResolvedFile` extraction methods (`into_std_file`, `into_parts`, and
`from_parts`) are internal Python-binding bridges, not general Rust APIs; once
a handle is extracted, the confinement guarantee no longer applies.

The `primitives` module re-exports the following types:

### Path validation

- **`ConfinedPath`** — Parsed, validated HTTP request target. Only representable after passing through the full validation pipeline (origin-form parsing, percent decoding, path normalization, component validation). Methods: `parse()`, `as_str()`, `components()`.

- **`PathPolicy`** — Configuration for path validation. Controls dotfile acceptance and backslash rejection during `ConfinedPath` parsing.

- **`PathRejection`** — Single error type for all path validation failures. Every variant maps to a specific security check.

### Policy types

- **`StaticPolicy`** — Composite security policy for static file serving. Defaults to most restrictive settings via `Default::default()` and `safe_default()`.

- **`DirectoryListingPolicy`** — `Disabled` (default) / `Enabled`. Controls directory listing generation.

- **`SymlinkPolicy`** — `Denied` (default) / `Follow`. Controls symlink following during resolution.

- **`DotfilePolicy`** (from `policy`) — `Denied` (default) / `Serve`. Controls whether dotfiles are served in responses.

- **`PathDotfilePolicy`** (from `path`) — `Denied` (default) / `Allow`. Controls whether dotfile paths are accepted during parsing. Distinct from the policy-level `DotfilePolicy`.

- **`StaticMetadataPolicy`** — `standard()` (emit `ETag` + `Last-Modified`) / `minimal_fingerprint()` (suppress both). Owned by `StaticPolicy.static_metadata`; planner `plan_file_response_with_preconditions_and_metadata`.

- **`ErrorRepresentationPolicy`** — `Minimal` (fixed generic bodies) / `Empty` (no bytes for runtime errors; application `Ok` never rewritten). Owned by `ServeConfig.error_policy` and `ResponsePolicy.error_policy`.

`server::response_policy::{ResponsePolicy, DatePolicy}` is experimental (Rust-only
advanced privacy; CLI/Python keep standards defaults). `ResponsePolicy` is the
sole `Date`/`Server`/denylist authority with Hyper automatic `Date` disabled.

`ResponseStream` is a stable, Hyper-free one-shot producer boundary. Its
constructors require `Stream<Item = Result<Bytes, ResponseStreamError>> + Send +
'static`; `Sync` is intentionally not required because the connection task
owns and polls the producer exclusively. `Response` and the internal transport
body remain `Send` for spawned connection tasks, while no cross-task concurrent
polling is supported.

The two intentional public Hyper conversion adapters are:

- `RequestHead::try_from_hyper()` — fallible inbound conversion from a Hyper
  request into canonical request metadata;
- `primitives::to_hyper_response()` — outbound conversion of a canonical
  response at the low-level transport boundary.

The outbound adapter returns a Hyper response with an opaque body type. Its
stable contract is the `http_body::Body<Data = bytes::Bytes,
Error = std::io::Error>` behavior, not `BoxBody` or another concrete erasure
type. The runtime's semaphore-aware conversion helper is internal. Hyper is
otherwise an implementation dependency of the runtime, not a requirement for
canonical consumers or `Service` implementations.

The downstream application-server contract is qualified externally by
`crates/eggserve-core/tests/app_server_consumer.rs` (Plan 175), which uses
only `primitives` + `server` plus ordinary downstream dependencies. The
builder-facing rules live in [downstream-app-server.md](downstream-app-server.md).
Plan 176 closed as deferred: no `UpgradeRequest`, `UpgradeResponse` /
`ServiceOutcome`, or `UpgradedIo` types exist — `Request` carries
head/body/connection/lifecycle only and `Service` returns `Response` only.

## Invariants

Every type in the public API enforces safety invariants at construction time:

1. **No unchecked path exists.** `ConfinedPath` is only representable after passing through the full validation pipeline.

2. **Safe defaults are enforced.** `StaticPolicy::default()` denies all optional behaviors. Callers must explicitly opt in.

3. **Single error type.** `PathRejection` is the only error type for path validation. No stringly-typed errors.

4. **No information leakage.** Rejected paths never reveal filesystem content or structure.

5. **Policy separation.** The path policy (`PathPolicy`) controls request-target acceptance during parsing. The static policy (`StaticPolicy`) controls whether a resolved resource may be served. A custom path policy that permits dotfile paths does not override a static policy that denies dotfile serving.

## Versioning policy

Before 1.0:
- **Patch releases preserve stable source compatibility.**
- **Intentional breaking changes to stable Rust APIs require an explicit minor
  transition**, such as `0.1.x` → `0.2.0`, with release notes and migration
  guidance.
- **Experimental APIs** may change in any non-patch release under their
  separately documented policy.

After 1.0:
- Follows semver strictly.
- New re-exports in `primitives` are non-breaking additions.
- Removal or renaming of types requires a major version bump.

## Migration guide

Historical type removals/renames and their replacements (for example
`ReadOnlyMethod` → `Method`, `response_write_timeout` → `connection_total_timeout`)
are catalogued in [the migration guide](migration-guide.md).

When a type is removed or renamed:
1. Check `docs/release-process.md` for the release timeline.
2. Use the new type name or re-export path.
3. Run `cargo clippy` — deprecated items emit warnings.

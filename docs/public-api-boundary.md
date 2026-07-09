# Public API boundary

This document defines the public API surface of `eggserve-core` and the rules for evolving it.

## Overview

`eggserve-core` exposes a deliberate, narrow public boundary through the `primitives` module. This module is the **intended integration point** for Rust consumers that want to embed eggserve's hardened path validation and policy enforcement without pulling in the full HTTP service layer.

## Public modules

| Module | Visibility | Stability | Purpose |
|--------|------------|-----------|---------|
| `primitives` | `pub` | Stable (semver-considered) | Core types for embedding: path validation, policy enforcement, rejection taxonomy |
| `config` | `pub` | Stable-ish | `ServeConfig`, `ServeState`, `StartupSummary` |
| `limits` | `pub` | Stable-ish | `Limits` (connections, streams, timeouts) |
| `policy` | `pub` | Stable-ish | `StaticPolicy`, `DirectoryListingPolicy`, `SymlinkPolicy`, `DotfilePolicy` |
| `service` | `pub` | Experimental | `handle_request` HTTP handler (body type not stable) |

## Internal modules (not public API)

`fs`, `path`, `response`, MIME detection, and the error taxonomy are `pub(crate)`. External callers must not depend on them. Types from these modules are re-exported through `primitives` where appropriate.

## Primitives module

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

## Invariants

Every type in the public API enforces safety invariants at construction time:

1. **No unchecked path exists.** `ConfinedPath` is only representable after passing through the full validation pipeline.

2. **Safe defaults are enforced.** `StaticPolicy::default()` denies all optional behaviors. Callers must explicitly opt in.

3. **Single error type.** `PathRejection` is the only error type for path validation. No stringly-typed errors.

4. **No information leakage.** Rejected paths never reveal filesystem content or structure.

5. **Policy separation.** The path policy (`PathPolicy`) controls request-target acceptance during parsing. The static policy (`StaticPolicy`) controls whether a resolved resource may be served. A custom path policy that permits dotfile paths does not override a static policy that denies dotfile serving.

## Versioning policy

Before 1.0:
- **Minor versions** may add new types and variants.
- **Patch versions** may fix bugs without API changes.
- **Breaking changes** will be accompanied by a major version bump and migration notes.

After 1.0:
- Follows semver strictly.
- New re-exports in `primitives` are non-breaking additions.
- Removal or renaming of types requires a major version bump.

## Migration guide

When a type is removed or renamed:
1. Check `docs/release-criteria.md` for the deprecation timeline.
2. Use the new type name or re-export path.
3. Run `cargo clippy` — deprecated items emit warnings.

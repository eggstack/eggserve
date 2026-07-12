# SecureRoot

## Overview

`SecureRoot` is a public API for resolving request-derived paths under eggserve's audited confinement. It wraps the internal `RootGuard` and policy enforcement behind a stable facade, intended for Rust embedders who want path resolution without the full HTTP service.

All types are exported from `eggserve_core::primitives`.

## Core types

### `SecureRoot`

Constructed from a root directory path and a `StaticPolicy`. The root is canonicalized at construction time.

Creates a fresh `RootGuard` per resolution call — matching the current request-handling behavior in the HTTP service layer.

**Methods:**

| Method | Signature | Description |
|--------|-----------|-------------|
| `new` | `(impl AsRef<Path>, StaticPolicy) -> Result<Self, io::Error>` | Canonicalizes root, verifies it is a directory |
| `policy` | `(&self) -> &StaticPolicy` | Returns the configured policy |
| `root_path` | `(&self) -> &Path` | Returns the canonicalized root path |
| `resolve` | `(&self, &ConfinedPath) -> ResolvedResource` | Resolves a pre-parsed confined path |
| `resolve_uri` | `(&self, &str) -> Result<ResolvedResource, PathRejection>` | Parses and resolves a raw URI string in one call |

`resolve_uri` constructs a `PathPolicy` from the stored `StaticPolicy` (mapping dotfile policy and backslash rejection), parses the URI into a `ConfinedPath`, and delegates to `resolve`.

### `ResolvedResource`

Enum representing the outcome of a path resolution.

```rust
pub enum ResolvedResource {
    File(ResolvedFile),
    Directory(ResolvedDirectory),
    NotFound,
    Denied(ResourceDeniedReason),
}
```

**Accessor methods:** `is_file()`, `is_directory()`, `is_not_found()`, `is_denied()`, `as_file()`, `as_directory()`, `into_file()`, `into_directory()`.

### `ResolvedFile`

Capability object wrapping an already-opened file handle. `ResolvedFile` is a resolver-created capability — there is no public constructor; it can only be obtained through `SecureRoot` resolution. The file was opened during resolution via `openat(O_NOFOLLOW)` on Unix safe defaults — the service layer never reopens it by absolute path.

| Method | Returns | Description |
|--------|---------|-------------|
| `len()` | `u64` | File size in bytes |
| `is_empty()` | `bool` | `len() == 0` |
| `modified()` | `Option<SystemTime>` | Last modification time |
| `content_type()` | `&'static str` | MIME type derived from `safe_relative_components()` |
| `safe_relative_components()` | `&[String]` | Path components relative to root (for MIME detection only) |
| `into_body(plan)` | `Result<BodySource, BodySourceError>` | Consumes self, converts to a body source for the given `StaticResponsePlan` |
| `into_range_body(start, end_inclusive)` | `Result<BodySource, BodySourceError>` | Consumes self, converts to a range body source |

**Feature-gated extraction methods** (behind `python-bindings-internal`):

| Method | Returns | Description |
|--------|---------|-------------|
| `into_std_file()` | `std::fs::File` | Consumes self, returns the underlying file handle. **Breaks confinement guarantee** — see [Capability boundary](#capability-boundary) below. |
| `into_parts()` | `(std::fs::File, std::fs::Metadata)` | Returns file handle and metadata. **Breaks confinement guarantee** — see [Capability boundary](#capability-boundary) below. |
| `from_parts(file, metadata, components)` | `ResolvedFile` | Reconstructs from raw components without provenance verification. Internal-only; external consumers must not use this. |

### `ResolvedDirectory`

Wraps a resolved directory with an open directory descriptor on Unix. Child resolution and listing use the same policy as the parent. `list()` returns a policy-filtered filesystem listing; HTTP directory-listing exposure is separately controlled by `StaticPolicy.directory_listing`.

| Method | Returns | Description |
|--------|---------|-------------|
| `components()` | `&[String]` | Path components relative to root |
| `resolve_child(&self, &str, &SecureRoot)` | `ResolvedResource` | Resolves a child entry within this directory |
| `list(&self, &SecureRoot)` | `Result<Vec<(String, bool)>, io::Error>` | Lists directory entries as `(name, is_dir)` |

Both `resolve_child` and `list` create a fresh `RootGuard` from the provided `SecureRoot` (no descriptor caching across calls).

### `ResourceDeniedReason`

Structured denial enum returned when resolution fails due to a policy or security check:

```rust
pub enum ResourceDeniedReason {
    SymlinkDenied,
    DotfileDenied,
    RootEscapeDenied,
    PolicyDenied(PathRejection),
}
```

Callers can match on specific denial reasons to produce appropriate responses (e.g., 403 Forbidden). Implements `Display`, `Error`, and `From<PathRejection>`.

## Security guarantees by platform

### Unix + symlink denied (safe defaults)

The strongest guarantee. Resolution uses descriptor-relative traversal:

1. Each path component is checked with `statat(AT_SYMLINK_NOFOLLOW)`.
2. The component is opened with `openat(O_NOFOLLOW)`.
3. If a symlink is swapped into place between stat and open, the open fails rather than following it.
4. Files are opened during resolution — the service layer never reopens by absolute path.

Both intermediate and final symlinks are rejected. This is the only path covered by the descriptor-relative TOCTOU-hardening guarantee.

### Unix + follow symlinks

Canonicalize-based fallback with root escape check:

1. Components are checked with `symlink_metadata`.
2. The final canonical path is verified against the canonical root.
3. Symlinks whose canonical target escapes the root are denied.

**This mode is weaker.** A symlink could be swapped after canonicalization and before the file is opened. Root escape is still checked against the canonical root, but the TOCTOU window is wider. Follow-symlinks is explicitly outside the descriptor-relative hardening guarantee.

### Non-Unix

Parser and canonicalization checks only. Symlink checks use `symlink_metadata` (component-wise, not descriptor-relative). Not equivalent to Unix descriptor-relative hardening.

### Windows

Functional but not hardened against all reparse-point or junction attacks. Not recommended for untrusted mutable public roots until a Windows-specific hardening plan.

## Usage example

```rust
use eggserve_core::primitives::{SecureRoot, StaticPolicy};
use eggserve_core::primitives::{ConfinedPath, PathPolicy};

let root = SecureRoot::new("/srv/public", StaticPolicy::safe_default())?;

// Resolve a pre-validated path
let confined = ConfinedPath::parse("/assets/app.css", &PathPolicy::default())?;
let resource = root.resolve(&confined);

// Or resolve a raw URI string
let resource = root.resolve_uri("/assets/app.css")?;

match resource {
    eggserve_core::primitives::ResolvedResource::File(file) => {
        let content_type = file.content_type();
        let size = file.len();
        // Convert to a BodySource for streaming (consumes the file handle)
        let plan = plan_file_response(method, metadata, content_type, ...);
        let body = file.into_body(&plan)?;
    }
    eggserve_core::primitives::ResolvedResource::Directory(dir) => {
        let entries = dir.list(&root)?;
    }
    eggserve_core::primitives::ResolvedResource::NotFound => { /* 404 */ }
    eggserve_core::primitives::ResolvedResource::Denied(reason) => { /* 403 */ }
}
```

## Capability boundary

The extraction methods (`into_std_file`, `into_parts`, `from_parts`) are behind the `python-bindings-internal` feature gate and exist solely for cross-crate Python bindings where the file was already resolved through a secure path.

**When you extract a raw `std::fs::File`, the confinement guarantee ends.** The file handle is no longer tracked by the resolver. A caller that extracts a file handle can:

- Read from the file at arbitrary offsets (no range enforcement)
- Retain the handle indefinitely (no automatic cleanup on drop of `ResolvedFile`)
- Pass it to code that does not know it came from a confined resolution

External Rust consumers should use `into_body(plan)` or `into_range_body(start, end_inclusive)` instead. These methods consume the file handle into a `BodySource` that carries the handle forward for streaming without exposing it to arbitrary use.

The `from_parts` constructor reconstructs a `ResolvedFile` without re-verifying that the file was opened through the confinement pipeline. It is intentionally limited to internal bindings where provenance is established by construction (the Python layer resolves through `SecureRoot` before calling `from_parts`).

## Why not reopen paths

The file handle from `ResolvedFile` was opened under policy enforcement (descriptor-relative traversal on Unix safe defaults). Reopening by absolute path would bypass the descriptor-relative guarantee:

- The absolute path could resolve differently if the filesystem was modified between resolution and reopening.
- A symlink could be swapped in during the gap.
- The service layer would lose the property that every open was governed by `O_NOFOLLOW`.

`safe_relative_components()` provides extension information for MIME detection without exposing the absolute path. Callers should use the returned `std::fs::File` handle directly, not re-derive a path for opening.

## Limitations

- **No descriptor caching.** `RootGuard` is created per resolution call. The root directory is opened fresh on every `resolve` / `resolve_child` / `list` call. This is correct but has overhead; caching the root descriptor across requests is a future optimization.
- **Directory child resolution creates a fresh `RootGuard`.** `resolve_child` does not reuse the parent directory's descriptor. The new guard reopens the root and resolves from there.
- **Response planning and body conversion available.** Callers can use `plan_file_response()` to generate `StaticResponsePlan` from a `ResolvedFile` + method + request headers, covering conditional requests (If-None-Match, If-Modified-Since), range requests, and ETag generation. Use `into_body(&plan)` or `into_range_body(start, end_inclusive)` to convert a resolved file into a `BodySource` that carries the opened file handle forward without path reopening. See the planner and body modules for details.
- **Python bindings available.** `SecureRoot`, `ResolvedResource`, `ResolvedFile`, `ResolvedDirectory`, and `StaticPolicy` are exposed via PyO3 in the `eggserve` package. See [python-api.md](python-api.md) for the full API reference.

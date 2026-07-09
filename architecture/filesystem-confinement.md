# Filesystem Confinement — Deep Dive

After path validation, filesystem confinement resolves the validated path against the configured root directory. This layer prevents path traversal and symlink escape, even under concurrent modification.

## Module Map

| Module | File | Purpose |
|--------|------|---------|
| `mod.rs` | `fs/mod.rs` | `RootGuard`, `ResolvedResource`, `ResolvedFile`, `ResolvedDirectory` |
| `unix.rs` | `fs/unix.rs` | Descriptor-relative traversal (statat + openat) |

## Core Types

### `RootGuard`

Per-request guard that canonicalizes the configured root and opens it as a directory descriptor. On Unix, this holds an `OwnedFd` for the root directory.

```rust
pub(crate) struct RootGuard {
    root_fd: OwnedFd,       // Unix: open directory descriptor
    root_path: PathBuf,     // canonicalized root path
}
```

Created once per request. Ensures the root is a valid directory before any traversal begins.

### `ResolvedResource`

The result of filesystem resolution:

```rust
pub enum ResolvedResource {
    File(ResolvedFile),
    Directory(ResolvedDirectory),
    NotFound,
    Denied(PathRejection),
}
```

Each variant carries enough information for the response layer to proceed. `Denied` carries the specific rejection reason for error responses.

### `ResolvedFile`

A pre-opened file handle. No re-opening by absolute path.

```rust
pub struct ResolvedFile {
    pub file: std::fs::File,        // pre-opened handle
    pub metadata: std::fs::Metadata, // length, modified time
    pub content_type: String,        // MIME type from extension
    pub etag: String,               // ETag for conditional requests
}
```

The key security property: once a file is resolved, it is accessed only through this handle. An attacker cannot redirect the serve to a different file by swapping symlinks after resolution.

### `ResolvedDirectory`

A directory handle for listing and child resolution.

```rust
pub struct ResolvedDirectory {
    pub dir_fd: Option<OwnedFd>,     // Unix: directory descriptor
    pub path: PathBuf,               // canonicalized directory path
}
```

## Unix Descriptor-Relative Traversal (`unix.rs`)

The strongest security guarantee. Each path component is resolved using:

1. **`statat(AT_SYMLINK_NOFOLLOW)`** — Check if the component is a symlink
2. **`openat(O_NOFOLLOW)`** — Open the component, rejecting symlinks at the kernel level

This prevents **symlink-swap TOCTOU attacks**: if an attacker swaps a regular file for a symlink between the `statat` and `openat` calls, the kernel returns `ELOOP` or `EMLINK`, which is treated as symlink denial.

### Traversal Algorithm

```
open(root_fd, O_DIRECTORY | O_NOFOLLOW)
    │
    ▼
for component in path.components:
    │
    ├── statat(fd, component, AT_SYMLINK_NOFOLLOW)
    │   ├── Is symlink? → Denied(SymlinkDenied)
    │   └── Is directory? → openat(fd, component, O_DIRECTORY | O_NOFOLLOW)
    │       └── fd = new fd
    │
    └── (continue to next component)
    │
    ▼
final fd → ResolvedFile or ResolvedDirectory
```

### ELOOP / EMLINK Handling

If `openat` returns `ELOOP` (too many symlink levels) or `EMLINK` (too many links), the kernel is detecting a cycle or attack. These are treated as `SymlinkDenied` rather than followed.

## Non-Unix Fallback

On non-Unix platforms (or in follow-symlinks mode), component-wise `symlink_metadata` checks are used. This is weaker than descriptor-relative traversal because:

- There is a TOCTOU window between `symlink_metadata` and `open`
- Symlink swaps within this window may be followed

This is explicitly documented as outside the descriptor-relative hardening guarantee.

## `RootGuard` Lifecycle

1. Created at the start of `handle_request()`
2. Canonicalizes the configured root path
3. Opens the root as a directory descriptor (Unix)
4. Passed to `resolve()` for path resolution
5. Dropped at the end of the request (closes directory fd)

The guard ensures the root is valid and holds the directory open for the duration of the request, preventing the root from being replaced between requests.

## Security Properties

1. **Descriptor-relative** — On Unix with safe defaults, all traversal is relative to the root directory descriptor. No absolute paths are used after the initial root open.
2. **No TOCTOU** — `statat` + `openat` with `O_NOFOLLOW` prevents symlink-swap attacks.
3. **Kernel-enforced** — Symlink rejection is enforced by the kernel via `O_NOFOLLOW`, not by userspace checks.
4. **Pre-opened handles** — `ResolvedFile` carries a `File` handle. The file is never re-opened by path.
5. **Per-request isolation** — Each request gets its own `RootGuard` and directory descriptor.

## See Also

- [path-confinement.md](path-confinement.md) — Path validation before filesystem access
- [policy-system.md](policy-system.md) — Symlink policy configuration
- [primitives-api.md](primitives-api.md) — Public API for `SecureRoot`

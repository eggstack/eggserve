# Filesystem Confinement — Deep Dive

After path validation, filesystem confinement resolves the validated path against the configured root directory. This layer prevents path traversal and symlink escape, even under concurrent modification. Root identity is pinned at startup via `PinnedRoot`, ensuring the running server is never retargeted by pathname changes.

## Module Map

| Module | File | Purpose |
|--------|------|---------|
| `mod.rs` | `fs/mod.rs` | `PinnedRoot` (pinned root identity), `RootGuard`, `ResolvedResource`, `ResolvedFile`, `ResolvedDirectory` |
| `unix.rs` | `fs/unix.rs` | Descriptor-relative traversal (statat + openat) |

## Core Types

### `PinnedRoot`

Opened once at server startup and retained for the server lifetime. Requests resolve relative to this persistent root, ensuring that renaming or replacing the configured pathname does not redirect the running server to a different tree.

```rust
pub(crate) struct PinnedRoot {
    canonical_root: PathBuf,     // canonicalized root path
    #[cfg(unix)]
    root_fd: fs::File,           // Unix: open directory descriptor
}
```

On Unix, holds an open directory fd that is cloned per-request via `try_clone()`. Cloning duplicates the underlying file descriptor, preserving the same root identity across concurrent requests.

### `RootGuard`

Per-request guard that borrows a `PinnedRoot` rather than opening the root independently. On Unix, cloning the `PinnedRoot` fd gives each request its own directory descriptor without reopening the root.

```rust
pub(crate) struct RootGuard<'a> {
    pinned: &'a PinnedRoot,
}
```

Created once per request. Borrowing the pinned root ensures the request resolves against the same root identity that was opened at startup.

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
pub(crate) struct ResolvedFile {
    pub(crate) file: std::fs::File,                  // pre-opened handle
    pub(crate) metadata: std::fs::Metadata,          // length, modified time
    pub(crate) safe_relative_components: Vec<String>, // for MIME detection only
}
```

The key security property: once a file is resolved, it is accessed only through this handle. An attacker cannot redirect the serve to a different file by swapping symlinks after resolution.

#### Capability boundary

The public `primitives::ResolvedFile` exposes extraction methods (`into_std_file`, `into_parts`, `from_parts`) behind the `python-bindings-internal` feature gate. These exist for cross-crate Python bindings where the file was already resolved through a secure path. **Extracting a raw `std::fs::File` ends the confinement guarantee** — the handle is no longer tracked by the resolver. External consumers should use `into_body(plan)` or `into_range_body(start, end_inclusive)` to convert to a `BodySource` that carries the handle forward without exposing it to arbitrary use. See [docs/secure-root.md](../docs/secure-root.md#capability-boundary) for details.

### `ResolvedDirectory`

A directory handle for listing and child resolution.

```rust
pub(crate) struct ResolvedDirectory {
    dir_fd: fs::File,              // Unix: directory descriptor
    #[cfg(windows)]
    dir_handle: OwnedHandle,       // Windows: retained directory handle for child resolution
    canonical_path: PathBuf,       // canonicalized directory path
    components: Vec<String>,       // path components relative to root
}
```

On Windows, `ResolvedDirectory` retains an `OwnedHandle` for handle-relative child resolution, analogous to the Unix `dir_fd`. This handle is used by `RootGuard::resolve_child` to traverse child entries without reopening by path.

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

Plan 084 has implemented handle-relative child resolution on Windows using `CreateFileW` with `FILE_FLAG_OPEN_REPARSE_POINT`. A full ADR is available at [architecture/adr-002-windows-handle-relative-filesystem.md](adr-002-windows-handle-relative-filesystem.md). `ResolvedDirectory` on Windows retains an `OwnedHandle` for handle-relative child resolution (analogous to Unix `dir_fd`), and `RootGuard::resolve_child` uses handle-relative traversal on Windows. Directory enumeration uses `NtQueryDirectoryFile` on the retained directory handle, eliminating the path-based fallback entirely.

## `RootGuard` Lifecycle

1. Created at the start of `handle_request()`, borrowing the `PinnedRoot`
2. Clones the pinned root fd on Unix (no reopen)
3. Passed to `resolve()` for path resolution
4. Dropped at the end of the request (closes cloned fd)

The guard borrows the pinned root identity established at startup. No root reopening or re-canonicalization occurs per request.

## Security Properties

1. **Pinned root identity** — `PinnedRoot` is opened once at startup and retained for the server lifetime. Changing the root pathname does not retarget the running server; restart/reconstruction is required to serve a replacement root.
2. **Descriptor-relative** — On Unix with safe defaults, all traversal is relative to the root directory descriptor. No absolute paths are used after the initial root open.
3. **No TOCTOU** — `statat` + `openat` with `O_NOFOLLOW` prevents symlink-swap attacks.
4. **Kernel-enforced** — Symlink rejection is enforced by the kernel via `O_NOFOLLOW`, not by userspace checks.
5. **Pre-opened handles** — `ResolvedFile` carries a `File` handle. The file is never re-opened by path.
6. **Per-request isolation** — Each request gets its own `RootGuard` (borrowing the pinned root) and a cloned directory descriptor.

## Resolution-Path Audit (Plan 034 Workstream A)

This section traces every path from HTTP request target to response body, proving that no serving path reopens a reconstructed filesystem path after secure resolution.

### Full trace: request → response body

| Step | Code | What happens | Handle lifecycle |
|------|------|-------------|-----------------|
| 1. Parse | `path/mod.rs: ConfinedPath::parse` | Length check → origin-form parse → single-pass percent decode → normalize slashes → split components → validate each (NUL, `/`, `.`, `..`, backslash, dotfile, double-encoded traversal, platform checks) | No handles |
| 2. Validate | `service.rs: handle_request` | Validates GET/HEAD, rejects bodies, builds `PathPolicy` from `StaticPolicy` | No handles |
| 3. Root guard | `fs/mod.rs: RootGuard::new` | Borrows `PinnedRoot`, clones its fd on Unix | Cloned `root_fd` for traversal |
| 4. Resolve | `fs/mod.rs: RootGuard::resolve` | Dispatches to `unix::resolve_fd_relative` (safe defaults) or `resolve_fallback` (follow-symlinks) | `root_fd` used for traversal |
| 5. fd-relative traversal | `fs/unix.rs: resolve_fd_relative` | Per component: dotfile check → `statat(AT_SYMLINK_NOFOLLOW)` symlink check → `openat(O_NOFOLLOW)`. Intermediate: `O_DIRECTORY\|O_NOFOLLOW`. Final: `O_RDONLY\|O_NOFOLLOW`. Previous fd dropped. | Per-component fds opened and dropped; final fd → `ResolvedFile.file` |
| 6. Fallback resolution | `fs/mod.rs: resolve_fallback` | Component-wise `symlink_metadata` checks → `fs::canonicalize` → `starts_with(canonical_root)` → `fs::metadata` → open | Final `File` → `ResolvedFile.file` |
| 7. Response plan | `service.rs` → `primitives/planner.rs` | `plan_file_response()` produces `StaticResponsePlan` (status, headers, `BodyPlan`) | No handles opened |
| 8. Body conversion | `fs/mod.rs: ResolvedFile::into_body` | Consumes `self.file` into `BodySource::FileFull` or `BodySource::FileRange` | `file` moved into `BodySource` |
| 9. Streaming | `service.rs: body_source_to_response` → `response.rs: file_response` / `file_response_range` | `std::fs::File` → `tokio::fs::File::from_std(file)`, acquires semaphore permit, streams via `AsyncReadExt::read` in 8KB chunks | `tokio::fs::File` + semaphore permit owned by stream closure |

### Key invariant

**A running server pins root identity. Changing the root pathname does not retarget the server. Restart/reconstruction is required to serve a replacement root.**

**After resolution, no code path reopens a file by path.** The `File` handle opened during resolution is carried through `ResolvedFile` → `BodySource` → `tokio::fs::File` → streaming body without any intermediate path reconstruction or reopening.

Evidence:
- `safe_relative_components` is used **only** for MIME detection (`fs/mod.rs:49`, `secure_root.rs:84,151,168`)
- `construct_path()` in `unix.rs:238-243` builds `canonical_path` for `ResolvedDirectory` — this is a logical path for `starts_with` verification, never opened after initial resolution
- `resolve_child` in `secure_root.rs:215-219` creates a new `RootGuard` and calls `guard.resolve_child()` — re-resolves from the parent `dir_fd`, not from a reconstructed absolute path

### Handle lifecycle summary

| Stage | Handle opened? | Where | Consumed/transferred? |
|-------|---------------|-------|----------------------|
| `RootGuard::new` | Cloned `root_fd` (borrows from `PinnedRoot`) | `fs/mod.rs:190` | Lives until request ends |
| `unix::resolve_fd_relative` | Per-component `openat` fd | `fs/unix.rs:72` | Previous fd dropped; final fd → `ResolvedFile.file` |
| `ResolvedFile::into_body` | No new open | `fs/mod.rs:40-76` | Moves `self.file` into `BodySource` |
| `body_source_to_response` | No new open | `service.rs:317,330` | `file` → `tokio::fs::File::from_std()` |
| `file_response` / `file_response_range` | No new open | `response.rs:93,143` | File + semaphore permit owned by stream unfold closure |

### Non-regular file rejection

- Unix fd-relative: `unix.rs:100-103` checks `(mode & S_IFMT) != S_IFREG` → `NotFound`
- Fallback: `fs/mod.rs:249-250` checks `!meta.is_file()` → `NotFound`
- FIFOs, sockets, block/char devices all rejected. Symlinks caught by `statat` pre-check.

## Pathname-Bearing Type Inventory (Plan 061 Track A)

Every type that carries path data is classified by its role in the serving pipeline:

| Type | Field | Classification | Notes |
|------|-------|---------------|-------|
| `PinnedRoot` | `canonical_root` | Diagnostic + fallback resolution | Canonical path for error messages and non-Unix fallback. Never opened after initial `PinnedRoot::new()`. |
| `PinnedRoot` | `root_fd` | Opened-resource owner | Unix directory descriptor, opened once, cloned per-request. The sole root authority. |
| `RootGuard` | `pinned` | Borrowed authority | Borrows `&PinnedRoot`. Never opens root by path. |
| `ResolvedFile` | `safe_relative_components` | Safe relative display data | Used only for MIME detection. Never used for file access. |
| `ResolvedFile` | `file` | Opened-resource owner | Pre-opened file handle. Consumed by `into_body()`. Never reopened by path. |
| `ResolvedFile` | `metadata` | Snapshot at resolution time | `fs::Metadata` captured during resolution. Used for ETag, Last-Modified, Content-Length. |
| `ResolvedDirectory` | `canonical_path` | Diagnostic + fallback listing | Used for error messages. On Unix, listing uses `dir_fd`. On Windows, listing uses `NtQueryDirectoryFile` on the retained handle. |
| `ResolvedDirectory` | `dir_fd` | Opened-resource owner | Unix directory descriptor for child resolution and listing. |
| `ResolvedDirectory` | `components` | Safe relative display data | Path components relative to root. Used for child resolution identity. |
| `ConfinedPath` | (internal components) | Policy input | Parsed request target components. Consumed by `RootGuard::resolve()`. |
| `StaticPolicy` | (all fields) | Policy input | Configuration for symlinks, dotfiles, listing. Never carries path data. |
| `BodySource::FileFull` | `file` | Opened-resource owner | Moved from `ResolvedFile`. Consumed by streaming. Never reopened. |
| `BodySource::FileRange` | `file` | Opened-resource owner | Moved from `ResolvedFile`. Consumed by streaming. Never reopened. |

**Forbidden pattern**: No code path extracts a path from `safe_relative_components` or `canonical_path` and calls `open`, `File::open`, `canonicalize`, or equivalent after initial resolution.

### Stream I/O error behavior (Workstream G)

The file streaming code in `response.rs` propagates read failures through the HTTP body after logging a warning. A seek failure is converted to a generic 500 response before streaming starts. The body error causes Hyper to terminate the affected response/connection instead of silently presenting a successful response with fewer bytes than its `Content-Length`.

The semaphore permit remains owned by the stream state and is released when the stream completes or errors. Error responses do not expose local filesystem paths.

## See Also

- [path-confinement.md](path-confinement.md) — Path validation before filesystem access
- [policy-system.md](policy-system.md) — Symlink policy configuration
- [primitives-api.md](primitives-api.md) — Public API for `SecureRoot`

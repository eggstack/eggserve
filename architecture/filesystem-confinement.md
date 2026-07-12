# Filesystem Confinement — Deep Dive

After path validation, filesystem confinement resolves the validated path against the configured root directory. This layer prevents path traversal and symlink escape, even under concurrent modification.

## Module Map

| Module | File | Purpose |
|--------|------|---------|
| `mod.rs` | `fs/mod.rs` | `RootGuard`, `ResolvedResource`, `ResolvedFile`, `ResolvedDirectory` |
| `unix.rs` | `fs/unix.rs` | Descriptor-relative traversal (statat + openat) |

## Core Types

### `RootGuard`

Per-request guard that canonicalizes the configured root and opens it as a directory descriptor. On Unix, this holds an `fs::File` for the root directory.

```rust
pub(crate) struct RootGuard {
    canonical_root: PathBuf,     // canonicalized root path
    root_fd: fs::File,           // Unix: open directory descriptor
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
    canonical_path: PathBuf,       // canonicalized directory path
    components: Vec<String>,       // path components relative to root
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

## Resolution-Path Audit (Plan 034 Workstream A)

This section traces every path from HTTP request target to response body, proving that no serving path reopens a reconstructed filesystem path after secure resolution.

### Full trace: request → response body

| Step | Code | What happens | Handle lifecycle |
|------|------|-------------|-----------------|
| 1. Parse | `path/mod.rs: ConfinedPath::parse` | Length check → origin-form parse → single-pass percent decode → normalize slashes → split components → validate each (NUL, `/`, `.`, `..`, backslash, dotfile, double-encoded traversal, platform checks) | No handles |
| 2. Validate | `service.rs: handle_request` | Validates GET/HEAD, rejects bodies, builds `PathPolicy` from `StaticPolicy` | No handles |
| 3. Root guard | `fs/mod.rs: RootGuard::new` | `fs::canonicalize(root)` + `fs::File::open(&canonical_root)` | `root_fd` opened |
| 4. Resolve | `fs/mod.rs: RootGuard::resolve` | Dispatches to `unix::resolve_fd_relative` (safe defaults) or `resolve_fallback` (follow-symlinks) | `root_fd` used for traversal |
| 5. fd-relative traversal | `fs/unix.rs: resolve_fd_relative` | Per component: dotfile check → `statat(AT_SYMLINK_NOFOLLOW)` symlink check → `openat(O_NOFOLLOW)`. Intermediate: `O_DIRECTORY\|O_NOFOLLOW`. Final: `O_RDONLY\|O_NOFOLLOW`. Previous fd dropped. | Per-component fds opened and dropped; final fd → `ResolvedFile.file` |
| 6. Fallback resolution | `fs/mod.rs: resolve_fallback` | Component-wise `symlink_metadata` checks → `fs::canonicalize` → `starts_with(canonical_root)` → `fs::metadata` → open | Final `File` → `ResolvedFile.file` |
| 7. Response plan | `service.rs` → `primitives/planner.rs` | `plan_file_response()` produces `StaticResponsePlan` (status, headers, `BodyPlan`) | No handles opened |
| 8. Body conversion | `fs/mod.rs: ResolvedFile::into_body` | Consumes `self.file` into `BodySource::FileFull` or `BodySource::FileRange` | `file` moved into `BodySource` |
| 9. Streaming | `service.rs: body_source_to_response` → `response.rs: file_response` / `file_response_range` | `std::fs::File` → `tokio::fs::File::from_std(file)`, acquires semaphore permit, streams via `AsyncReadExt::read` in 8KB chunks | `tokio::fs::File` + semaphore permit owned by stream closure |

### Key invariant

**After resolution, no code path reopens a file by path.** The `File` handle opened during resolution is carried through `ResolvedFile` → `BodySource` → `tokio::fs::File` → streaming body without any intermediate path reconstruction or reopening.

Evidence:
- `safe_relative_components` is used **only** for MIME detection (`fs/mod.rs:49`, `secure_root.rs:84,151,168`)
- `construct_path()` in `unix.rs:238-243` builds `canonical_path` for `ResolvedDirectory` — this is a logical path for `starts_with` verification, never opened after initial resolution
- `resolve_child` in `secure_root.rs:215-219` creates a new `RootGuard` and calls `guard.resolve_child()` — re-resolves from the parent `dir_fd`, not from a reconstructed absolute path

### Handle lifecycle summary

| Stage | Handle opened? | Where | Consumed/transferred? |
|-------|---------------|-------|----------------------|
| `RootGuard::new` | `root_fd` | `fs/mod.rs:132` | Lives until request ends |
| `unix::resolve_fd_relative` | Per-component `openat` fd | `fs/unix.rs:72` | Previous fd dropped; final fd → `ResolvedFile.file` |
| `ResolvedFile::into_body` | No new open | `fs/mod.rs:40-76` | Moves `self.file` into `BodySource` |
| `body_source_to_response` | No new open | `service.rs:317,330` | `file` → `tokio::fs::File::from_std()` |
| `file_response` / `file_response_range` | No new open | `response.rs:93,143` | File + semaphore permit owned by stream unfold closure |

### Non-regular file rejection

- Unix fd-relative: `unix.rs:100-103` checks `(mode & S_IFMT) != S_IFREG` → `NotFound`
- Fallback: `fs/mod.rs:249-250` checks `!meta.is_file()` → `NotFound`
- FIFOs, sockets, block/char devices all rejected. Symlinks caught by `statat` pre-check.

### Stream I/O error behavior (Workstream G)

The file streaming code in `response.rs` handles I/O errors by silently terminating the stream:

- `response.rs:104`: Full-file stream — `Err(_) => None` ends the stream without propagating the error
- `response.rs:161`: Range stream — same pattern: `Err(_) => None`
- `response.rs:139-141`: Range seek failure — returns 500 `planned_response` (not a stream error)

**Properties:**
- Stream errors terminate cleanly (no infinite loops, no panics)
- Semaphore permit is dropped when the unfold closure ends, releasing the file-stream slot
- No local filesystem paths are leaked to clients in error responses
- HTTP response headers are already sent before streaming begins, so a mid-stream termination results in a truncated body with correct `Content-Length` header (client sees an incomplete response)

**Known gaps:**
- Stream I/O errors are now logged at warn level via `eprintln!` before terminating the stream
- No explicit mechanism to prevent unsafe connection reuse after a truncated response (HTTP/1.1 keep-alive connections may reuse after a truncated body)
- These are acceptable for a static file server: stream errors are rare (file deleted after open, disk failure), and the response is already partially sent

## See Also

- [path-confinement.md](path-confinement.md) — Path validation before filesystem access
- [policy-system.md](policy-system.md) — Symlink policy configuration
- [primitives-api.md](primitives-api.md) — Public API for `SecureRoot`

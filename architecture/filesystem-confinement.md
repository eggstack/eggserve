# Filesystem Confinement — Deep Dive

After path validation, filesystem confinement resolves the validated path against the configured root directory. This layer prevents path traversal and symlink escape, even under concurrent modification. Root identity is pinned at startup via `PinnedRoot`, ensuring the running server is never retargeted by pathname changes.

## Module Map

| Module | File | Purpose |
|--------|------|---------|
| `mod.rs` | `fs/mod.rs` | `PinnedRoot` (pinned root identity), `RootGuard`, `ResolvedResource`, `ResolvedFile`, `ResolvedDirectory` |
| `unix.rs` | `fs/unix.rs` | Descriptor-relative traversal (statat + openat) |
| `windows.rs` | `fs/windows.rs` | Handle-relative traversal (NtOpenFile, NtQueryDirectoryFile), reparse-point denial, directory buffer parsing (Windows only) |

## Core Types

### `PinnedRoot`

Opened once at server startup and retained for the server lifetime. Requests resolve relative to this persistent root, ensuring that renaming or replacing the configured pathname does not redirect the running server to a different tree.

```rust
pub(crate) struct PinnedRoot {
    canonical_root: PathBuf,     // canonicalized root path
    #[cfg(unix)]
    root_fd: fs::File,           // Unix: open directory descriptor
    #[cfg(windows)]
    root_handle: windows::OwnedHandle, // Windows: retained root handle
}
```

On Unix, holds an open directory fd that the resolver duplicates for request-scoped traversal. On Windows, holds an `OwnedHandle` opened once with `CreateFileW` using `FILE_FLAG_OPEN_REPARSE_POINT`. For ordinary descendant traversal, the resolver uses that retained handle directly as `ObjectAttributes.RootDirectory` authority; the root handle is duplicated only when an owned root-directory result is required.

### `RootGuard`

Per-request guard that borrows a `PinnedRoot` rather than opening the root independently. On Unix, the resolver duplicates the `PinnedRoot` fd into request-scoped traversal state. On Windows, the resolver uses the retained root handle directly as `RootDirectory` authority for ordinary traversal.

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

On non-Unix platforms without handle support (or in follow-symlinks mode), component-wise `symlink_metadata` checks are used. This is weaker than descriptor-relative traversal because:

- There is a TOCTOU window between `symlink_metadata` and `open`
- Symlink swaps within this window may be followed

This is explicitly documented as outside the descriptor-relative hardening guarantee.

**Windows handle-relative is stronger than the fallback.** Windows uses true handle-relative traversal via `NtOpenFile` with `ObjectAttributes.RootDirectory` and `NtQueryDirectoryFile` for enumeration. Under the hardened profile (symlinks denied), Windows uses handle-relative traversal exclusively — no path reconstruction is used as filesystem authority. A full ADR is available at [architecture/adr-002-windows-handle-relative-filesystem.md](adr-002-windows-handle-relative-filesystem.md). A comprehensive adversarial test suite covers reparse-point denial, namespace normalization, concurrent mutation races, root identity, file validators, ACL/sharing, resource stability, and installed artifact parity.

## `RootGuard` Lifecycle

1. `ServeState` pins the configured root once during static-service construction
2. Each static request creates a `RootGuard` from that pinned root
3. `RootGuard` borrows the pinned root; the resolver duplicates the root fd on Unix or uses the retained root handle directly on Windows for request-scoped traversal
4. Resolution uses that request-scoped authority without reopening the configured root pathname
5. The request-scoped guard is dropped after planning; any file handle retained by the canonical response follows its own streaming lifetime

One pinned root per static service. One request-scoped `RootGuard` per static request. The guard borrows the pinned root identity established at startup. No root reopening or re-canonicalization occurs per request.

## Security Properties

1. **Pinned root identity** — `PinnedRoot` is opened once at startup and retained for the server lifetime. Changing the root pathname does not retarget the running server; restart/reconstruction is required to serve a replacement root.
2. **Descriptor-relative (Unix)** — On Unix with safe defaults, all traversal is relative to the root directory descriptor. No absolute paths are used after the initial root open.
3. **Handle-relative (Windows)** — On Windows with safe defaults, all traversal is relative to the retained root handle via `NtOpenFile` with `ObjectAttributes.RootDirectory`. Directory enumeration uses `NtQueryDirectoryFile` on the retained directory handle. No path reconstruction is used as filesystem authority.
4. **No TOCTOU** — `statat` + `openat` with `O_NOFOLLOW` prevents symlink-swap attacks (Unix). `FILE_FLAG_OPEN_REPARSE_POINT` suppresses reparse following at every level (Windows).
5. **Kernel-enforced** — Symlink rejection is enforced by the kernel via `O_NOFOLLOW` (Unix) or `FILE_ATTRIBUTE_REPARSE_POINT` checks from `GetFileInformationByHandleEx` (Windows).
6. **Pre-opened handles** — `ResolvedFile` carries a `File` handle. The file is never re-opened by path.
7. **Per-request isolation** — Each request gets its own `RootGuard` (borrowing the pinned root). The resolver duplicates the root fd on Unix; on Windows, the retained root handle is used directly for ordinary traversal.

## Resolution-Path Audit

This section traces every path from HTTP request target to response body, proving that no serving path reopens a reconstructed filesystem path after secure resolution.

### Full trace: request → response body

| Step | Code | What happens | Handle lifecycle |
|------|------|-------------|-----------------|
| 1. Parse | `path/mod.rs: ConfinedPath::parse` | Length check → origin-form parse → single-pass percent decode → normalize slashes → split components → validate each (NUL, `/`, `.`, `..`, backslash, dotfile, double-encoded traversal, platform checks) | No handles |
| 2. Validate | `StaticService::call` | Validates GET/HEAD, rejects bodies, and builds `PathPolicy` from `StaticPolicy` | No handles |
| 3. Root guard | `fs/mod.rs: RootGuard::new` | Borrows `PinnedRoot`; resolver duplicates root fd on Unix or uses retained root handle on Windows | Request-scoped traversal authority |
| 4a. Resolve (Unix) | `fs/mod.rs: RootGuard::resolve` | Dispatches to `unix::resolve_fd_relative` (safe defaults) or `resolve_fallback` (follow-symlinks) | `root_fd` used for traversal |
| 4b. Resolve (Windows) | `fs/mod.rs: RootGuard::resolve` | Dispatches to `windows::resolve_to_resource` (handle-relative) or `resolve_fallback` (follow-symlinks) | `root_handle` used for traversal |
| 5. fd-relative traversal (Unix) | `fs/unix.rs: resolve_fd_relative` | Per component: dotfile check → `statat(AT_SYMLINK_NOFOLLOW)` symlink check → `openat(O_NOFOLLOW)`. Intermediate: `O_DIRECTORY\|O_NOFOLLOW`. Final: `O_RDONLY\|O_NONBLOCK\|O_NOFOLLOW`. Previous fd dropped. | Per-component fds opened and dropped; final fd → `ResolvedFile.file` |
| 5b. handle-relative traversal (Windows) | `fs/windows.rs: resolve_to_resource` | Per component: dotfile check → `NtOpenFile` (via `open_directory_relative` or `open_file_relative`). Intermediate dir check via `get_file_standard_info`. Reparse check via `deny_all_reparse_check` / `GetFileInformationByHandleEx`. Previous handle dropped. | Per-component handles opened and dropped; final handle → `ResolvedFile.file` or retained in `ResolvedDirectory` |
| 6. Fallback resolution | `fs/mod.rs: resolve_fallback` | Component-wise `symlink_metadata` checks → `fs::canonicalize` → `starts_with(canonical_root)` → `fs::metadata` → open | Final `File` → `ResolvedFile.file` |
| 7. Response plan | `service.rs` → `primitives/planner.rs` | `plan_file_response()` produces `StaticResponsePlan` (status, headers, `BodyPlan`) | No handles opened |
| 8. Body conversion | `fs/mod.rs: ResolvedFile::into_body` | Consumes `self.file` into `BodySource::FileFull` or `BodySource::FileRange` | `file` moved into `BodySource` |
| 9. Streaming | Runtime canonical transport conversion → `response.rs: file_response` / `file_response_range` | `std::fs::File` → `tokio::fs::File::from_std(file)`, acquires the server-wide semaphore permit, streams via `AsyncReadExt::read` | `tokio::fs::File` + semaphore permit owned by stream closure |

### Key invariant

**A running server pins root identity. Changing the root pathname does not retarget the server. Restart/reconstruction is required to serve a replacement root.**

**After resolution, no code path reopens a file by path.** The `File` handle opened during resolution is carried through `ResolvedFile` → `BodySource` → `tokio::fs::File` → streaming body without any intermediate path reconstruction or reopening.

Evidence:
- `safe_relative_components` is used **only** for MIME detection (`fs/mod.rs:52,69`, `secure_root.rs:85,178,195,218`)
- `construct_path()` in `unix.rs:259-265` builds `canonical_path` for `ResolvedDirectory` — this is a logical path for `starts_with` verification, never opened after initial resolution
- `resolve_child` in `secure_root.rs:241-244` creates a new `RootGuard` and calls `guard.resolve_child()` — re-resolves from the parent `dir_fd`, not from a reconstructed absolute path

### Handle lifecycle summary

| Stage | Handle opened? | Where | Consumed/transferred? |
|-------|---------------|-------|----------------------|
| `RootGuard::new` | Borrows `PinnedRoot` | `fs/mod.rs:275` | Lives until request ends |
| `unix::resolve_fd_relative` | Per-component `openat` fd | `fs/unix.rs:83` | Previous fd dropped; final fd → `ResolvedFile.file` |
| `windows::resolve_to_resource` | Per-component `NtOpenFile` handle | `fs/windows.rs:565` (dir), `631` (file) | Previous handle dropped; final handle → `ResolvedFile.file` or `ResolvedDirectory.dir_handle` |
| `windows::resolve_child_relative` | Single child `NtOpenFile` handle | `fs/windows.rs:1001-1005` | Handle → `ResolvedFile.file` or `ResolvedDirectory.dir_handle` |
| `windows::list_directory_handle` | `NtQueryDirectoryFile` on retained handle | `fs/windows.rs:1079` | Buffer owned by call; no handle transfer |
| `ResolvedFile::into_body` | No new open | `fs/mod.rs:43-79` | Moves `self.file` into `BodySource` |
| canonical runtime transport conversion | No new open | `server/connection.rs` | `file` → `tokio::fs::File::from_std()` |
| `file_response` / `file_response_range` | No new open | `response.rs:93,143` | File + semaphore permit owned by stream unfold closure |

### Non-regular file rejection

- Unix fd-relative: `unix.rs:111-113` checks `(mode & S_IFMT) != S_IFREG` → `NotFound`
- Windows handle-relative: `windows.rs:898-901` checks `get_file_standard_info(directory == 0)` to distinguish files from directories; reparse points rejected by `deny_all_reparse_check`
- Fallback: `fs/mod.rs:458-459` checks `!meta.is_file()` → `NotFound`
- FIFOs, sockets, block/char devices all rejected. Symlinks caught by `statat` pre-check (Unix) or `FILE_ATTRIBUTE_REPARSE_POINT` check (Windows).

## Pathname-Bearing Type Inventory

Every type that carries path data is classified by its role in the serving pipeline:

| Type | Field | Classification | Notes |
|------|-------|---------------|-------|
| `PinnedRoot` | `canonical_root` | Diagnostic + fallback resolution | Canonical path for error messages and non-Unix fallback. Never opened after initial `PinnedRoot::new()`. |
| `PinnedRoot` | `root_fd` | Opened-resource owner | Unix directory descriptor, opened once, duplicated by the resolver for request-scoped traversal. The sole root authority. |
| `PinnedRoot` | `root_handle` | Opened-resource owner | Windows directory handle, opened once with `FILE_FLAG_OPEN_REPARSE_POINT`, used directly by the resolver for ordinary traversal and duplicated only when an owned root-directory result is required. The sole root authority. |
| `RootGuard` | `pinned` | Borrowed authority | Borrows `&PinnedRoot`. Never opens root by path. |
| `ResolvedFile` | `safe_relative_components` | Safe relative display data | Used only for MIME detection. Never used for file access. |
| `ResolvedFile` | `file` | Opened-resource owner | Pre-opened file handle. Consumed by `into_body()`. Never reopened by path. |
| `ResolvedFile` | `metadata` | Snapshot at resolution time | `fs::Metadata` captured during resolution. Used for ETag, Last-Modified, Content-Length. |
| `ResolvedDirectory` | `canonical_path` | Diagnostic + fallback listing | Used for error messages. On Unix, listing uses `dir_fd`. On Windows, listing uses `NtQueryDirectoryFile` on the retained handle. |
| `ResolvedDirectory` | `dir_fd` | Opened-resource owner | Unix directory descriptor for child resolution and listing. |
| `ResolvedDirectory` | `dir_handle` | Opened-resource owner | Windows directory handle for child resolution (`NtOpenFile`) and listing (`NtQueryDirectoryFile`). `OwnedHandle::try_clone()` is fallible. |
| `ResolvedDirectory` | `components` | Safe relative display data | Path components relative to root. Used for child resolution identity. |
| `ConfinedPath` | (internal components) | Policy input | Parsed request target components. Consumed by `RootGuard::resolve()`. |
| `StaticPolicy` | (all fields) | Policy input | Configuration for symlinks, dotfiles, listing. Never carries path data. |
| `BodySource::FileFull` | `file` | Opened-resource owner | Moved from `ResolvedFile`. Consumed by streaming. Never reopened. |
| `BodySource::FileRange` | `file` | Opened-resource owner | Moved from `ResolvedFile`. Consumed by streaming. Never reopened. |

**Forbidden pattern**: No code path extracts a path from `safe_relative_components` or `canonical_path` and calls `open`, `File::open`, `canonicalize`, or equivalent after initial resolution.

### Stream I/O error behavior (Workstream G)

The file streaming code in `response.rs` propagates read failures through the HTTP body after logging a warning. A seek failure is converted to a generic 500 response before streaming starts. The body error causes Hyper to terminate the affected response/connection instead of silently presenting a successful response with fewer bytes than its `Content-Length`.

The semaphore permit remains owned by the stream state and is released when the stream completes or errors. Error responses do not expose local filesystem paths.

## Filesystem Race Test Taxonomy (Plan CORRECTIVE-CLOSURE-PHASES-31-35, Track G)

The filesystem confinement guarantee rests on two pillars: **proof by design** (the kernel enforces `O_NOFOLLOW`/`FILE_FLAG_OPEN_REPARSE_POINT`) and **stress evidence** (bounded adversarial scheduling under concurrent mutation). The test suite in `tests/filesystem_race_qualification.rs` is categorized by what each test proves.

### Proof-by-design tests

These tests exercise code paths that rely on kernel-enforced invariants. If the kernel returns `ELOOP`/`EMLINK` on `openat(O_NOFOLLOW)`, the request is denied — this is a structural guarantee, not a probabilistic one.

- **Descriptor-relative traversal invariant** — verifies that a symlink swapped into the path between `statat` and `openat` causes `openat` to fail rather than follow the new target. Under safe defaults this is enforced by the kernel; the test proves the code path exercises it.
- **Kernel-enforced `O_NOFOLLOW` behavior** — tests that rely on the kernel returning `ELOOP`/`EMLINK` when `openat` encounters a symlink with `O_NOFOLLOW`, proving the defense is not purely software-level.

### Stress evidence tests

These tests complement the structural argument by showing no outside-root bytes are served under bounded adversarial scheduling. They do not prove absence of all races — they demonstrate that the common mutation patterns do not leak.

- **Sequential post-mutation regression** — single mutation then verify: serve old content, new content, or reject — never mixed or escaped. Proves resolution logic is consistent under single-writer mutation.
- **Concurrent race stress** — concurrent reads and writes stress the resolution pipeline. Two bounded concurrent swap stress tests (`concurrent_symlink_swap_stress`, `concurrent_directory_swap_stress`) exercise repeated resolution under adversarial scheduling.

### Invariant test matrix

| Category | What it proves | Evidence type |
|----------|---------------|---------------|
| Descriptor-relative traversal invariant | `openat(O_NOFOLLOW)` rejects swapped symlinks | Proof by design (kernel-enforced) |
| Kernel-enforced `O_NOFOLLOW` | `ELOOP`/`EMLINK` from kernel | Proof by design (kernel-enforced) |
| Sequential post-mutation regression | Consistent outcome under single mutation | Structural correctness |
| Concurrent race stress | No outside-root bytes under adversarial scheduling | Stress evidence (bounded) |

**Key distinction**: proof-by-design tests fail deterministically if the kernel invariant is violated. Stress evidence tests demonstrate resilience under bounded adversarial scheduling but cannot prove absence of all races.

## See Also

- [path-confinement.md](path-confinement.md) — Path validation before filesystem access
- [policy-system.md](policy-system.md) — Symlink policy configuration
- [primitives-api.md](primitives-api.md) — Public API for `SecureRoot`

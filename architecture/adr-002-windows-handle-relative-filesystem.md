# ADR-002: Windows Handle-Relative Filesystem Confinement

## Status

**Accepted** (implemented — Plans 084, 085)

## Context

eggserve provides descriptor-relative filesystem confinement on Unix via `statat(AT_SYMLINK_NOFOLLOW)` + `openat(O_NOFOLLOW)` in `fs/unix.rs`. On Windows, `PinnedRoot` currently stores only a `PathBuf` — no handle is retained. All non-Unix resolution falls through to `resolve_fallback()` which uses `symlink_metadata` + `canonicalize`, weaker than the Unix path.

Windows has parser-level protections (reserved names, ADS, drive prefixes, backslash) in `path/platform.rs`. Plans 062–065 are the roadmap for Windows reparse-point hardening. Plan 061 established `PinnedRoot` and `RootGuard` abstractions.

This ADR documents the feasibility spike for Windows handle-relative filesystem operations, proving that the same open-once confinement invariant can be implemented on Windows.

## Decision

Prove that the same open-once confinement invariant can be implemented on Windows using handle-relative operations.

## API Choice and Fallback Hierarchy

### Primary: `NtOpenFile` with `OBJECT_ATTRIBUTES.RootDirectory`

- Lower-level NT API for root-relative opens
- `OBJECT_ATTRIBUTES.RootDirectory` provides native handle-relative open semantics
- More control over disposition, share mode, and options
- Requires `ntdll.dll` — dynamically resolved via `LoadLibraryW`/`GetProcAddress`
- Semi-documented but widely used by Windows system programming
- Available since Windows XP
- **Production API** as of Plan 084 — used for all handle-relative child resolution

### Secondary: `CreateFileW` with `FILE_FLAG_OPEN_REPARSE_POINT`

- Well-documented, stable Win32 API
- `FILE_FLAG_OPEN_REPARSE_POINT` opens the reparse point object itself rather than following it
- Available on all Windows versions since Windows XP/Server 2003
- Can be used with a root directory handle via `HANDLE` parameter
- Share mode `FILE_SHARE_READ | FILE_SHARE_DELETE` for concurrent access
- Retained as a fallback path for non-hardened modes

### Diagnostic: `GetFinalPathNameByHandleW`

- Defense-in-depth diagnostics to verify handle identity
- Returns the final normalized path for an opened handle
- Does NOT use this path for serving — only for logging/verification

### Recommended approach

As of Plan 084, `NtOpenFile` with `OBJECT_ATTRIBUTES.RootDirectory` is the production API for handle-relative child resolution. It provides native handle-relative semantics without the limitations of `CreateFileW`. `CreateFileW` is retained as the fallback for non-hardened modes.

## Minimum Supported Windows Version

- Windows 8 / Server 2012 (for consistent `FILE_ATTRIBUTE_TAG_INFO` support)
- `CreateFileW` with `FILE_FLAG_OPEN_REPARSE_POINT` is available since Windows XP SP2
- `GetFileInformationByHandleEx` with `FileAttributeTagInfo` requires Windows Vista+
- Recommended minimum: Windows 8 (reasonable for 2026+ deployment)

## ntdll Import Strategy

- Dynamically resolve `NtCreateFile`/`NtOpenFile` via `LoadLibraryW`/`GetProcAddress` at runtime
- Fallback gracefully if `ntdll` calls are unavailable (log warning, use `CreateFileW` path)
- This avoids a hard link to `ntdll.dll` which may not be available in all Windows environments

## Desired Access, Share, Disposition, and Option Flags

### Directory open (root and intermediate):

```
dwDesiredAccess: FILE_LIST_DIRECTORY
dwShareMode: FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
dwCreationDisposition: OPEN_EXISTING
dwFlagsAndAttributes: FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS
```

### File open (final component):

```
dwDesiredAccess: GENERIC_READ
dwShareMode: FILE_SHARE_READ | FILE_SHARE_DELETE
dwCreationDisposition: OPEN_EXISTING
dwFlagsAndAttributes: FILE_FLAG_OPEN_REPARSE_POINT
```

### Key flags:

- `FILE_FLAG_OPEN_REPARSE_POINT`: Open the reparse point itself, do not traverse
- `FILE_FLAG_BACKUP_SEMANTICS`: Required for directory handles on Windows
- `FILE_SHARE_READ | FILE_SHARE_DELETE`: Allow concurrent reads and deletes

## HANDLE Ownership and Duplication Rules

- Each opened handle is owned by the Rust wrapper struct
- `DuplicateHandle` is used when cloning handles (e.g., for `PinnedRoot::clone`)
- `CloseHandle` is called on drop
- No handle escaping to external code
- Handle values are never stored globally — always held in struct fields

## NTSTATUS/Win32 Error Conversion

- `CreateFileW` returns `INVALID_HANDLE_VALUE` on failure
- Call `GetLastError()` to retrieve Win32 error code
- Map common errors:
  - `ERROR_FILE_NOT_FOUND` → `NotFound`
  - `ERROR_ACCESS_DENIED` → `NotFound` (security)
  - `ERROR_NOT_A_DIRECTORY` → `NotFound`
  - `ERROR_TOO_MANY_LINKS` → `Denied` (symlink)
  - `ERROR_CANT_RESOLVE_FILENAME` → `Denied` (reparse)
- For `NtCreateFile`: check `NTSTATUS` directly
  - `STATUS_OBJECT_NAME_NOT_FOUND` → `NotFound`
  - `STATUS_ACCESS_DENIED` → `NotFound`
  - `STATUS_REPARSE_POINT_NOT_TRAVERSED` → `Denied`

## Directory vs File Open Semantics

- Directories require `FILE_FLAG_BACKUP_SEMANTICS` on `CreateFileW`
- Use `GetFileInformationByHandleEx` with `FileStandardInfo` to determine if the handle is a directory
- Intermediate components must be directories; open with directory semantics and check attributes
- Final component may be file or directory

## Cancellation and Blocking Behavior

- `CreateFileW` is synchronous and non-cancellable from Rust
- Network filesystems (SMB) may block — Tokio integration via `spawn_blocking`
- Local NTFS opens are near-instant (< 1ms)
- No async Windows file I/O API is needed for the prototype

## Tokio Integration

- File operations use `std::fs::File` wrapping the Windows `HANDLE`
- Tokio's `tokio::fs::File` wraps `std::fs::File` internally
- Conversion: extract `HANDLE` from `std::fs::File`, pass to `CreateFileW` for relative opens, wrap result in `std::fs::File` via `from_raw_handle`
- `spawn_blocking` for any Windows API calls that may block (SMB)

## CPython Wheel Builds

- The `windows-sys` crate compiles on all Rust targets including cross-compilation
- `cfg(windows)` gates ensure no Windows code compiles on Unix/macOS
- Python wheel builds use maturin on Windows runners — `windows-sys` is a pure Rust dependency
- No Python C API interaction needed for filesystem operations

## Security Implications of NT APIs

- `NtCreateFile` is semi-documented but stable and widely used
- Risk: undocumented parameters or behavioral changes across Windows versions
- Mitigation: prefer `CreateFileW` (documented), use `NtCreateFile` only as fallback
- Risk: handle leaks if error paths miss `CloseHandle`
- Mitigation: RAII wrapper with `Drop` implementation
- Risk: race conditions between handle open and attribute query
- Mitigation: same approach as Unix — open with `NOFOLLOW`, check attributes from the opened handle

## Testability

- GitHub-hosted Windows runners support basic filesystem tests
- Symlink/junction creation requires Developer Mode or administrator privileges
- Tests requiring elevated privileges should report `skipped-with-reason` in generic CI
- Dedicated Windows runners with Developer Mode can run the full test suite

## Go/No-Go Conclusion

### GO

The feasibility spike demonstrates that:

1. `CreateFileW` with `FILE_FLAG_OPEN_REPARSE_POINT` can open reparse points without following them
2. Handle-relative opens are possible using the Windows API
3. `GetFileInformationByHandleEx` can detect reparse points and retrieve file identity
4. Directory enumeration from an open handle is possible via `FindFirstFileW`/`FindNextFileW`
5. `std::fs::File` can be constructed from a raw `HANDLE` for Tokio integration
6. The `windows-sys` crate provides all needed API bindings with minimal dependency surface

### Unsupported environments (detected by spike):

- SMB/network shares: reparse-point behavior is inconsistent; functional-only
- ReFS: reparse-point semantics differ from NTFS; functional-only
- FAT32/exFAT: no reparse points; functional-only
- Cloud placeholder files (OneDrive): may appear as reparse points; functional-only

## Plan 084 Completion

Plan 084 completed the production implementation of Windows handle-relative filesystem confinement:

- **Directory handle retention**: `ResolvedDirectory` on Windows retains an `OwnedHandle` for handle-relative traversal. `OwnedHandle::try_clone()` is fallible (not `Clone`), so the owned handle is preserved rather than cloned.
- **Handle-relative child resolution**: `RootGuard::resolve_child` uses `NtOpenFile` with `OBJECT_ATTRIBUTES.RootDirectory` for handle-relative opens in hardened mode.
- **Index handle-relative**: Directory index resolution uses the retained directory handle, not path reconstruction.
- **Unicode child names**: Non-ASCII child names resolve correctly through the `NtOpenFile` production path with correct UTF-16 lengths.
- **Handle ownership semantics**: `OwnedHandle` duplication is fallible and non-panicking; borrowed handles are never closed by owners.
- **Hardened no-fallback**: Hardened Windows resolution never reconstructs filesystem authority from a path.

Reparse-point hardening remains deferred (Plans 085–086).

## Plan 085: Windows Handle-Relative Directory Enumeration

### Selected API

`NtQueryDirectoryFile` with `FileIdBothDirectoryInfo` (information class 10).

This is the least-complex API that provides handle-relative enumeration, UTF-16 filename length in bytes, file attributes and reparse indication, stable file identity, and deterministic continuation through a caller-owned bounded buffer.

### Windows Support Floor

**Windows XP+** — `ntdll.dll` is always present and exports `NtQueryDirectoryFile`. No version-gating required. `FILE_ID_BOTH_DIR_INFO` is supported on NTFS and ReFS from Windows XP onward; on FAT32/exFAT the file identity field is zeroed, which is handled by the parser.

### Buffer Layout

`FILE_ID_BOTH_DIR_INFO` structure:

```
Byte offset  Size  Field
-----------  ----  -----
0            4     NextEntryOffset (0 for final entry)
4            4     FileIndex (8-byte stable identity, low 4 bytes)
8            8     CreationTime
16           8     LastAccessTime
24           8     LastWriteTime
32           8     ChangeTime
40           4     EndOfFile (low 4 bytes)
44           4     EndOfFile (high 4 bytes)
48           4     AllocationSize (low 4 bytes)
52           4     AllocationSize (high 4 bytes)
56           4     FileAttributes
60           4     FileNameLength (in bytes, must be even)
64           2     FileIndex (high 2 bytes)
66           64    Reserved
66+FileNameLen var  FileName (UTF-16LE, variable length)
```

Total header: 80 bytes. Each entry is 8-byte aligned via `NextEntryOffset`.

### Restart Semantics

- First call: `restart_scan=TRUE` — discards any cached state, returns entries from the beginning.
- Subsequent calls: `restart_scan=FALSE` — resumes from the last returned entry.
- The caller passes the directory handle; no path reconstruction is involved.

### End-of-Directory

`NtQueryDirectoryFile` returns `STATUS_NO_MORE_FILES` (`0x80000006`) when the last entry has been returned. This is the canonical termination signal for the enumeration loop. The caller treats it as end-of-directory and stops.

### Filename Encoding

- UTF-16LE, always.
- `FileNameLength` is in **bytes** (not code units). Must be even; odd values are rejected by the parser.
- The filename range must lie within the returned buffer.
- Invalid UTF-16 sequences (unpaired surrogates) are handled deterministically without unsafe truncation — entries with invalid names are omitted with a categorized event.

### File Identity

- `FileIndex` (8-byte field from the header) provides a stable identity across renames within the same volume.
- On FAT32/exFAT where `FileIndex` is zeroed, identity is unavailable — the entry is still valid but identity is `None`.
- `FileIndex` is never used as authority for reopening; reopened handles are validated independently.

### File Attributes

From the `FILE_ID_BOTH_DIR_INFO` header:

- `FILE_ATTRIBUTE_DIRECTORY` (0x10) — entry is a directory.
- `FILE_ATTRIBUTE_REPARSE_POINT` (0x400) — entry is a reparse point (junction, symlink, mount point).
- In the hardened branch, all reparse-point entries are denied regardless of reparse tag. The attribute bit is sufficient for denial; no `ReparseTag` parsing is needed.

### Cancellation and Blocking

- `NtQueryDirectoryFile` is synchronous and non-cancellable from Rust.
- SMB/network shares may block — integration uses `spawn_blocking` to avoid blocking the Tokio executor.
- Local NTFS enumeration is near-instant (< 1ms for typical directories).
- A blocked OS call cannot cause the server to claim all work has stopped; the file-operation semaphore bounds concurrency.

### Error Mapping

NTSTATUS negative values (errors) map to `WindowsFsError::IoError`:

| NTSTATUS | Meaning | Mapped to |
|----------|---------|-----------|
| `STATUS_NO_MORE_FILES` (0x80000006) | End of directory | Loop termination (not error) |
| `STATUS_INVALID_PARAMETER` (0xC000000D) | Bad buffer or class | `WindowsFsError::IoError` |
| `STATUS_ACCESS_DENIED` (0xC0000022) | Handle lacks `FILE_LIST_DIRECTORY` | `WindowsFsError::IoError` |
| `STATUS_NOT_A_DIRECTORY` (0xC0000103) | Handle is not a directory | `WindowsFsError::IoError` |
| Other negative NTSTATUS | Unexpected kernel error | `WindowsFsError::IoError` |

Positive NTSTATUS values (informational) are not expected from `NtQueryDirectoryFile`.

### Why Not `FindFirstFileW`

`FindFirstFileW` and `FindNextFileW` require a **path** as input, not a handle. To use them from a retained handle, the implementation would need to reconstruct the path via `GetFinalPathNameByHandleW`. This reintroduces TOCTOU:

1. `GetFinalPathNameByHandleW` returns a normalized path, not the original — namespace normalization may differ from the original request.
2. The reconstructed path could reference a different directory if a rename or replace occurred between handle open and enumeration.
3. It splits filesystem authority between the retained handle (for child opens) and the reconstructed path (for enumeration), creating a two-source security model.

`NtQueryDirectoryFile` operates directly on the directory handle — no path reconstruction, no TOCTOU window. The invariant is preserved: all directory entries come from the already-validated handle.

## Consequences

- Plan 084 implemented the production resolver using `NtOpenFile` with `OBJECT_ATTRIBUTES.RootDirectory` as the primary API
- `ResolvedDirectory` retains an `OwnedHandle` for handle-relative child resolution; `OwnedHandle::try_clone()` is fallible
- Windows hardened profile requires passing all handle-relative child resolution tests (Plans 084 gates)
- `windows-sys` is a Windows-only dependency (feature-gated)
- The existing `resolve_fallback()` path remains for non-hardened modes
- Parser-level protections are retained as a first line of defense
- Plan 085 implements handle-relative directory enumeration via `NtQueryDirectoryFile` with `FileIdBothDirectoryInfo` (class 10), replacing path-based enumeration in the hardened branch
- Plan 086 completes the roadmap for reparse-point hardening via adversarial qualification
- Windows hardened profile promotion requires passing all reparse-point race tests (Plans 085–086)

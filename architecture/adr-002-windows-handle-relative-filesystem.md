# ADR-002: Windows Handle-Relative Filesystem Confinement

## Status

**Proposed** (feasibility spike — not yet implemented)

## Context

eggserve provides descriptor-relative filesystem confinement on Unix via `statat(AT_SYMLINK_NOFOLLOW)` + `openat(O_NOFOLLOW)` in `fs/unix.rs`. On Windows, `PinnedRoot` currently stores only a `PathBuf` — no handle is retained. All non-Unix resolution falls through to `resolve_fallback()` which uses `symlink_metadata` + `canonicalize`, weaker than the Unix path.

Windows has parser-level protections (reserved names, ADS, drive prefixes, backslash) in `path/platform.rs`. Plans 062–065 are the roadmap for Windows reparse-point hardening. Plan 061 established `PinnedRoot` and `RootGuard` abstractions.

This ADR documents the feasibility spike for Windows handle-relative filesystem operations, proving that the same open-once confinement invariant can be implemented on Windows.

## Decision

Prove that the same open-once confinement invariant can be implemented on Windows using handle-relative operations.

## API Choice and Fallback Hierarchy

### Primary: `CreateFileW` with `FILE_FLAG_OPEN_REPARSE_POINT`

- Well-documented, stable Win32 API
- `FILE_FLAG_OPEN_REPARSE_POINT` opens the reparse point object itself rather than following it
- Available on all Windows versions since Windows XP/Server 2003
- Can be used with a root directory handle via `HANDLE` parameter
- Share mode `FILE_SHARE_READ | FILE_SHARE_DELETE` for concurrent access

### Secondary: `NtCreateFile` / `NtOpenFile` with `OBJECT_ATTRIBUTES.RootDirectory`

- Lower-level NT API for root-relative opens
- `OBJECT_ATTRIBUTES.RootDirectory` provides native handle-relative open semantics
- More control over disposition, share mode, and options
- Requires `ntdll.dll` — either statically linked via `windows-sys` or dynamically resolved
- Semi-documented but widely used by Windows system programming
- Available since Windows XP

### Diagnostic: `GetFinalPathNameByHandleW`

- Defense-in-depth diagnostics to verify handle identity
- Returns the final normalized path for an opened handle
- Does NOT use this path for serving — only for logging/verification

### Recommended approach

Use `CreateFileW` as the primary API. It is fully documented, stable, and sufficient for handle-relative opens. Fall back to `NtCreateFile` only if `CreateFileW` cannot provide the needed root-relative semantics.

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

## Consequences

- Plan 063 can implement the production resolver using the approved API subset
- Windows hardened profile promotion requires passing all reparse-point race tests
- `windows-sys` will be added as a Windows-only dependency (feature-gated)
- The existing `resolve_fallback()` path remains for non-hardened modes
- Parser-level protections are retained as a first line of defense

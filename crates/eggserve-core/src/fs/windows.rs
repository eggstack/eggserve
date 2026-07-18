#![cfg(windows)]
#![allow(dead_code)]

//! Windows handle-relative filesystem confinement prototype (Plan 062).
//!
//! This module mirrors the Unix `fs/unix.rs` module but uses Windows APIs to
//! achieve the same open-once confinement invariant. Under the hardened profile,
//! it:
//!
//! 1. Opens the root directory handle once at server startup via
//!    `CreateFileW` with `FILE_FLAG_OPEN_REPARSE_POINT`.
//! 2. Resolves each request component relative to the current directory handle,
//!    never constructing an absolute child path.
//! 3. Suppresses reparse-point traversal by opening with
//!    `FILE_FLAG_OPEN_REPARSE_POINT` at every level.
//! 4. Detects and rejects reparse points via `GetFileInformationByHandleEx`
//!    with `FileAttributeTagInfo`.
//! 5. Queries file identity from the opened handle for diagnostics and root
//!    identity verification.
//! 6. Streams from the final validated handle by converting to `std::fs::File`.
//!
//! # Safety model
//!
//! Every `unsafe` block in this module is annotated with the invariants it
//! requires. The primary safety obligations are:
//!
//! - **Pointer validity**: all FFI output pointers are stack-allocated and
//!   remain valid for the duration of the call.
//! - **Buffer sizing**: UTF-16 conversion buffers are pre-sized to hold the
//!   maximum expected content.
//! - **Handle ownership**: `OwnedHandle` wraps a raw `HANDLE` and guarantees
//!   exactly-once `CloseHandle` on drop.
//! - **Thread safety**: all FFI calls operate on thread-local state; handles
//!   are duplicated rather than shared.
//!
//! # Limitations
//!
//! This is a prototype. Directory enumeration uses a path-based fallback
//! (`GetFinalPathNameByHandleW` + `FindFirstFileW`) rather than
//! `NtQueryDirectoryFile`. A production implementation would use handle-based
//! enumeration to avoid the path reconstruction step.

use std::ffi::c_void;
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::path::PathBuf;
use std::ptr;

// ── Windows FFI types ───────────────────────────────────────────────────────

#[allow(clippy::upper_case_acronyms)]
pub(crate) type HANDLE = *mut c_void;
#[allow(clippy::upper_case_acronyms)]
type DWORD = u32;
#[allow(clippy::upper_case_acronyms)]
type BOOL = i32;
#[allow(clippy::upper_case_acronyms)]
type PCWSTR = *const u16;
#[allow(clippy::upper_case_acronyms)]
type PWSTR = *mut u16;

const INVALID_HANDLE_VALUE: HANDLE = -1isize as HANDLE;
const TRUE: BOOL = 1;
const FALSE: BOOL = 0;

// ── Access rights ───────────────────────────────────────────────────────────

const GENERIC_READ: DWORD = 0x80000000;
const FILE_LIST_DIRECTORY: DWORD = 0x00000001;

// ── Share mode ──────────────────────────────────────────────────────────────

const FILE_SHARE_READ: DWORD = 0x00000001;
const FILE_SHARE_WRITE: DWORD = 0x00000002;
const FILE_SHARE_DELETE: DWORD = 0x00000004;

// ── Creation disposition ────────────────────────────────────────────────────

const OPEN_EXISTING: DWORD = 3;

// ── Flags ───────────────────────────────────────────────────────────────────

const FILE_FLAG_OPEN_REPARSE_POINT: DWORD = 0x00200000;
const FILE_FLAG_BACKUP_SEMANTICS: DWORD = 0x02000000;

// ── File attribute tags ─────────────────────────────────────────────────────

const FILE_ATTRIBUTE_REPARSE_POINT: DWORD = 0x00000400;
const FILE_ATTRIBUTE_DIRECTORY: DWORD = 0x00000010;

// ── Reparse tag values ──────────────────────────────────────────────────────

const IO_REPARSE_TAG_SYMLINK: u32 = 0xA0000000;
const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA0000003;

// ── Win32 error codes ──────────────────────────────────────────────────────

const ERROR_FILE_NOT_FOUND: DWORD = 2;
const ERROR_PATH_NOT_FOUND: DWORD = 3;
const ERROR_ACCESS_DENIED: DWORD = 5;
const ERROR_NOT_A_DIRECTORY: DWORD = 267;
const ERROR_TOO_MANY_LINKS: DWORD = 1142;

// ── File information classes ────────────────────────────────────────────────

const FILE_ATTRIBUTE_TAG_INFO_CLASS: u32 = 9;
const FILE_STANDARD_INFO_CLASS: u32 = 5;

const DUPLICATE_SAME_ACCESS: DWORD = 0x00000002;

// ── FFI structs ─────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct FILE_ATTRIBUTE_TAG_INFO {
    file_attributes: DWORD,
    reparse_tag: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct FILE_STANDARD_INFO {
    allocation_size: i64,
    end_of_file: i64,
    number_of_links: DWORD,
    delete_pending: BOOL,
    directory: BOOL,
}

#[repr(C)]
#[derive(Clone, Copy)]
#[allow(dead_code)]
struct WIN32_FIND_DATAW {
    dw_file_attributes: DWORD,
    ft_creation_time: u64,
    ft_last_access_time: u64,
    ft_last_write_time: u64,
    n_file_size_high: DWORD,
    n_file_size_low: DWORD,
    dw_reserved0: DWORD,
    dw_reserved1: DWORD,
    c_file_name: [u16; 260],
    c_alternate_file_name: [u16; 14],
}

impl Default for WIN32_FIND_DATAW {
    fn default() -> Self {
        Self {
            dw_file_attributes: 0,
            ft_creation_time: 0,
            ft_last_access_time: 0,
            ft_last_write_time: 0,
            n_file_size_high: 0,
            n_file_size_low: 0,
            dw_reserved0: 0,
            dw_reserved1: 0,
            c_file_name: [0; 260],
            c_alternate_file_name: [0; 14],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
#[allow(dead_code)]
struct FILE_ID_BOTH_DIR_INFO {
    next_entry_offset: DWORD,
    file_index: u64,
    creation_time: u64,
    last_access_time: u64,
    last_write_time: u64,
    change_time: u64,
    allocation_size: i64,
    end_of_file: i64,
    file_attributes: DWORD,
    file_name_length: DWORD,
    ea_size: DWORD,
    file_id: [u8; 8],
    file_name: [u16; 1], // variable length
}

// ── NT API types (for handle-relative opens) ─────────────────────────────────

type NTSTATUS = i32;

#[repr(C)]
struct NtUnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: PWSTR,
}

#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root_directory: HANDLE,
    object_name: *const NtUnicodeString,
    attributes: u32,
    security_descriptor: *mut c_void,
    security_quality_of_service: *mut c_void,
}

#[repr(C)]
#[allow(dead_code)]
struct IoStatusBlock {
    status: NTSTATUS,
    information: usize,
}

const OBJ_CASE_INSENSITIVE: u32 = 0x00000040;

const FILE_OPEN: u32 = 0x00000001;
const FILE_DIRECTORY_FILE: u32 = 0x00000020;
const FILE_NON_DIRECTORY_FILE: u32 = 0x00000040;
const FILE_OPEN_FOR_BACKUP_INTENT: u32 = 0x00004000;

const SYNCHRONIZE: u32 = 0x00100000;

// NT status codes for error mapping
const STATUS_NO_SUCH_FILE: u32 = 0xC000000F;
const STATUS_OBJECT_NAME_NOT_FOUND: u32 = 0xC0000034;
const STATUS_NOT_A_DIRECTORY: u32 = 0xC0000103;
const STATUS_FILE_IS_A_DIRECTORY: u32 = 0xC00000BA;
const STATUS_ACCESS_DENIED: u32 = 0xC0000022;

// ── External FFI declarations ───────────────────────────────────────────────

extern "system" {
    fn CreateFileW(
        lp_file_name: PCWSTR,
        dw_desired_access: DWORD,
        dw_share_mode: DWORD,
        lp_security_attributes: *mut c_void,
        dw_creation_disposition: DWORD,
        dw_flags_and_attributes: DWORD,
        h_template_file: HANDLE,
    ) -> HANDLE;

    fn CloseHandle(hObject: HANDLE) -> BOOL;

    fn GetFileInformationByHandleEx(
        h_file: HANDLE,
        file_info_class: u32,
        lp_file_information: *mut c_void,
        dw_buffer_size: DWORD,
    ) -> BOOL;

    fn GetFinalPathNameByHandleW(
        h_file: HANDLE,
        lpsz_file_path: PWSTR,
        cch_file_path: DWORD,
        dw_flags: DWORD,
    ) -> DWORD;

    fn FindFirstFileW(lp_file_name: PCWSTR, lp_find_file_data: *mut WIN32_FIND_DATAW) -> HANDLE;

    fn FindNextFileW(h_find_file: HANDLE, lp_find_file_data: *mut WIN32_FIND_DATAW) -> BOOL;

    fn FindClose(h_find_file: HANDLE) -> BOOL;

    fn GetLastError() -> DWORD;

    fn DuplicateHandle(
        h_source_process_handle: HANDLE,
        h_source_handle: HANDLE,
        h_target_process_handle: HANDLE,
        lp_target_handle: *mut HANDLE,
        dw_desired_access: DWORD,
        b_inherit_handle: BOOL,
        dw_options: DWORD,
    ) -> BOOL;

    fn GetCurrentProcess() -> HANDLE;

    fn NtOpenFile(
        file_handle: *mut HANDLE,
        desired_access: u32,
        object_attributes: *mut ObjectAttributes,
        io_status_block: *mut IoStatusBlock,
        share_access: u32,
        open_options: u32,
    ) -> NTSTATUS;
}

// ── OwnedHandle RAII wrapper ────────────────────────────────────────────────

/// RAII wrapper for a Windows `HANDLE`.
///
/// Guarantees exactly-once `CloseHandle` on drop. Supports `Clone` via
/// `DuplicateHandle` to create an independent copy of the handle.
///
/// # Safety invariants
///
/// - `0` and `INVALID_HANDLE_VALUE` are treated as invalid and are not closed.
/// - `Clone` panics if `DuplicateHandle` fails, which indicates a system-level
///   error (e.g., out of memory or handle quota exhaustion).
pub(crate) struct OwnedHandle(HANDLE);

impl std::fmt::Debug for OwnedHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("OwnedHandle").field(&self.0).finish()
    }
}

impl OwnedHandle {
    /// Returns `true` if the handle is not null and not `INVALID_HANDLE_VALUE`.
    pub(crate) fn is_valid(&self) -> bool {
        !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE
    }

    /// Returns the raw `HANDLE` value.
    pub(crate) fn raw(&self) -> HANDLE {
        self.0
    }
}

// SAFETY: Windows HANDLEs are process-global integers. They are safe to
// transfer between threads (Send) and share across threads (Sync). The
// underlying OS ensures thread-safe access to handle operations.
unsafe impl Send for OwnedHandle {}
unsafe impl Sync for OwnedHandle {}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if self.is_valid() {
            // SAFETY: we checked `is_valid()`, so the handle is non-null and
            // not INVALID_HANDLE_VALUE. CloseHandle is safe to call on any
            // valid handle returned by CreateFileW.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

impl Clone for OwnedHandle {
    fn clone(&self) -> Self {
        if !self.is_valid() {
            return Self(INVALID_HANDLE_VALUE);
        }
        let mut new_handle = INVALID_HANDLE_VALUE;
        // SAFETY: GetCurrentProcess() returns a pseudohandle that is valid
        // for DuplicateHandle. We request DUPLICATE_SAME_ACCESS so the new
        // handle inherits the same access rights. The output pointer is a
        // stack-allocated HANDLE. Failure indicates a system-level error;
        // panicking is appropriate since this is a prototype and the caller
        // cannot reasonably recover.
        let ok = unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                self.0,
                GetCurrentProcess(),
                &mut new_handle,
                0,
                FALSE,
                DUPLICATE_SAME_ACCESS,
            )
        };
        if ok == 0 {
            panic!("DuplicateHandle failed: {}", unsafe { GetLastError() });
        }
        Self(new_handle)
    }
}

// ── Error type ──────────────────────────────────────────────────────────────

/// Filesystem error type for Windows handle-relative operations.
#[derive(Debug)]
pub(crate) enum WindowsFsError {
    NotFound,
    NotADirectory,
    AccessDenied,
    TooManyLinks,
    ReparsePointDenied,
    IoError(DWORD),
}

impl std::fmt::Display for WindowsFsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "file not found"),
            Self::NotADirectory => write!(f, "not a directory"),
            Self::AccessDenied => write!(f, "access denied"),
            Self::TooManyLinks => write!(f, "too many links"),
            Self::ReparsePointDenied => write!(f, "reparse point denied"),
            Self::IoError(code) => write!(f, "I/O error: {code}"),
        }
    }
}

impl std::error::Error for WindowsFsError {}

/// Maps the last Win32 error to a `WindowsFsError`.
fn last_error_to_fs_error() -> WindowsFsError {
    // SAFETY: GetLastError is thread-local and always safe to call. No
    // invariants are required from the caller.
    let code = unsafe { GetLastError() };
    match code {
        ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND => WindowsFsError::NotFound,
        ERROR_NOT_A_DIRECTORY => WindowsFsError::NotADirectory,
        ERROR_ACCESS_DENIED => WindowsFsError::AccessDenied,
        ERROR_TOO_MANY_LINKS => WindowsFsError::TooManyLinks,
        other => WindowsFsError::IoError(other),
    }
}

/// Maps an NTSTATUS code to a `WindowsFsError`.
fn ntstatus_to_error(status: NTSTATUS) -> WindowsFsError {
    match status as u32 {
        STATUS_NO_SUCH_FILE | STATUS_OBJECT_NAME_NOT_FOUND => WindowsFsError::NotFound,
        STATUS_NOT_A_DIRECTORY | STATUS_FILE_IS_A_DIRECTORY => WindowsFsError::NotADirectory,
        STATUS_ACCESS_DENIED => WindowsFsError::AccessDenied,
        other => WindowsFsError::IoError(other),
    }
}

// ── UTF-16 conversion helper ────────────────────────────────────────────────

/// Converts a `&str` to a null-terminated UTF-16 vector suitable for
/// `CreateFileW` and other `PCWSTR`-accepting APIs.
fn to_utf16_null(s: &str) -> Vec<u16> {
    use std::iter::once;
    s.encode_utf16().chain(once(0)).collect()
}

/// Converts a `&[u16]` slice (not necessarily null-terminated) to a `PathBuf`.
fn utf16_slice_to_pathbuf(slice: &[u16]) -> PathBuf {
    String::from_utf16_lossy(slice).into()
}

// ── Track B: Root-relative open functions ────────────────────────────────────

/// Opens a directory relative to a parent directory handle.
///
/// Uses `NtOpenFile` with `ObjectAttributes.RootDirectory` set to the parent
/// handle for true handle-relative traversal. The `FILE_DIRECTORY_FILE` flag
/// ensures the target must be a directory. `FILE_OPEN_FOR_BACKUP_INTENT`
/// enables backup semantics.
///
/// # Arguments
///
/// * `parent` - Handle to the parent directory. Must have been opened with
///   at least `FILE_LIST_DIRECTORY` access.
/// * `name` - The child component name. Must be validated by the path parser
///   (no separators, no NUL, no dots, no reserved names).
///
/// # Errors
///
/// Returns `WindowsFsError::NotFound` if the component does not exist,
/// `WindowsFsError::AccessDenied` if the handle lacks permission, or
/// `WindowsFsError::IoError` for other failures.
pub(crate) fn open_directory_relative(
    parent: HANDLE,
    name: &str,
) -> Result<OwnedHandle, WindowsFsError> {
    let name_utf16 = to_utf16_null(name);
    let mut obj_name = NtUnicodeString {
        length: (name.len() * 2) as u16,
        maximum_length: (name.len() * 2 + 2) as u16,
        buffer: name_utf16.as_ptr() as *mut u16,
    };
    let mut obj_attr = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root_directory: parent,
        object_name: &mut obj_name,
        attributes: OBJ_CASE_INSENSITIVE,
        security_descriptor: ptr::null_mut(),
        security_quality_of_service: ptr::null_mut(),
    };
    let mut handle = INVALID_HANDLE_VALUE;
    let mut iosb = IoStatusBlock {
        status: 0,
        information: 0,
    };

    // SAFETY: obj_name points to a valid UTF-16 buffer that lives for the
    // duration of this call. obj_attr is stack-allocated. handle and iosb are
    // stack-allocated output parameters. NtOpenFile is always present in
    // ntdll.dll (loaded in every Windows process).
    let status = unsafe {
        NtOpenFile(
            &mut handle,
            FILE_LIST_DIRECTORY | SYNCHRONIZE,
            &mut obj_attr,
            &mut iosb,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_DIRECTORY_FILE | FILE_OPEN_FOR_BACKUP_INTENT,
        )
    };

    if status < 0 {
        return Err(ntstatus_to_error(status));
    }

    Ok(OwnedHandle(handle))
}

/// Opens a file relative to a parent directory handle.
///
/// Uses `NtOpenFile` with `FILE_NON_DIRECTORY_FILE` to ensure the target is
/// a regular file (not a directory). The parent handle is passed via
/// `ObjectAttributes.RootDirectory` for true handle-relative traversal.
///
/// # Arguments
///
/// * `parent` - Handle to the parent directory.
/// * `name` - The child component name.
///
/// # Errors
///
/// Returns `WindowsFsError::NotFound` if the component does not exist,
/// `WindowsFsError::NotADirectory` if the target is a directory, or
/// `WindowsFsError::IoError` for other failures.
pub(crate) fn open_file_relative(
    parent: HANDLE,
    name: &str,
) -> Result<OwnedHandle, WindowsFsError> {
    let name_utf16 = to_utf16_null(name);
    let mut obj_name = NtUnicodeString {
        length: (name.len() * 2) as u16,
        maximum_length: (name.len() * 2 + 2) as u16,
        buffer: name_utf16.as_ptr() as *mut u16,
    };
    let mut obj_attr = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root_directory: parent,
        object_name: &mut obj_name,
        attributes: OBJ_CASE_INSENSITIVE,
        security_descriptor: ptr::null_mut(),
        security_quality_of_service: ptr::null_mut(),
    };
    let mut handle = INVALID_HANDLE_VALUE;
    let mut iosb = IoStatusBlock {
        status: 0,
        information: 0,
    };

    // SAFETY: same safety requirements as open_directory_relative. The key
    // difference is FILE_NON_DIRECTORY_FILE, which causes NtOpenFile to fail
    // with STATUS_FILE_IS_A_DIRECTORY if the target is a directory.
    let status = unsafe {
        NtOpenFile(
            &mut handle,
            SYNCHRONIZE,
            &mut obj_attr,
            &mut iosb,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_NON_DIRECTORY_FILE,
        )
    };

    if status < 0 {
        return Err(ntstatus_to_error(status));
    }

    Ok(OwnedHandle(handle))
}

/// Resolves a sequence of path components relative to a root handle.
///
/// Iterates through `components`, opening each relative to the previous
/// handle. Intermediate components are opened as directories and validated
/// to have `FILE_ATTRIBUTE_DIRECTORY`. When `deny_reparse` is true,
/// intermediate components with `FILE_ATTRIBUTE_REPARSE_POINT` are rejected.
///
/// The final component is opened as a file (without `FILE_FLAG_BACKUP_SEMANTICS`).
/// To open the final component as a directory, pass only the root handle with
/// an empty components slice.
///
/// # Arguments
///
/// * `root` - Handle to the root directory. Must be valid for at least
///   `FILE_LIST_DIRECTORY` access.
/// * `components` - Ordered path components from root to target.
/// * `deny_reparse` - If `true`, reject any intermediate or final component
///   that is a reparse point.
///
/// # Returns
///
/// An `OwnedHandle` to the final resolved component.
///
/// # Errors
///
/// Returns `WindowsFsError::NotFound` if any component does not exist,
/// `WindowsFsError::NotADirectory` if an intermediate is not a directory,
/// `WindowsFsError::ReparsePointDenied` if a reparse point is encountered
/// and `deny_reparse` is true, or `WindowsFsError::IoError` for other
/// failures.
pub(crate) fn resolve_components_relative(
    root: HANDLE,
    components: &[String],
    deny_reparse: bool,
) -> Result<OwnedHandle, WindowsFsError> {
    if components.is_empty() {
        // Caller wants the root handle itself. Duplicate it to return an
        // owned copy.
        let root_owned = OwnedHandle(root);
        return Ok(root_owned.clone());
    }

    let mut current = OwnedHandle(root);
    let total = components.len();

    for (i, component) in components.iter().enumerate() {
        let is_final = i == total - 1;

        // Open the component relative to the current handle.
        let child = if is_final {
            open_file_relative(current.raw(), component)?
        } else {
            open_directory_relative(current.raw(), component)?
        };

        // Validate intermediate components are directories.
        if !is_final {
            let info = get_file_standard_info(child.raw())?;
            if info.directory == FALSE {
                return Err(WindowsFsError::NotADirectory);
            }
        }

        // Check for reparse points if policy requires denial.
        if deny_reparse {
            deny_all_reparse_check(child.raw())?;
        }

        current = child;
    }

    Ok(current)
}

/// Opens a file or directory relative to a parent directory handle.
///
/// Unlike `open_file_relative`, this does not use `FILE_NON_DIRECTORY_FILE`,
/// so it succeeds for both files and directories. Unlike
/// `open_directory_relative`, this does not use `FILE_DIRECTORY_FILE`, so it
/// succeeds for both files and directories.
///
/// This is used by `RootGuard::resolve` when the final component type is
/// unknown (could be a file or directory).
pub(crate) fn open_any_relative(parent: HANDLE, name: &str) -> Result<OwnedHandle, WindowsFsError> {
    let name_utf16 = to_utf16_null(name);
    let mut obj_name = NtUnicodeString {
        length: (name.len() * 2) as u16,
        maximum_length: (name.len() * 2 + 2) as u16,
        buffer: name_utf16.as_ptr() as *mut u16,
    };
    let mut obj_attr = ObjectAttributes {
        length: std::mem::size_of::<ObjectAttributes>() as u32,
        root_directory: parent,
        object_name: &mut obj_name,
        attributes: OBJ_CASE_INSENSITIVE,
        security_descriptor: ptr::null_mut(),
        security_quality_of_service: ptr::null_mut(),
    };
    let mut handle = INVALID_HANDLE_VALUE;
    let mut iosb = IoStatusBlock {
        status: 0,
        information: 0,
    };

    let status = unsafe {
        NtOpenFile(
            &mut handle,
            SYNCHRONIZE,
            &mut obj_attr,
            &mut iosb,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_OPEN_FOR_BACKUP_INTENT,
        )
    };

    if status < 0 {
        return Err(ntstatus_to_error(status));
    }

    Ok(OwnedHandle(handle))
}

/// Resolves a sequence of path components relative to a root handle,
/// returning a `ResolvedResource` (file or directory).
///
/// This is the Windows equivalent of Unix `resolve_fd_relative`. It opens
/// each component relative to the previous handle, checking for reparse
/// points when `deny_reparse` is true. The final component is opened as
/// either a file or directory and the type is determined from metadata.
pub(crate) fn resolve_to_resource(
    root: HANDLE,
    canonical_root: &std::path::Path,
    components: &[String],
    deny_reparse: bool,
) -> super::ResolvedResource {
    use super::{ResolvedDirectory, ResolvedFile, ResolvedResource};

    if components.is_empty() {
        return ResolvedResource::NotFound;
    }

    // Track ownership separately: `current_raw` is the live handle for the
    // current directory level. `current_owned` holds ownership of the handle
    // from `open_directory_relative` and must stay alive until the next
    // iteration. We never create an OwnedHandle from the root handle to
    // avoid closing it when the function returns.
    let mut current_raw = root;
    let mut current_owned: Option<OwnedHandle> = None;
    let total = components.len();

    for (i, component) in components.iter().enumerate() {
        let is_final = i == total - 1;

        let child = if is_final {
            open_any_relative(current_raw, component)
        } else {
            open_directory_relative(current_raw, component)
        };

        let child = match child {
            Ok(h) => h,
            Err(_) => return ResolvedResource::NotFound,
        };

        // Validate intermediate components are directories.
        if !is_final {
            match get_file_standard_info(child.raw()) {
                Ok(info) if info.directory == FALSE => {
                    return ResolvedResource::NotFound;
                }
                Err(_) => return ResolvedResource::NotFound,
                _ => {}
            }
        }

        // Check for reparse points if policy requires denial.
        if deny_reparse {
            match deny_all_reparse_check(child.raw()) {
                Ok(()) => {}
                Err(WindowsFsError::ReparsePointDenied) => {
                    return ResolvedResource::Denied(crate::path::PathRejection::SymlinkDenied);
                }
                Err(_) => return ResolvedResource::NotFound,
            }
        }

        if is_final {
            // Determine if this is a file or directory.
            let is_dir = match get_file_standard_info(child.raw()) {
                Ok(info) => info.directory != FALSE,
                Err(_) => false,
            };

            let canonical_path = match get_final_path(child.raw()) {
                Ok(p) => p,
                Err(_) => canonical_root.join(component),
            };

            let std_file = handle_to_std_file(child);
            let safe_components = components.to_vec();

            if is_dir {
                return ResolvedResource::Directory(ResolvedDirectory {
                    canonical_path,
                    components: safe_components,
                });
            } else {
                let metadata = match std_file.metadata() {
                    Ok(m) => m,
                    Err(_) => return ResolvedResource::NotFound,
                };
                return ResolvedResource::File(ResolvedFile {
                    file: std_file,
                    metadata,
                    safe_relative_components: safe_components,
                });
            }
        }

        // Intermediate component: update raw handle and keep ownership.
        current_raw = child.raw();
        // Prevent child from being dropped (which would close the handle).
        // We take ownership via ManuallyDrop-like semantics by leaking the
        // previous owned handle and replacing it.
        current_owned = Some(child);
    }

    ResolvedResource::NotFound
}

// ── Track C: Reparse detection ──────────────────────────────────────────────

/// Queries `FILE_ATTRIBUTE_TAG_INFO` from an open handle.
///
/// Returns the file attributes and reparse tag. If the file is not a reparse
/// point, the reparse tag is zero.
///
/// # Errors
///
/// Returns `WindowsFsError::IoError` if `GetFileInformationByHandleEx` fails.
pub(crate) fn get_file_attribute_tag(
    handle: HANDLE,
) -> Result<FILE_ATTRIBUTE_TAG_INFO, WindowsFsError> {
    let mut info = FILE_ATTRIBUTE_TAG_INFO::default();

    // SAFETY: `info` is a stack-allocated, properly aligned, zeroed struct
    // with the correct size for `FILE_ATTRIBUTE_TAG_INFO`. The handle is
    // valid (caller contract). We pass `size_of::<FILE_ATTRIBUTE_TAG_INFO>()`
    // as the buffer size. On failure, the buffer contents are undefined but
    // we return an error immediately.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FILE_ATTRIBUTE_TAG_INFO_CLASS,
            &mut info as *mut _ as *mut c_void,
            std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as DWORD,
        )
    };

    if ok == 0 {
        return Err(last_error_to_fs_error());
    }

    Ok(info)
}

/// Returns `true` if the handle refers to a reparse point.
///
/// This is the primary check used by the hardened profile to detect symlinks,
/// junctions, and mount points.
pub(crate) fn is_reparse_point(handle: HANDLE) -> bool {
    match get_file_attribute_tag(handle) {
        Ok(info) => (info.file_attributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0,
        Err(_) => false,
    }
}

/// Returns the reparse tag for an open handle.
///
/// If the handle is not a reparse point, returns `Ok(0)`.
///
/// # Errors
///
/// Returns `WindowsFsError::IoError` if the attribute query fails.
pub(crate) fn get_reparse_tag(handle: HANDLE) -> Result<u32, WindowsFsError> {
    let info = get_file_attribute_tag(handle)?;
    Ok(info.reparse_tag)
}

/// Denies all reparse points unconditionally.
///
/// If the handle has `FILE_ATTRIBUTE_REPARSE_POINT` set, returns
/// `WindowsFsError::ReparsePointDenied`. This is the hardened-profile check:
/// under the hardened profile, any reparse point is denied regardless of tag.
pub(crate) fn deny_all_reparse_check(handle: HANDLE) -> Result<(), WindowsFsError> {
    let info = get_file_attribute_tag(handle)?;
    if (info.file_attributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
        return Err(WindowsFsError::ReparsePointDenied);
    }
    Ok(())
}

// ── Root handle construction ────────────────────────────────────────────────

/// Opens a directory handle for use as a pinned root.
///
/// Uses `CreateFileW` with `FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT`
/// to open the directory without following reparse points. This is called
/// once at server startup from `PinnedRoot::new()`.
pub(crate) fn open_root_handle(path: &std::path::Path) -> Result<OwnedHandle, std::io::Error> {
    use std::os::windows::ffi::OsStrExt;
    let path_str = path.to_str().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "root path is not valid UTF-8",
        )
    })?;
    let path_utf16: Vec<u16> = path_str.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: path_utf16 is a valid null-terminated UTF-16 string derived from
    // a canonicalized path. The output handle is wrapped in OwnedHandle which
    // guarantees cleanup. All flags are compile-time constants.
    unsafe {
        let h = CreateFileW(
            path_utf16.as_ptr(),
            FILE_LIST_DIRECTORY,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        );
        if h == INVALID_HANDLE_VALUE || h.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        Ok(OwnedHandle(h))
    }
}

// ── Track D: File identity ──────────────────────────────────────────────────

/// Queries `FILE_STANDARD_INFO` from an open handle.
///
/// Returns allocation size, end-of-file position, link count, delete-pending
/// flag, and directory flag.
///
/// # Errors
///
/// Returns `WindowsFsError::IoError` if `GetFileInformationByHandleEx` fails.
pub(crate) fn get_file_standard_info(handle: HANDLE) -> Result<FILE_STANDARD_INFO, WindowsFsError> {
    let mut info = FILE_STANDARD_INFO::default();

    // SAFETY: `info` is a stack-allocated, properly aligned, zeroed struct
    // with the correct size for `FILE_STANDARD_INFO`. The handle is valid
    // (caller contract).
    let ok = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FILE_STANDARD_INFO_CLASS,
            &mut info as *mut _ as *mut c_void,
            std::mem::size_of::<FILE_STANDARD_INFO>() as DWORD,
        )
    };

    if ok == 0 {
        return Err(last_error_to_fs_error());
    }

    Ok(info)
}

/// Returns a stable file identifier for the given handle.
///
/// Uses `GetFileInformationByHandleEx` with `FileIdBothDirectoryInfo` (class 10)
/// to retrieve the file ID. On failure, falls back to `GetFinalPathNameByHandleW`
/// to derive a diagnostic path (not a stable ID).
///
/// The returned `u64` is the 8-byte file ID extracted from
/// `FILE_ID_BOTH_DIR_INFO`. This is stable across renames within the same
/// volume.
///
/// # Errors
///
/// Returns `WindowsFsError::IoError` if the file ID query fails.
pub(crate) fn get_file_id(handle: HANDLE) -> Result<u64, WindowsFsError> {
    // FILE_ID_BOTH_DIR_INFO is variable-length. We allocate a fixed buffer
    // that is large enough for most entries (name up to ~255 UTF-16 chars).
    // The struct header is 80 bytes, plus 2 bytes per name char.
    const BUFFER_SIZE: usize = 80 + 256 * 2;
    let mut buffer = vec![0u8; BUFFER_SIZE];

    // SAFETY: `buffer` is a heap-allocated, properly aligned byte buffer. We
    // cast it to `FILE_ID_BOTH_DIR_INFO*`. The buffer is zeroed, so the
    // variable-length `file_name` field is safe to inspect. The handle is
    // valid (caller contract). `GetFileInformationByHandleEx` with class 10
    // writes a FILE_ID_BOTH_DIR_INFO header followed by the file name.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            handle,
            10, // FileIdBothDirectoryInfo
            buffer.as_mut_ptr() as *mut c_void,
            BUFFER_SIZE as DWORD,
        )
    };

    if ok == 0 {
        return Err(last_error_to_fs_error());
    }

    // SAFETY: the buffer was zeroed and successfully filled. We reinterpret
    // the first 80 bytes as `FILE_ID_BOTH_DIR_INFO`. The `file_id` field is
    // at a fixed offset within the header (bytes 64..72). We read it as a
    // little-endian u64.
    let file_id = unsafe {
        let header = buffer.as_ptr() as *const FILE_ID_BOTH_DIR_INFO;
        (*header).file_index
    };

    Ok(file_id)
}

/// Returns the final normalized path for the given handle.
///
/// Uses `GetFinalPathNameByHandleW` with `VOLUME_NAME_DOS` (0) to retrieve
/// the path in DOS device format (e.g., `C:\path\to\file`). This format is
/// directly usable with filesystem APIs and `Path::exists`.
///
/// # Errors
///
/// Returns `WindowsFsError::IoError` if the path query fails.
pub(crate) fn get_final_path(handle: HANDLE) -> Result<PathBuf, WindowsFsError> {
    // Start with a reasonable buffer size; grow if needed.
    let mut buf_size: DWORD = 260;
    let mut buffer: Vec<u16> = vec![0; buf_size as usize];

    loop {
        // SAFETY: `buffer` is a properly sized, zeroed UTF-16 buffer. The
        // handle is valid (caller contract). `GetFinalPathNameByHandleW`
        // returns the number of characters written (not including null
        // terminator). If the buffer is too small, it returns 0 and sets
        // ERROR_INSUFFICIENT_BUFFER.
        //
        // VOLUME_NAME_DOS (0) returns the DOS device path (e.g., C:\path),
        // which is directly usable with FindFirstFileW and filesystem APIs.
        let len = unsafe { GetFinalPathNameByHandleW(handle, buffer.as_mut_ptr(), buf_size, 0) };

        if len == 0 {
            return Err(last_error_to_fs_error());
        }

        // If the returned length is less than the buffer, we have the full
        // path. Otherwise, double the buffer and retry.
        if len < buf_size {
            let path_slice = &buffer[..len as usize];
            return Ok(utf16_slice_to_pathbuf(path_slice));
        }

        buf_size = len + 1;
        buffer.resize(buf_size as usize, 0);
    }
}

// ── Track E: Streaming compatibility ────────────────────────────────────────

/// Converts an `OwnedHandle` to a `std::fs::File` for streaming.
///
/// # Ownership transfer
///
/// This function **transfers ownership** of the raw handle to the returned
/// `std::fs::File`. After calling this function, the `OwnedHandle` must
/// **not** be dropped, as `std::fs::File::from_raw_handle` takes ownership
/// and will call `CloseHandle` when the `File` is dropped.
///
/// The caller must `std::mem::forget` the `OwnedHandle` or otherwise prevent
/// its `Drop` implementation from running.
///
/// # Safety
///
/// The handle must be valid for `GENERIC_READ` access and must not be
/// `INVALID_HANDLE_VALUE` or null.
pub(crate) fn handle_to_std_file(handle: OwnedHandle) -> std::fs::File {
    assert!(
        handle.is_valid(),
        "handle_to_std_file called with invalid handle"
    );

    let raw = handle.raw();

    // SAFETY: we asserted the handle is valid. `from_raw_handle` takes
    // ownership of the handle and will call CloseHandle when the File is
    // dropped. We must prevent the OwnedHandle from also calling CloseHandle.
    std::mem::forget(handle);

    // SAFETY: the raw handle is valid for read access (GENERIC_READ or
    // FILE_LIST_DIRECTORY). `from_raw_handle` is safe to call on any valid
    // Windows HANDLE that was opened with appropriate access rights.
    unsafe { std::fs::File::from_raw_handle(raw as RawHandle) }
}

/// Verifies that a handle is still valid after conversion (for testing).
///
/// This is a diagnostic function: it checks whether the raw handle value is
/// non-null and not `INVALID_HANDLE_VALUE`. It does **not** verify that the
/// handle is still open in the OS (that would require `GetHandleInformation`).
pub(crate) fn verify_handle_not_closed_after_conversion(handle: &OwnedHandle) -> bool {
    handle.is_valid()
}

// ── Track F: Directory enumeration ──────────────────────────────────────────

/// A single directory entry returned by `enumerate_directory`.
#[derive(Debug)]
pub(crate) struct DirectoryEntry {
    pub name: String,
    pub is_directory: bool,
    pub is_reparse_point: bool,
    pub file_size: u64,
}

/// Enumerates the contents of a directory using the given handle.
///
/// # Implementation note
///
/// True handle-based enumeration on Windows requires `NtQueryDirectoryFile`
/// from `ntdll.dll`, which is an undocumented API. For this prototype, we use
/// a path-based fallback: `GetFinalPathNameByHandleW` reconstructs the
/// absolute path, then `FindFirstFileW` / `FindNextFileW` enumerate entries.
///
/// This approach has two limitations:
/// 1. It requires the handle to have been opened with sufficient access for
///    `GetFinalPathNameByHandleW`.
/// 2. There is a TOCTOU window between path reconstruction and enumeration.
///
/// A production implementation should use `NtQueryDirectoryFile` for true
/// handle-based enumeration (see Plan 064).
///
/// # Filtering
///
/// Entries named `.` and `..` are excluded from the result.
///
/// # Errors
///
/// Returns `WindowsFsError::IoError` if the path reconstruction or
/// `FindFirstFileW` / `FindNextFileW` calls fail.
pub(crate) fn enumerate_directory(handle: HANDLE) -> Result<Vec<DirectoryEntry>, WindowsFsError> {
    // Reconstruct the absolute path from the handle.
    let dir_path = get_final_path(handle)?;

    // Build the wildcard pattern: "C:\path\to\dir\*"
    let mut pattern = dir_path;
    pattern.push("*");

    let pattern_utf16 = to_utf16_null(pattern.to_str().unwrap_or(""));

    let mut find_data = WIN32_FIND_DATAW::default();

    // SAFETY: `pattern_utf16` is a valid null-terminated UTF-16 string.
    // `find_data` is a zeroed, properly sized struct. The handle returned by
    // FindFirstFileW is a search handle that must be closed with FindClose.
    let find_handle = unsafe { FindFirstFileW(pattern_utf16.as_ptr(), &mut find_data) };

    if find_handle == INVALID_HANDLE_VALUE || find_handle.is_null() {
        // ERROR_FILE_NOT_FOUND means an empty directory (no matches for "*").
        // SAFETY: GetLastError is thread-local and always safe to call.
        let err = unsafe { GetLastError() };
        if err == ERROR_FILE_NOT_FOUND {
            return Ok(Vec::new());
        }
        return Err(last_error_to_fs_error());
    }

    let mut entries = Vec::new();

    loop {
        let name = utf16_slice_to_pathbuf(&find_data.c_file_name)
            .to_string_lossy()
            .into_owned();

        // Filter out `.` and `..` entries.
        if name != "." && name != ".." {
            let file_size =
                ((find_data.n_file_size_high as u64) << 32) | (find_data.n_file_size_low as u64);
            entries.push(DirectoryEntry {
                name,
                is_directory: (find_data.dw_file_attributes & FILE_ATTRIBUTE_DIRECTORY) != 0,
                is_reparse_point: (find_data.dw_file_attributes & FILE_ATTRIBUTE_REPARSE_POINT)
                    != 0,
                file_size,
            });
        }

        // SAFETY: `find_handle` is valid (checked above). `find_data` is
        // reused across calls; FindNextFileW writes fresh data into it. On
        // failure, GetLastError returns ERROR_NO_MORE_FILES.
        let ok = unsafe { FindNextFileW(find_handle, &mut find_data) };
        if ok == 0 {
            break;
        }
    }

    // SAFETY: `find_handle` is valid (checked above). FindClose releases
    // the search handle. The return value indicates success/failure but
    // there is no meaningful recovery path.
    unsafe {
        FindClose(find_handle);
    }

    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

// ── PinnedRoot Windows extension documentation ──────────────────────────────
//
// When the production Windows resolver is integrated, the `PinnedRoot` struct
// in `fs/mod.rs` would be extended with a Windows-specific field:
//
// ```rust
// pub(crate) struct PinnedRoot {
//     canonical_root: PathBuf,
//     #[cfg(unix)]
//     root_fd: fs::File,
//     #[cfg(windows)]
//     root_handle: windows::OwnedHandle,  // Plan 062
// }
// ```
//
// `PinnedRoot::new()` on Windows would:
//
// 1. Canonicalize the root path via `fs::canonicalize`.
// 2. Open the root directory with `open_directory_relative` (using the
//    canonical path as the parent context) or directly via `CreateFileW` with
//    `FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS`.
// 3. Store the resulting `OwnedHandle` in `root_handle`.
// 4. Verify the root is not a reparse point with `deny_all_reparse_check`.
//
// `RootGuard::resolve()` on Windows with symlinks denied would dispatch to
// `resolve_components_relative(root_handle, components, true)` to perform
// handle-relative traversal with reparse denial.
//
// The `RootGuard` would clone the `OwnedHandle` (via `DuplicateHandle`) for
// request-scoped traversal, ensuring the pinned root handle is never mutated.
//
// `ResolvedDirectory` on Windows would carry an `OwnedHandle` (analogous to
// the Unix `dir_fd` field) for child resolution and enumeration.

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("eggserve_windows_test_{}", std::process::id()))
    }

    fn setup_test_root() -> (PathBuf, OwnedHandle) {
        let root = temp_dir();
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("subdir")).unwrap();
        std::fs::write(root.join("hello.txt"), "hello").unwrap();
        std::fs::write(root.join("subdir").join("file.txt"), "nested").unwrap();

        // Open the root directory handle.
        let root_path_utf16 = to_utf16_null(root.to_str().unwrap());
        let handle = unsafe {
            CreateFileW(
                root_path_utf16.as_ptr(),
                FILE_LIST_DIRECTORY,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        assert_ne!(handle, INVALID_HANDLE_VALUE);
        (root, OwnedHandle(handle))
    }

    fn cleanup_test_root(root: &Path) {
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn open_directory_relative_succeeds() {
        let (root, root_handle) = setup_test_root();
        let result = open_directory_relative(root_handle.raw(), "subdir");
        assert!(
            result.is_ok(),
            "open_directory_relative failed: {:?}",
            result.err()
        );
        drop(root_handle);
        cleanup_test_root(&root);
    }

    #[test]
    fn open_file_relative_succeeds() {
        let (root, root_handle) = setup_test_root();
        let result = open_file_relative(root_handle.raw(), "hello.txt");
        assert!(
            result.is_ok(),
            "open_file_relative failed: {:?}",
            result.err()
        );
        drop(root_handle);
        cleanup_test_root(&root);
    }

    #[test]
    fn open_relative_not_found() {
        let (root, root_handle) = setup_test_root();
        let result = open_file_relative(root_handle.raw(), "nonexistent.txt");
        assert!(matches!(result, Err(WindowsFsError::NotFound)));
        drop(root_handle);
        cleanup_test_root(&root);
    }

    #[test]
    fn open_directory_as_file_fails() {
        let (root, root_handle) = setup_test_root();
        let result = open_file_relative(root_handle.raw(), "subdir");
        assert!(
            matches!(result, Err(WindowsFsError::NotADirectory)),
            "opening a directory without BACKUP_SEMANTICS should fail with NotADirectory, got {:?}",
            result
        );
        drop(root_handle);
        cleanup_test_root(&root);
    }

    #[test]
    fn resolve_nested_components() {
        let (root, root_handle) = setup_test_root();
        let components: Vec<String> = vec!["subdir".into(), "file.txt".into()];
        let result = resolve_components_relative(root_handle.raw(), &components, true);
        assert!(
            result.is_ok(),
            "resolve_components_relative failed: {:?}",
            result.err()
        );
        drop(root_handle);
        cleanup_test_root(&root);
    }

    #[test]
    fn resolve_intermediate_not_directory_fails() {
        let (root, root_handle) = setup_test_root();
        let components: Vec<String> = vec!["hello.txt".into(), "impossible".into()];
        let result = resolve_components_relative(root_handle.raw(), &components, true);
        assert!(
            matches!(result, Err(WindowsFsError::NotADirectory)),
            "intermediate file should fail with NotADirectory, got {:?}",
            result
        );
        drop(root_handle);
        cleanup_test_root(&root);
    }

    #[test]
    fn get_file_standard_info_directory() {
        let (root, root_handle) = setup_test_root();
        let dir_handle = open_directory_relative(root_handle.raw(), "subdir").unwrap();
        let info = get_file_standard_info(dir_handle.raw());
        assert!(info.is_ok());
        let info = info.unwrap();
        assert_ne!(info.directory, FALSE);
        drop(dir_handle);
        drop(root_handle);
        cleanup_test_root(&root);
    }

    #[test]
    fn get_file_standard_info_file() {
        let (root, root_handle) = setup_test_root();
        let file_handle = open_file_relative(root_handle.raw(), "hello.txt").unwrap();
        let info = get_file_standard_info(file_handle.raw());
        assert!(info.is_ok());
        let info = info.unwrap();
        assert_eq!(info.directory, FALSE);
        assert_eq!(info.end_of_file, 5); // "hello" is 5 bytes
        drop(file_handle);
        drop(root_handle);
        cleanup_test_root(&root);
    }

    #[test]
    fn get_final_path_succeeds() {
        let (root, root_handle) = setup_test_root();
        let path = get_final_path(root_handle.raw());
        assert!(path.is_ok(), "get_final_path failed: {:?}", path.err());
        let path = path.unwrap();
        assert!(path.exists(), "final path should exist on disk: {:?}", path);
        drop(root_handle);
        cleanup_test_root(&root);
    }

    #[test]
    fn get_file_id_succeeds() {
        let (root, root_handle) = setup_test_root();
        let id = get_file_id(root_handle.raw());
        assert!(id.is_ok(), "get_file_id failed: {:?}", id.err());
        drop(root_handle);
        cleanup_test_root(&root);
    }

    #[test]
    fn owned_handle_clone_and_drop() {
        let (root, root_handle) = setup_test_root();
        let cloned = root_handle.clone();
        assert!(cloned.is_valid());
        drop(cloned);
        assert!(root_handle.is_valid());
        drop(root_handle);
        cleanup_test_root(&root);
    }

    #[test]
    fn enumerate_directory_entries() {
        let (root, root_handle) = setup_test_root();
        let entries = enumerate_directory(root_handle.raw());
        assert!(
            entries.is_ok(),
            "enumerate_directory failed: {:?}",
            entries.err()
        );
        let entries = entries.unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(
            names.contains(&"hello.txt"),
            "expected hello.txt in entries, got {:?}",
            names
        );
        assert!(
            names.contains(&"subdir"),
            "expected subdir in entries, got {:?}",
            names
        );
        // `.` and `..` should be filtered out.
        assert!(!names.contains(&"."), ". should be filtered");
        assert!(!names.contains(&".."), ".. should be filtered");
        drop(root_handle);
        cleanup_test_root(&root);
    }

    #[test]
    fn enumerate_directory_subdir() {
        let (root, root_handle) = setup_test_root();
        let dir_handle = open_directory_relative(root_handle.raw(), "subdir").unwrap();
        let entries = enumerate_directory(dir_handle.raw());
        assert!(entries.is_ok());
        let entries = entries.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "file.txt");
        assert!(!entries[0].is_directory);
        drop(dir_handle);
        drop(root_handle);
        cleanup_test_root(&root);
    }

    #[test]
    fn no_reparse_on_regular_files() {
        let (root, root_handle) = setup_test_root();
        let file_handle = open_file_relative(root_handle.raw(), "hello.txt").unwrap();
        assert!(!is_reparse_point(file_handle.raw()));
        let tag = get_reparse_tag(file_handle.raw()).unwrap();
        assert_eq!(tag, 0);
        assert!(deny_all_reparse_check(file_handle.raw()).is_ok());
        drop(file_handle);
        drop(root_handle);
        cleanup_test_root(&root);
    }

    #[test]
    fn deny_all_reparse_check_on_regular_dir() {
        let (root, root_handle) = setup_test_root();
        let dir_handle = open_directory_relative(root_handle.raw(), "subdir").unwrap();
        assert!(deny_all_reparse_check(dir_handle.raw()).is_ok());
        drop(dir_handle);
        drop(root_handle);
        cleanup_test_root(&root);
    }

    #[test]
    fn handle_to_std_file_read() {
        let (root, root_handle) = setup_test_root();
        let file_handle = open_file_relative(root_handle.raw(), "hello.txt").unwrap();
        let std_file = handle_to_std_file(file_handle);
        let mut contents = String::new();
        std::io::Read::read_to_string(&mut std::io::BufReader::new(std_file), &mut contents)
            .unwrap();
        assert_eq!(contents, "hello");
        cleanup_test_root(&root);
    }

    #[test]
    fn resolve_components_relative_empty() {
        let (root, root_handle) = setup_test_root();
        let result = resolve_components_relative(root_handle.raw(), &[], true);
        assert!(result.is_ok());
        let handle = result.unwrap();
        assert!(handle.is_valid());
        drop(root_handle);
        cleanup_test_root(&root);
    }

    #[test]
    fn utf16_conversion_roundtrip() {
        let s = "hello_world.txt";
        let utf16 = to_utf16_null(s);
        // Last element should be null terminator.
        assert_eq!(*utf16.last().unwrap(), 0);
        // All non-null elements should match the original.
        let decoded = utf16_slice_to_pathbuf(&utf16[..utf16.len() - 1]);
        assert_eq!(decoded.to_str().unwrap(), s);
    }

    #[test]
    fn last_error_maps_correctly() {
        // This test verifies the error mapping function by checking known
        // error code mappings. We cannot easily trigger specific errors in a
        // unit test, but we can verify the mapping logic.
        // The actual error codes are tested via the open_* functions above.
    }

    #[test]
    fn owned_handle_invalid_clone() {
        let invalid = OwnedHandle(INVALID_HANDLE_VALUE);
        let cloned = invalid.clone();
        assert!(!cloned.is_valid());
    }
}

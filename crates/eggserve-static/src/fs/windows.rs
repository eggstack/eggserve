#![cfg(windows)]
#![allow(dead_code)]
// This module is the single Windows FFI boundary for handle-relative
// confinement. Every unsafe block below has a local SAFETY comment and the
// module-level exception keeps the workspace deny policy effective elsewhere.
#![allow(unsafe_code)]

//! Windows handle-relative filesystem confinement.
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
//! 5. Retains directory handles in `ResolvedDirectory` for handle-relative
//!    child resolution (index lookup, nested traversal).
//! 6. Queries file identity from the opened handle for diagnostics and root
//!    identity verification.
//! 7. Streams from the final validated handle by converting to `std::fs::File`.
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
//! Directory enumeration uses `NtQueryDirectoryFile` with
//! `FileIdBothDirectoryInfo` for true handle-based enumeration (Plan 085).
//! A legacy path-based fallback (`GetFinalPathNameByHandleW` +
//! `FindFirstFileW`) is retained as `enumerate_directory_path_based` for
//! compatibility profiles.

use std::ffi::c_void;
use std::os::windows::io::{FromRawHandle, RawHandle};
use std::path::PathBuf;
use std::ptr;

use eggserve_primitives::policy::{DotfilePolicy, SymlinkPolicy};

use super::dirbuf::{
    parse_directory_buffer, DirectoryEntryKind, DirectoryEntryRecord, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_ID_BOTH_DIR_INFO_FILE_ID_OFFSET,
    FILE_ID_BOTH_DIR_INFO_HEADER_SIZE,
};

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
const FILE_GENERIC_READ: DWORD = 0x00120089;
const FILE_LIST_DIRECTORY: DWORD = 0x00000001;
const FILE_READ_DATA: DWORD = 0x0001;
const FILE_READ_ATTRIBUTES: DWORD = 0x0080;
const FILE_READ_EA: DWORD = 0x0008;
const READ_CONTROL: DWORD = 0x00020000;

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
//
// The NTFS attribute bits are `FILE_ID_BOTH_DIR_INFO` wire-format values, not
// Windows API bindings; they live with the platform-neutral record parser in
// `fs::dirbuf` so the parser and this module share one definition.

// ── Reparse tag values ──────────────────────────────────────────────────────

const IO_REPARSE_TAG_SYMLINK: u32 = 0xA0000000;
const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA0000003;

// ── Win32 error codes ──────────────────────────────────────────────────────

const ERROR_FILE_NOT_FOUND: DWORD = 2;
const ERROR_PATH_NOT_FOUND: DWORD = 3;
const ERROR_ACCESS_DENIED: DWORD = 5;
const ERROR_INVALID_HANDLE: DWORD = 6;
const ERROR_NOT_A_DIRECTORY: DWORD = 267;
const ERROR_TOO_MANY_LINKS: DWORD = 1142;

// ── File information classes ────────────────────────────────────────────────

const FILE_ATTRIBUTE_TAG_INFO_CLASS: u32 = 9;
const FILE_STANDARD_INFO_CLASS: u32 = 1;
// `FileIdBothDirectoryInformation` in the NT `FILE_INFORMATION_CLASS` enum
// (`ntifs.h`), which is the numbering `NtQueryDirectoryFile` uses. The
// value is 37 there — the user-mode `winnt.h` list numbers the same member
// 16, and the two headers disagree. `windows-sys` 0.61.2 and `ntapi` 0.4.3
// both declare 37, matching `ntifs.h`.
//
// The class selects the `FILE_ID_BOTH_DIR_INFORMATION` record layout, whose
// field offsets are declared once in `fs::dirbuf`.
const FILE_ID_BOTH_DIRECTORY_INFO: u32 = 37;

// `FileIdBothDirectoryInfo` in the user-mode `FILE_INFO_BY_HANDLE_CLASS`
// enum (`winnt.h`) — the class `GetFileInformationByHandleEx` expects. This
// is a different enum from the NT `FILE_INFORMATION_CLASS` above, and the
// two members share a name without sharing a value: this one is 10, not 37.
// It selects the same `FILE_ID_BOTH_DIR_INFORMATION` record layout.
const FILE_ID_BOTH_DIRECTORY_INFO_CLASS: u32 = 10;

const STATUS_NO_MORE_FILES: u32 = 0x80000006;

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
    delete_pending: u8,
    directory: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
#[allow(dead_code)]
struct WIN32_FIND_DATAW {
    dw_file_attributes: DWORD,
    ft_creation_time: [u32; 2],
    ft_last_access_time: [u32; 2],
    ft_last_write_time: [u32; 2],
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
            ft_creation_time: [0; 2],
            ft_last_access_time: [0; 2],
            ft_last_write_time: [0; 2],
            n_file_size_high: 0,
            n_file_size_low: 0,
            dw_reserved0: 0,
            dw_reserved1: 0,
            c_file_name: [0; 260],
            c_alternate_file_name: [0; 14],
        }
    }
}

// ── FILE_ID_BOTH_DIR_INFO record layout ─────────────────────────────────────
//
// The `FILE_ID_BOTH_DIR_INFO` record is a fixed wire format: the NT layout
// table (every field and its byte offset) is documented once, with the
// parser, in `fs::dirbuf`. This module does not declare a second copy of the
// structure — it reads the record as raw bytes at the shared offsets, so the
// FFI boundary and the parser cannot drift apart.
//
// The `NextEntryOffset` field and the `FileAttributes` bits the parser reads
// live with the parser for the same reason.

// ── NT API types (for handle-relative opens) ─────────────────────────────────

#[allow(clippy::upper_case_acronyms)]
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
const FILE_DIRECTORY_FILE: u32 = 0x00000001;
const FILE_NON_DIRECTORY_FILE: u32 = 0x00000040;
const FILE_OPEN_FOR_BACKUP_INTENT: u32 = 0x00004000;
const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x00000020;
const FILE_OPEN_REPARSE_POINT: u32 = 0x00200000;

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

    fn NtQueryDirectoryFile(
        file_handle: HANDLE,
        event: HANDLE,
        apc_routine: *mut c_void,
        apc_context: *mut c_void,
        io_status_block: *mut IoStatusBlock,
        file_information: *mut c_void,
        length: DWORD,
        file_information_class: u32,
        return_single_entry: BOOL,
        file_name: *const NtUnicodeString,
        restart_scan: BOOL,
    ) -> NTSTATUS;
}

// ── OwnedHandle RAII wrapper ────────────────────────────────────────────────

/// RAII wrapper for a Windows `HANDLE`.
///
/// Guarantees exactly-once `CloseHandle` on drop. Handles are duplicated
/// via `try_clone()` rather than `Clone` — duplication may fail due to
/// handle-quota exhaustion, and the error must be propagated.
///
/// # Safety invariants
///
/// - `0` and `INVALID_HANDLE_VALUE` are treated as invalid and are not closed.
/// - Borrowed handles (e.g., the raw root handle from `PinnedRoot`) must
///   never be wrapped in `OwnedHandle` — doing so would cause a double-close.
pub(crate) struct OwnedHandle(HANDLE);

impl std::fmt::Debug for OwnedHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("OwnedHandle").finish()
    }
}

impl OwnedHandle {
    /// Wraps a raw `HANDLE` as an owned value.
    ///
    /// # Safety contract
    ///
    /// The caller must ensure:
    /// - The handle is valid (not null, not `INVALID_HANDLE_VALUE`).
    /// - The handle was returned by a Windows API that transfers ownership.
    /// - The handle will not be closed by any other code path.
    pub(crate) unsafe fn from_raw(handle: HANDLE) -> Self {
        Self(handle)
    }

    /// Returns `true` if the handle is not null and not `INVALID_HANDLE_VALUE`.
    pub(crate) fn is_valid(&self) -> bool {
        !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE
    }

    /// Returns the raw `HANDLE` value.
    pub(crate) fn raw(&self) -> HANDLE {
        self.0
    }

    /// Duplicates this handle via `DuplicateHandle`, creating an independent
    /// owned copy with the same access rights.
    ///
    /// # Errors
    ///
    /// Returns `WindowsFsError::IoError` if `DuplicateHandle` fails (e.g.,
    /// handle-quota exhaustion or out of memory).
    pub(crate) fn try_clone(&self) -> Result<Self, WindowsFsError> {
        if !self.is_valid() {
            return Err(WindowsFsError::IoError(ERROR_INVALID_HANDLE));
        }
        let mut new_handle = INVALID_HANDLE_VALUE;
        // SAFETY: GetCurrentProcess() returns a pseudohandle valid for
        // DuplicateHandle. DUPLICATE_SAME_ACCESS copies access rights.
        // The output pointer is stack-allocated.
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
            // SAFETY: `GetLastError` reads thread-local Win32 error state
            // and imposes no preconditions on the caller. The failed
            // `DuplicateHandle` call above is what set the value read here.
            return Err(WindowsFsError::IoError(unsafe { GetLastError() }));
        }
        Ok(Self(new_handle))
    }
}

// SAFETY: Windows HANDLEs are process-global integers. They are safe to
// transfer between threads (Send) and share across threads (Sync). The
// underlying OS ensures thread-safe access to handle operations.
unsafe impl Send for OwnedHandle {}
// SAFETY: sharing `OwnedHandle` across threads is safe for the same reason:
// the wrapped handle is a process-global integer, and every operation on it —
// including the exactly-once `CloseHandle` on drop — is performed by the OS
// under its own synchronization, so no thread observes a torn or
// double-closed handle.
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

// ── Error type ──────────────────────────────────────────────────────────────

/// Filesystem error type for Windows handle-relative operations.
///
/// Each variant maps to a stable internal event category used for
/// diagnostics and observability. The categories are:
///
/// - `NotFound` — **child-open not found**: the target file or directory
///   does not exist at the requested path, or the NTSTATUS code indicates
///   `STATUS_NO_SUCH_FILE` / `STATUS_OBJECT_NAME_NOT_FOUND`.
/// - `NotADirectory` — **invalid namespace/component**: an intermediate
///   path component was expected to be a directory but was a file, or the
///   NTSTATUS code indicates `STATUS_NOT_A_DIRECTORY` /
///   `STATUS_FILE_IS_A_DIRECTORY`.
/// - `AccessDenied` — **child access denied**: the handle lacks permission
///   to open the target, or the NTSTATUS code indicates
///   `STATUS_ACCESS_DENIED`.
/// - `TooManyLinks` — **root-open failure**: the file system has too many
///   hard links for the target (maps from Win32 `ERROR_TOO_MANY_LINKS`).
/// - `ReparsePointDenied` — **reparse denied**: the target is a reparse
///   point (symlink, junction, mount point) and the hardened policy requires
///   all reparse points to be denied.
/// - `ParseError(DirBufParseError)` — **malformed directory buffer**: the
///   kernel returned a directory-information buffer that is not a well-formed
///   `FILE_ID_BOTH_DIR_INFORMATION` record chain, so the parser rejected it.
///   The underlying reason is carried rather than collapsed into an I/O code.
/// - `IoError(DWORD)` — **unexpected Windows status/error code**: a Win32
///   error code or NTSTATUS value that does not map to a specific variant.
///   The contained `DWORD` is the raw error code for diagnostics.
#[derive(Debug)]
pub(crate) enum WindowsFsError {
    NotFound,
    NotADirectory,
    AccessDenied,
    TooManyLinks,
    ReparsePointDenied,
    /// The component name's UTF-16 encoding exceeds the 16-bit byte-length
    /// fields of `UNICODE_STRING` (64 KiB).
    NameTooLong,
    /// The kernel's directory-information buffer could not be parsed.
    ///
    /// This is a distinct category from a Win32 or NTSTATUS code: the bytes
    /// arrived from the kernel but did not describe a well-formed record, so
    /// the underlying reason is carried rather than collapsed into a generic
    /// I/O error.
    ParseError(super::dirbuf::DirBufParseError),
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
            Self::NameTooLong => write!(f, "name too long"),
            Self::ParseError(error) => write!(f, "directory buffer parse error: {error:?}"),
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
///
/// Trims trailing NUL bytes that are padding in fixed-size Windows arrays
/// like `WIN32_FIND_DATAW.c_file_name`.
fn utf16_slice_to_pathbuf(slice: &[u16]) -> PathBuf {
    let end = slice.iter().position(|&c| c == 0).unwrap_or(slice.len());
    String::from_utf16_lossy(&slice[..end]).into()
}

// ── Track B: Root-relative open functions ────────────────────────────────────

/// Builds an [`NtUnicodeString`] over `name`, including its NUL terminator.
///
/// Returns the string struct and its backing buffer; the caller must keep
/// both alive for the duration of the native call. `length` covers the
/// name's UTF-16 bytes only, while `maximum_length` additionally includes
/// the two-byte NUL terminator carried by the buffer.
///
/// Returns [`WindowsFsError::NameTooLong`] instead of silently truncating
/// when the encoded name exceeds the 16-bit byte-length fields NT uses.
fn build_nt_unicode_string(name: &str) -> Result<(NtUnicodeString, Vec<u16>), WindowsFsError> {
    let buffer = to_utf16_null(name);
    let utf16_byte_len = buffer
        .len()
        .checked_mul(2)
        .ok_or(WindowsFsError::NameTooLong)?;
    if utf16_byte_len > u16::MAX as usize {
        return Err(WindowsFsError::NameTooLong);
    }
    let utf16_byte_len = utf16_byte_len as u16;
    Ok((
        NtUnicodeString {
            length: utf16_byte_len - 2,
            maximum_length: utf16_byte_len,
            buffer: buffer.as_ptr() as *mut u16,
        },
        buffer,
    ))
}

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
/// `WindowsFsError::AccessDenied` if the handle lacks permission,
/// `WindowsFsError::NameTooLong` if the name cannot fit NT's 16-bit
/// length fields, or `WindowsFsError::IoError` for other failures.
pub(crate) fn open_directory_relative(
    parent: HANDLE,
    name: &str,
) -> Result<OwnedHandle, WindowsFsError> {
    debug_assert!(!name.is_empty(), "child name must not be empty");
    let (mut obj_name, _name_buf) = build_nt_unicode_string(name)?;
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
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            &mut obj_attr,
            &mut iosb,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_DIRECTORY_FILE
                | FILE_SYNCHRONOUS_IO_NONALERT
                | FILE_OPEN_FOR_BACKUP_INTENT
                | FILE_OPEN_REPARSE_POINT,
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
/// `WindowsFsError::NotADirectory` if the target is a directory,
/// `WindowsFsError::NameTooLong` if the name cannot fit NT's 16-bit
/// length fields, or `WindowsFsError::IoError` for other failures.
pub(crate) fn open_file_relative(
    parent: HANDLE,
    name: &str,
) -> Result<OwnedHandle, WindowsFsError> {
    debug_assert!(!name.is_empty(), "child name must not be empty");
    let (mut obj_name, _name_buf) = build_nt_unicode_string(name)?;
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
            FILE_READ_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            &mut obj_attr,
            &mut iosb,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_NON_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
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
/// Duplicates a raw handle via `DuplicateHandle`, returning a new owned handle.
///
/// This is used when we need to create an `OwnedHandle` from a borrowed raw
/// handle (e.g., the root handle from `PinnedRoot`).
fn duplicate_raw_handle(source: HANDLE) -> Result<OwnedHandle, WindowsFsError> {
    let mut new_handle = INVALID_HANDLE_VALUE;
    // SAFETY: `GetCurrentProcess` returns a pseudohandle that is valid for
    // `DuplicateHandle` in this process. `source` is a live handle borrowed
    // from the caller for the duration of the call; `DuplicateHandle` reads
    // it but does not consume or close it. `&mut new_handle` is a
    // stack-allocated, properly aligned output pointer that is valid for
    // the write. `DUPLICATE_SAME_ACCESS` copies the source access rights
    // onto the duplicate.
    let ok = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            source,
            GetCurrentProcess(),
            &mut new_handle,
            0,
            FALSE,
            DUPLICATE_SAME_ACCESS,
        )
    };
    if ok == 0 {
        // SAFETY: `GetLastError` reads thread-local Win32 error state and
        // imposes no preconditions on the caller. The failed
        // `DuplicateHandle` call above is what set the value read here.
        return Err(WindowsFsError::IoError(unsafe { GetLastError() }));
    }
    // SAFETY: `DuplicateHandle` succeeded, so `new_handle` is a fresh, valid
    // handle that no other code path owns or closes. `from_raw` transfers
    // that ownership to `OwnedHandle`, whose `Drop` closes it exactly once;
    // `new_handle` is a local that is never closed directly.
    Ok(unsafe { OwnedHandle::from_raw(new_handle) })
}

pub(crate) fn resolve_components_relative(
    root: HANDLE,
    components: &[String],
    deny_reparse: bool,
) -> Result<OwnedHandle, WindowsFsError> {
    if components.is_empty() {
        // Caller wants the root handle itself. Duplicate it to return an
        // owned copy.
        return duplicate_raw_handle(root);
    }

    // Track the current directory as a raw handle. Intermediate OwnedHandles
    // are kept alive in `intermediates` to ensure parent handles outlive any
    // child handles opened from them.
    let mut current_raw = root;
    let mut intermediates: Vec<OwnedHandle> = Vec::new();
    let total = components.len();

    for (i, component) in components.iter().enumerate() {
        let is_final = i == total - 1;

        // Open the component relative to the current handle.
        let child = if is_final {
            open_file_relative(current_raw, component)?
        } else {
            open_directory_relative(current_raw, component)?
        };

        // Validate intermediate components are directories.
        if !is_final {
            let info = get_file_standard_info(child.raw())?;
            if info.directory == 0 {
                return Err(WindowsFsError::NotADirectory);
            }
        }

        // Check for reparse points if policy requires denial.
        if deny_reparse {
            deny_all_reparse_check(child.raw())?;
        }

        if is_final {
            return Ok(child);
        }

        // Intermediate: keep alive and update the raw pointer.
        current_raw = child.raw();
        intermediates.push(child);
    }

    Err(WindowsFsError::NotFound)
}

/// Opens a file or directory relative to a parent directory handle.
///
/// Tries `open_file_relative` first (non-directory). If that fails with
/// `NotADirectory`, tries `open_directory_relative`. This avoids needing a
/// single NtOpenFile CreateOptions value that works for both, since
/// `FILE_SYNCHRONOUS_IO_NONALERT` (0x20) collides with `FILE_DIRECTORY_FILE`.
///
/// This is used by `RootGuard::resolve` when the final component type is
/// unknown (could be a file or directory).
pub(crate) fn open_any_relative(parent: HANDLE, name: &str) -> Result<OwnedHandle, WindowsFsError> {
    match open_file_relative(parent, name) {
        Ok(h) => return Ok(h),
        Err(WindowsFsError::NotADirectory) => {}
        Err(e) => return Err(e),
    }
    open_directory_relative(parent, name)
}

/// Resolves a sequence of path components relative to a root handle,
/// returning a `ResolvedResource` (file or directory).
///
/// This is the Windows equivalent of Unix `resolve_fd_relative`. It opens
/// each component relative to the previous handle, checking for reparse
/// points when `deny_reparse` is true. The final component is opened as
/// either a file or directory and the type is determined from metadata.
///
/// When `dotfiles_denied` is true, components starting with `.` are rejected
/// with `DotfileDenied`, matching the Unix behavior.
/// Resolves a sequence of path components relative to a root handle,
/// returning a `ResolvedResource` (file or directory).
///
/// This is the Windows equivalent of Unix `resolve_fd_relative`. It opens
/// each component relative to the previous handle, checking for reparse
/// points when `deny_reparse` is true. The final component is opened as
/// either a file or directory and the type is determined from metadata.
///
/// When `dotfiles_denied` is true, components starting with `.` are rejected
/// with `DotfileDenied`, matching the Unix behavior.
///
/// # Hardened mode invariant
///
/// When `deny_reparse` is `true` (hardened mode), this function must **never**
/// call [`super::RootGuard::resolve_fallback`]. All resolution is performed
/// through handle-relative opens; no path-based fallback is used. The caller
/// in `RootGuard::resolve` is responsible for dispatching to this function
/// only when `SymlinkPolicy::Denied` is in effect.
pub(crate) fn resolve_to_resource(
    root: HANDLE,
    canonical_root: &std::path::Path,
    components: &[String],
    deny_reparse: bool,
    dotfiles_denied: bool,
    reject_backslash: bool,
) -> super::ResolvedResource {
    use super::{ResolvedDirectory, ResolvedFile, ResolvedResource};

    debug_assert!(
        !root.is_null() && root != INVALID_HANDLE_VALUE,
        "root handle must be valid"
    );

    if components.is_empty() {
        // Root path — return the root directory itself, retaining a handle.
        let dir_handle = match duplicate_raw_handle(root) {
            Ok(h) => h,
            Err(error) => return ResolvedResource::IoError(std::io::Error::other(error)),
        };
        return ResolvedResource::Directory(ResolvedDirectory {
            #[cfg(windows)]
            dir_handle,
            canonical_path: canonical_root.to_path_buf(),
            components: Vec::new(),
            reject_backslash,
        });
    }

    // Track ownership: `current` holds the OwnedHandle for the current
    // directory level. We extract the raw pointer via `raw()` for the next
    // open call. We never create an OwnedHandle from the root handle to
    // avoid closing it when the function returns.
    let mut current: Option<OwnedHandle> = None;
    let total = components.len();

    for (i, component) in components.iter().enumerate() {
        let is_final = i == total - 1;
        let parent_raw = current.as_ref().map_or(root, |h| h.raw());

        // Check dotfile policy before opening the component.
        if dotfiles_denied && component.starts_with('.') {
            return ResolvedResource::Denied(crate::path::PathRejection::DotfileDenied);
        }

        let child = if is_final {
            open_any_relative(parent_raw, component)
        } else {
            open_directory_relative(parent_raw, component)
        };

        let child = match child {
            Ok(h) => h,
            Err(_) => return ResolvedResource::NotFound,
        };

        // Validate intermediate components are directories.
        if !is_final {
            match get_file_standard_info(child.raw()) {
                Ok(info) if info.directory == 0 => {
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

        // 8.3 alias defense: a requested component that syntactically
        // resembles an NTFS short-name alias may resolve to a long name
        // that begins with `.` (e.g., `ENV~1` → `.env`). Re-check the
        // resolved long name against the dotfile policy before use.
        if dotfiles_denied && super::may_be_short_name_alias(component) {
            match get_final_path(child.raw()) {
                Ok(final_path) => {
                    if final_path
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy().starts_with('.'))
                    {
                        return ResolvedResource::Denied(crate::path::PathRejection::DotfileDenied);
                    }
                }
                // Fail closed when the long name cannot be confirmed.
                Err(_) => return ResolvedResource::NotFound,
            }
        }

        if is_final {
            // Determine if this is a file or directory.
            let is_dir = match get_file_standard_info(child.raw()) {
                Ok(info) => info.directory != 0,
                Err(_) => return ResolvedResource::NotFound,
            };

            let canonical_path = match get_final_path(child.raw()) {
                Ok(p) => p,
                // Diagnostic fallback only: join every component traversed
                // so far, not just the final one.
                Err(_) => {
                    let mut constructed = canonical_root.to_path_buf();
                    for traversed in &components[..=i] {
                        constructed.push(traversed);
                    }
                    constructed
                }
            };

            let safe_components = components.to_vec();

            if is_dir {
                // Retain the directory handle in ResolvedDirectory for
                // handle-relative child resolution.
                return ResolvedResource::Directory(ResolvedDirectory {
                    #[cfg(windows)]
                    dir_handle: child,
                    canonical_path,
                    components: safe_components,
                    reject_backslash,
                });
            } else {
                // Duplicate the handle for the std::fs::File, preserving the
                // original OwnedHandle for potential intermediate use.
                let file_handle = match child.try_clone() {
                    Ok(h) => h,
                    Err(error) => return ResolvedResource::IoError(std::io::Error::other(error)),
                };
                let std_file = match handle_to_std_file(file_handle) {
                    Ok(file) => file,
                    Err(_) => return ResolvedResource::NotFound,
                };
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

        // Intermediate component: transfer ownership to `current`.
        // The previous OwnedHandle (if any) is dropped here, closing the
        // previous directory handle. The new child handle is kept alive
        // for the next iteration.
        current = Some(child);
    }

    ResolvedResource::NotFound
}

// ── Track C: Child resolution and directory listing ─────────────────────────

/// Resolves a single child component relative to a retained directory handle.
///
/// This is the Windows equivalent of `unix::resolve_child_fd`. It opens the
/// child relative to the parent directory handle using NtOpenFile, checks for
/// reparse points, and returns a `ResolvedResource`.
///
/// The child must have been validated by `validate_child_component` before
/// calling this function (no empty, `.`, `..`, NUL, or separator characters).
///
/// # Arguments
///
/// * `parent_handle` - Handle to the parent directory (from `ResolvedDirectory`).
/// * `parent_components` - The parent's logical component path (for building the
///   child's `safe_relative_components`).
/// * `child` - The child component name.
/// * `deny_reparse` - If `true`, reject the child if it is a reparse point.
/// * `dotfiles_denied` - If `true`, reject children starting with `.`.
///
/// # Errors
///
/// Returns `ResolvedResource::NotFound` if the child does not exist or cannot
/// be opened, `ResolvedResource::Denied` for policy violations.
pub(crate) fn resolve_child_relative(
    parent_handle: HANDLE,
    parent_components: &[String],
    parent_path: &std::path::Path,
    child: &str,
    deny_reparse: bool,
    dotfiles_denied: bool,
    reject_backslash: bool,
) -> super::ResolvedResource {
    use super::{ResolvedDirectory, ResolvedFile, ResolvedResource};

    debug_assert!(
        !parent_handle.is_null() && parent_handle != INVALID_HANDLE_VALUE,
        "parent handle must be valid"
    );

    if dotfiles_denied && child.starts_with('.') {
        return ResolvedResource::Denied(crate::path::PathRejection::DotfileDenied);
    }

    // Try opening as a file first, then as a directory.
    let child_handle = match open_file_relative(parent_handle, child) {
        Ok(h) => h,
        Err(WindowsFsError::NotADirectory) => {
            // Not a file — try as directory.
            match open_directory_relative(parent_handle, child) {
                Ok(h) => h,
                Err(_) => return ResolvedResource::NotFound,
            }
        }
        Err(_) => return ResolvedResource::NotFound,
    };

    // Check for reparse points if policy requires denial.
    if deny_reparse {
        match deny_all_reparse_check(child_handle.raw()) {
            Ok(()) => {}
            Err(WindowsFsError::ReparsePointDenied) => {
                return ResolvedResource::Denied(crate::path::PathRejection::SymlinkDenied);
            }
            Err(_) => return ResolvedResource::NotFound,
        }
    }

    // 8.3 alias defense (see resolve_to_resource): a child that resembles
    // an NTFS short-name alias is re-checked against the dotfile policy
    // using its resolved long name.
    if dotfiles_denied && super::may_be_short_name_alias(child) {
        match get_final_path(child_handle.raw()) {
            Ok(final_path) => {
                if final_path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with('.'))
                {
                    return ResolvedResource::Denied(crate::path::PathRejection::DotfileDenied);
                }
            }
            // Fail closed when the long name cannot be confirmed.
            Err(_) => return ResolvedResource::NotFound,
        }
    }

    // Determine if this is a file or directory.
    let is_dir = match get_file_standard_info(child_handle.raw()) {
        Ok(info) => info.directory != 0,
        Err(_) => return ResolvedResource::NotFound,
    };

    let mut components = parent_components.to_vec();
    components.push(child.to_string());

    if is_dir {
        ResolvedResource::Directory(ResolvedDirectory {
            #[cfg(windows)]
            dir_handle: child_handle,
            canonical_path: parent_path.join(child),
            components,
            reject_backslash,
        })
    } else {
        let std_file = match handle_to_std_file(child_handle) {
            Ok(file) => file,
            Err(_) => return ResolvedResource::NotFound,
        };
        let metadata = match std_file.metadata() {
            Ok(m) => m,
            Err(_) => return ResolvedResource::NotFound,
        };
        ResolvedResource::File(ResolvedFile {
            file: std_file,
            metadata,
            safe_relative_components: components,
        })
    }
}

/// Enumerates directory contents with policy filtering.
///
/// Returns entries filtered according to the dotfile and symlink policies.
/// This is the Windows handle-relative equivalent of `unix::list_directory_fd`.
///
/// Entries are enumerated from the directory handle using `NtQueryDirectoryFile`
/// (no path reconstruction). Policy is applied before rendering.
pub(crate) fn list_directory_handle(
    dir_handle: HANDLE,
    policy: &eggserve_primitives::policy::StaticPolicy,
    max_entries: usize,
) -> Result<Vec<super::ListingEntry>, std::io::Error> {
    let entries = enumerate_directory(dir_handle, max_entries).map_err(std::io::Error::other)?;

    let mut result = Vec::new();
    for entry in entries {
        // Filter dotfiles.
        if policy.dotfiles == DotfilePolicy::Denied && entry.hidden_or_dot {
            continue;
        }

        // Filter reparse points when symlinks are denied.
        if policy.symlinks == SymlinkPolicy::Denied
            && entry.kind == DirectoryEntryKind::ReparsePoint
        {
            continue;
        }

        // Skip unsupported object classes (Other kind).
        if entry.kind == DirectoryEntryKind::Other {
            continue;
        }

        let is_dir = entry.kind == DirectoryEntryKind::Directory;
        result.push(super::ListingEntry::from_name(entry.name, is_dir));
    }

    result.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}

// ── Track D: Reparse detection ──────────────────────────────────────────────

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
            FILE_LIST_DIRECTORY | SYNCHRONIZE,
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
/// to retrieve the file ID.
///
/// The returned `u64` is the eight-byte `FileId` field of the
/// `FILE_ID_BOTH_DIR_INFO` record, read at the shared wire offset
/// (`FILE_ID_BOTH_DIR_INFO_FILE_ID_OFFSET`: byte 96 of the record's fixed
/// 104-byte header). This is stable across renames within the same volume.
///
/// # Errors
///
/// Returns `WindowsFsError::IoError` if the file ID query fails.
pub(crate) fn get_file_id(handle: HANDLE) -> Result<u64, WindowsFsError> {
    // FILE_ID_BOTH_DIR_INFO is variable-length: a fixed
    // `FILE_ID_BOTH_DIR_INFO_HEADER_SIZE`-byte header followed by a UTF-16
    // file name. The buffer therefore holds the shared header constant plus
    // room for the longest name NTFS allows (255 UTF-16 code units) and one
    // spare code unit. Sizing it from the shared constant keeps the FFI
    // boundary and the parser on the same layout.
    const MAX_NAME_UTF16_UNITS: usize = 256;
    const BUFFER_SIZE: usize = FILE_ID_BOTH_DIR_INFO_HEADER_SIZE + MAX_NAME_UTF16_UNITS * 2;
    let mut buffer = vec![0u8; BUFFER_SIZE];

    // SAFETY: `buffer` is a heap-allocated, zeroed byte buffer of
    // `BUFFER_SIZE` bytes, which is large enough for the record the call
    // writes (the fixed header plus a UTF-16 name of up to 255 code units).
    // The handle is valid (caller contract). `GetFileInformationByHandleEx`
    // with class 10 writes a FILE_ID_BOTH_DIR_INFO record — a fixed header
    // followed by the file name — into the buffer.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FILE_ID_BOTH_DIRECTORY_INFO_CLASS,
            buffer.as_mut_ptr() as *mut c_void,
            BUFFER_SIZE as DWORD,
        )
    };

    if ok == 0 {
        return Err(last_error_to_fs_error());
    }

    // The `FileId` field is the eight bytes at the shared wire offset. The
    // buffer is sized from the shared header constant, so the field is
    // always in bounds; the const assertion states that at compile time
    // instead of leaving it to a runtime check that could never fail.
    const FILE_ID_END: usize = FILE_ID_BOTH_DIR_INFO_FILE_ID_OFFSET + 8;
    const _: () = assert!(
        FILE_ID_END <= BUFFER_SIZE,
        "the file-id field must fit inside the record buffer"
    );
    let file_id_bytes: [u8; 8] = buffer[FILE_ID_BOTH_DIR_INFO_FILE_ID_OFFSET..FILE_ID_END]
        .try_into()
        .expect("the file-id slice is exactly eight bytes wide");

    Ok(u64::from_le_bytes(file_id_bytes))
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
/// # Errors
///
/// Returns an I/O error when the handle is null or
/// `INVALID_HANDLE_VALUE`.
pub(crate) fn handle_to_std_file(handle: OwnedHandle) -> std::io::Result<std::fs::File> {
    if !handle.is_valid() {
        return Err(std::io::Error::from_raw_os_error(
            ERROR_INVALID_HANDLE as i32,
        ));
    }

    let raw = handle.raw();

    // SAFETY: we asserted the handle is valid. `from_raw_handle` takes
    // ownership of the handle and will call CloseHandle when the File is
    // dropped. We must prevent the OwnedHandle from also calling CloseHandle.
    std::mem::forget(handle);

    // SAFETY: the raw handle is valid for read access (GENERIC_READ or
    // FILE_LIST_DIRECTORY). `from_raw_handle` is safe to call on any valid
    // Windows HANDLE that was opened with appropriate access rights.
    Ok(unsafe { std::fs::File::from_raw_handle(raw as RawHandle) })
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

/// Default buffer size for NtQueryDirectoryFile (64 KiB).
/// This is large enough for most directories without reallocation.
const DIR_ENUM_BUFFER_SIZE: usize = 64 * 1024;

/// A single directory entry returned by the path-based `enumerate_directory_path_based`.
#[derive(Debug)]
pub(crate) struct DirectoryEntryPathBased {
    pub name: String,
    pub is_directory: bool,
    pub is_reparse_point: bool,
    pub file_size: u64,
}

/// Enumerates the contents of a directory using path-based fallback.
///
/// # Implementation note
///
/// This uses `GetFinalPathNameByHandleW` to reconstruct the absolute path,
/// then `FindFirstFileW` / `FindNextFileW` to enumerate entries. This is the
/// legacy path-based approach, retained for compatibility profiles.
///
/// For handle-based enumeration (no path reconstruction), use the primary
/// `enumerate_directory` function which uses `NtQueryDirectoryFile`.
///
/// # Filtering
///
/// Entries named `.` and `..` are excluded from the result.
///
/// # Errors
///
/// Returns `WindowsFsError::IoError` if the path reconstruction or
/// `FindFirstFileW` / `FindNextFileW` calls fail.
pub(crate) fn enumerate_directory_path_based(
    handle: HANDLE,
) -> Result<Vec<DirectoryEntryPathBased>, WindowsFsError> {
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
            entries.push(DirectoryEntryPathBased {
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

/// Enumerates the contents of a directory using the given handle.
///
/// Uses `NtQueryDirectoryFile` with `FileIdBothDirectoryInfo` for true
/// handle-based enumeration. No path reconstruction is performed.
///
/// For directories that exceed the buffer size, multiple calls are made
/// with `restart_scan=FALSE` to continue enumeration.
///
/// # Filtering
///
/// Entries named `.` and `..` are excluded from the result.
///
/// # Arguments
///
/// * `handle` - Directory handle opened with `FILE_LIST_DIRECTORY | SYNCHRONIZE`.
/// * `max_entries` - Maximum number of entries to enumerate (from configuration).
///
/// # Errors
///
/// Returns `WindowsFsError::IoError` if the `NtQueryDirectoryFile` call fails.
pub(crate) fn enumerate_directory(
    handle: HANDLE,
    max_entries: usize,
) -> Result<Vec<DirectoryEntryRecord>, WindowsFsError> {
    let mut buffer = vec![0u8; DIR_ENUM_BUFFER_SIZE];
    let mut all_entries = Vec::new();
    let mut first_call = true;

    loop {
        if all_entries.len() >= max_entries {
            break;
        }

        let mut io_status = IoStatusBlock {
            status: 0,
            information: 0,
        };

        let restart_scan = if first_call { TRUE } else { FALSE };

        // SAFETY: `buffer` is a live heap byte buffer of
        // DIR_ENUM_BUFFER_SIZE bytes, and `Length` is passed as the same
        // size, so the kernel cannot write past its end. `io_status` is a
        // live stack output parameter. `handle` is valid (caller contract,
        // opened with FILE_LIST_DIRECTORY | SYNCHRONIZE and without
        // FILE_FLAG_OVERLAPPED, so the call is synchronous and the null
        // event/APC arguments are the documented way to ask for it).
        // `NtQueryDirectoryFile` writes `FileIdBothDirectoryInformation`
        // records — the `FILE_ID_BOTH_DIR_INFORMATION` layout whose field
        // offsets `fs::dirbuf` declares — into `buffer`; only the first
        // `io_status.information` bytes are read back, so bytes the kernel
        // leaves untouched from a previous call are never interpreted. All
        // optional pointer parameters are null (no event, no APC, no file
        // name filter).
        let status = unsafe {
            NtQueryDirectoryFile(
                handle,
                ptr::null_mut(), // event
                ptr::null_mut(), // apc_routine
                ptr::null_mut(), // apc_context
                &mut io_status,
                buffer.as_mut_ptr() as *mut c_void,
                DIR_ENUM_BUFFER_SIZE as DWORD,
                FILE_ID_BOTH_DIRECTORY_INFO,
                FALSE,       // return_single_entry
                ptr::null(), // file_name (null = all entries)
                restart_scan,
            )
        };

        first_call = false;

        if status as u32 == STATUS_NO_MORE_FILES {
            break;
        }

        if status < 0 {
            return Err(WindowsFsError::IoError(status as u32));
        }

        let bytes_returned = io_status.information;
        if bytes_returned == 0 || bytes_returned > DIR_ENUM_BUFFER_SIZE {
            break;
        }

        let remaining = max_entries.saturating_sub(all_entries.len());
        let parsed = parse_directory_buffer(&buffer[..bytes_returned], remaining)
            .map_err(WindowsFsError::ParseError)?;

        let count = parsed.len();
        all_entries.extend(parsed);

        // A short response can still contain the final record of a full
        // directory buffer. Continue until Windows reports STATUS_NO_MORE_FILES.
        if count == 0 {
            break;
        }
    }

    Ok(all_entries)
}

// ── PinnedRoot and ResolvedDirectory Windows fields ─────────────────────────
//
// `PinnedRoot` in `fs/mod.rs` carries `root_handle: OwnedHandle` on Windows.
// `ResolvedDirectory` carries `dir_handle: OwnedHandle` on Windows, enabling
// handle-relative child resolution for index lookup and nested traversal.

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_root() -> (TempDir, OwnedHandle) {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join("subdir")).unwrap();
        std::fs::write(root.join("hello.txt"), "hello").unwrap();
        std::fs::write(root.join("subdir").join("file.txt"), "nested").unwrap();

        // Open the root directory handle.
        let root_path_utf16 = to_utf16_null(root.to_str().unwrap());
        // SAFETY: `root_path_utf16` is a null-terminated UTF-16 buffer that
        // stays alive for the call, and `CreateFileW` does not retain the
        // pointer. The two null arguments are the optional
        // security-attributes and template-handle parameters, which
        // `CreateFileW` accepts as absent.
        let handle = unsafe {
            CreateFileW(
                root_path_utf16.as_ptr(),
                FILE_LIST_DIRECTORY | SYNCHRONIZE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        assert_ne!(handle, INVALID_HANDLE_VALUE);
        (tmp, OwnedHandle(handle))
    }

    #[test]
    fn open_directory_relative_succeeds() {
        let (_tmp, root_handle) = setup_test_root();
        let result = open_directory_relative(root_handle.raw(), "subdir");
        assert!(
            result.is_ok(),
            "open_directory_relative failed: {:?}",
            result.err()
        );
    }

    #[test]
    fn open_file_relative_succeeds() {
        let (_tmp, root_handle) = setup_test_root();
        let result = open_file_relative(root_handle.raw(), "hello.txt");
        assert!(
            result.is_ok(),
            "open_file_relative failed: {:?}",
            result.err()
        );
    }

    #[test]
    fn open_relative_not_found() {
        let (_tmp, root_handle) = setup_test_root();
        let result = open_file_relative(root_handle.raw(), "nonexistent.txt");
        assert!(matches!(result, Err(WindowsFsError::NotFound)));
    }

    #[test]
    fn open_directory_as_file_fails() {
        let (_tmp, root_handle) = setup_test_root();
        let result = open_file_relative(root_handle.raw(), "subdir");
        assert!(
            matches!(result, Err(WindowsFsError::NotADirectory)),
            "opening a directory without BACKUP_SEMANTICS should fail with NotADirectory, got {:?}",
            result
        );
    }

    #[test]
    fn resolve_nested_components() {
        let (_tmp, root_handle) = setup_test_root();
        let components: Vec<String> = vec!["subdir".into(), "file.txt".into()];
        let result = resolve_components_relative(root_handle.raw(), &components, true);
        assert!(
            result.is_ok(),
            "resolve_components_relative failed: {:?}",
            result.err()
        );
    }

    #[test]
    fn resolve_intermediate_not_directory_fails() {
        let (_tmp, root_handle) = setup_test_root();
        let components: Vec<String> = vec!["hello.txt".into(), "impossible".into()];
        let result = resolve_components_relative(root_handle.raw(), &components, true);
        assert!(
            matches!(result, Err(WindowsFsError::NotADirectory)),
            "intermediate file should fail with NotADirectory, got {:?}",
            result
        );
    }

    #[test]
    fn get_file_standard_info_directory() {
        let (_tmp, root_handle) = setup_test_root();
        let dir_handle = open_directory_relative(root_handle.raw(), "subdir").unwrap();
        let info = get_file_standard_info(dir_handle.raw());
        assert!(info.is_ok());
        let info = info.unwrap();
        assert_ne!(info.directory, 0);
    }

    #[test]
    fn get_file_standard_info_file() {
        let (_tmp, root_handle) = setup_test_root();
        let file_handle = open_file_relative(root_handle.raw(), "hello.txt").unwrap();
        let info = get_file_standard_info(file_handle.raw());
        assert!(info.is_ok());
        let info = info.unwrap();
        assert_eq!(info.directory, 0);
        assert_eq!(info.end_of_file, 5); // "hello" is 5 bytes
    }

    #[test]
    fn get_final_path_succeeds() {
        let (_tmp, root_handle) = setup_test_root();
        let path = get_final_path(root_handle.raw());
        assert!(path.is_ok(), "get_final_path failed: {:?}", path.err());
        let path = path.unwrap();
        assert!(path.exists(), "final path should exist on disk: {:?}", path);
    }

    #[test]
    fn get_file_id_succeeds() {
        let (_tmp, root_handle) = setup_test_root();
        let id = get_file_id(root_handle.raw());
        assert!(id.is_ok(), "get_file_id failed: {:?}", id.err());
    }

    /// The NT information class the enumeration path depends on.
    ///
    /// `NtQueryDirectoryFile` numbers its `FILE_INFORMATION_CLASS` argument
    /// with the *kernel* (`ntifs.h`) enumeration, where
    /// `FileIdBothDirectoryInformation` is 37. The user-mode `winnt.h` list
    /// numbers the same member 16, and the two headers disagree — a very
    /// plausible thing to "fix" by accident. This test pins the value
    /// against a live kernel: class 37 must be accepted and select the
    /// `FILE_ID_BOTH_DIR_INFORMATION` layout, and class 16 must be rejected
    /// as an invalid parameter.
    #[test]
    fn nt_query_directory_file_uses_the_kernel_information_class() {
        const STATUS_INVALID_PARAMETER: i32 = 0xC0000003u32.cast_signed();
        const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC0000004u32.cast_signed();

        let tmp = TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        std::fs::create_dir_all(root.join("subdir")).expect("create subdir");
        std::fs::write(root.join("hello.txt"), "hello").expect("write hello.txt");
        let root_utf16 = to_utf16_null(root.to_str().unwrap());

        // SAFETY: `root_utf16` is a null-terminated UTF-16 buffer that stays
        // alive for the call and `CreateFileW` does not retain the pointer.
        // The two null arguments are the optional security-attributes and
        // template-handle parameters, which `CreateFileW` accepts as absent.
        let handle = unsafe {
            CreateFileW(
                root_utf16.as_ptr(),
                FILE_LIST_DIRECTORY | SYNCHRONIZE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                ptr::null_mut(),
            )
        };
        assert_ne!(handle, INVALID_HANDLE_VALUE);

        let mut buffer = vec![0u8; DIR_ENUM_BUFFER_SIZE];
        let mut io_status = IoStatusBlock {
            status: 0,
            information: 0,
        };
        // SAFETY: `handle` is a live directory handle opened with
        // `FILE_LIST_DIRECTORY`; `buffer` is a live heap byte buffer of
        // `DIR_ENUM_BUFFER_SIZE` bytes, which is the length passed to the
        // kernel; `io_status` is a live stack output parameter. All optional
        // pointers are null, which requests a synchronous call.
        let status = unsafe {
            NtQueryDirectoryFile(
                handle,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut io_status,
                buffer.as_mut_ptr() as *mut c_void,
                DIR_ENUM_BUFFER_SIZE as DWORD,
                FILE_ID_BOTH_DIRECTORY_INFO,
                FALSE,
                ptr::null(),
                TRUE,
            )
        };
        assert_eq!(
            status, 0,
            "the kernel information class must be accepted, got 0x{:08x}",
            status as u32
        );
        assert!(
            io_status.information > 0,
            "the enumeration must return at least one record"
        );

        // The user-mode numbering of the same member is not a valid NT
        // information class. If this ever stops holding, the two headers
        // have converged and this test's premise needs revisiting.
        let mut io_status = IoStatusBlock {
            status: 0,
            information: 0,
        };
        // SAFETY: as above: `handle` is live, `buffer` is a live heap buffer
        // of the length passed, `io_status` is a live stack output
        // parameter, and all optional pointers are null.
        let status = unsafe {
            NtQueryDirectoryFile(
                handle,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut io_status,
                buffer.as_mut_ptr() as *mut c_void,
                DIR_ENUM_BUFFER_SIZE as DWORD,
                16,
                FALSE,
                ptr::null(),
                TRUE,
            )
        };
        assert_eq!(
            status, STATUS_INVALID_PARAMETER,
            "the user-mode information class numbering must be rejected, got 0x{:08x}",
            status as u32
        );

        // A buffer that cannot hold a whole fixed header is too small.
        let mut small = vec![0u8; FILE_ID_BOTH_DIR_INFO_HEADER_SIZE - 1];
        let mut io_status = IoStatusBlock {
            status: 0,
            information: 0,
        };
        // SAFETY: `small` is a live heap buffer of exactly the length passed
        // to the kernel, `handle` is live, and `io_status` is a live stack
        // output parameter. All optional pointers are null.
        let status = unsafe {
            NtQueryDirectoryFile(
                handle,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut io_status,
                small.as_mut_ptr() as *mut c_void,
                small.len() as DWORD,
                FILE_ID_BOTH_DIRECTORY_INFO,
                FALSE,
                ptr::null(),
                TRUE,
            )
        };
        assert_eq!(
            status, STATUS_INFO_LENGTH_MISMATCH,
            "an undersized buffer must be reported as a length mismatch, got 0x{:08x}",
            status as u32
        );

        // SAFETY: `handle` came from `CreateFileW` above, is still open, and
        // is closed exactly once here — no other code path owns or closes it.
        unsafe {
            CloseHandle(handle);
        }
    }
    #[test]
    fn owned_handle_try_clone_and_drop() {
        let (_tmp, root_handle) = setup_test_root();
        let cloned = root_handle.try_clone().unwrap();
        assert!(cloned.is_valid());
        drop(cloned);
        assert!(root_handle.is_valid());
    }

    #[test]
    fn enumerate_directory_entries() {
        let (_tmp, root_handle) = setup_test_root();
        let entries = enumerate_directory(root_handle.raw(), 4096);
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
    }

    #[test]
    fn enumerate_directory_subdir() {
        let (_tmp, root_handle) = setup_test_root();
        let dir_handle = open_directory_relative(root_handle.raw(), "subdir").unwrap();
        let entries = enumerate_directory(dir_handle.raw(), 4096);
        assert!(entries.is_ok());
        let entries = entries.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "file.txt");
        assert_eq!(entries[0].kind, DirectoryEntryKind::File);
    }

    #[test]
    fn enumerate_directory_path_based_entries() {
        let (_tmp, root_handle) = setup_test_root();
        let entries = enumerate_directory_path_based(root_handle.raw());
        assert!(
            entries.is_ok(),
            "enumerate_directory_path_based failed: {:?}",
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
        assert!(!names.contains(&"."), ". should be filtered");
        assert!(!names.contains(&".."), ".. should be filtered");
    }

    #[test]
    fn no_reparse_on_regular_files() {
        let (_tmp, root_handle) = setup_test_root();
        let file_handle = open_file_relative(root_handle.raw(), "hello.txt").unwrap();
        assert!(!is_reparse_point(file_handle.raw()));
        let tag = get_reparse_tag(file_handle.raw()).unwrap();
        assert_eq!(tag, 0);
        assert!(deny_all_reparse_check(file_handle.raw()).is_ok());
    }

    #[test]
    fn deny_all_reparse_check_on_regular_dir() {
        let (_tmp, root_handle) = setup_test_root();
        let dir_handle = open_directory_relative(root_handle.raw(), "subdir").unwrap();
        assert!(deny_all_reparse_check(dir_handle.raw()).is_ok());
    }

    #[test]
    fn handle_to_std_file_read() {
        let (_tmp, root_handle) = setup_test_root();
        let file_handle = open_file_relative(root_handle.raw(), "hello.txt").unwrap();
        let std_file = handle_to_std_file(file_handle).unwrap();
        let mut contents = String::new();
        std::io::Read::read_to_string(&mut std::io::BufReader::new(std_file), &mut contents)
            .unwrap();
        assert_eq!(contents, "hello");
    }

    #[test]
    fn handle_to_std_file_rejects_invalid_handle() {
        // SAFETY: this fixture deliberately wraps the null handle to prove
        // that `handle_to_std_file` rejects an invalid handle. Taking
        // ownership of the null handle cannot close anything: `OwnedHandle`'s
        // `Drop` checks `is_valid()` first and skips `CloseHandle` for a
        // null handle.
        let invalid = unsafe { OwnedHandle::from_raw(std::ptr::null_mut()) };
        let error = handle_to_std_file(invalid).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(ERROR_INVALID_HANDLE as i32));
    }

    #[test]
    fn resolve_components_relative_empty() {
        let (_tmp, root_handle) = setup_test_root();
        let result = resolve_components_relative(root_handle.raw(), &[], true);
        assert!(result.is_ok());
        let handle = result.unwrap();
        assert!(handle.is_valid());
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
    fn owned_handle_invalid_try_clone() {
        let invalid = OwnedHandle(INVALID_HANDLE_VALUE);
        assert!(matches!(
            invalid.try_clone(),
            Err(WindowsFsError::IoError(ERROR_INVALID_HANDLE))
        ));
    }

    // ── Enumeration-to-open swap race tests ────────────────────────────────
    //
    // These tests verify that when an entry is enumerated and then opened
    // relative to the directory handle, the protocol correctly handles races.
    // They require a Windows runner with NTFS.

    #[test]
    #[ignore = "Race condition timing differs on CI; needs Windows VM qualification"]
    fn race_file_to_reparse_point_denied() {
        let tmp = TempDir::new().unwrap();
        let root_path = tmp.path().to_path_buf();
        std::fs::write(root_path.join("target.txt"), "original").unwrap();

        let root_utf16 = to_utf16_null(root_path.to_str().unwrap());
        // SAFETY: `root_utf16` is a null-terminated UTF-16 buffer that stays
        // alive for the call, and `CreateFileW` does not retain the pointer.
        // The two null arguments are the optional security-attributes and
        // template-handle parameters, which `CreateFileW` accepts as absent.
        let root_handle = unsafe {
            CreateFileW(
                root_utf16.as_ptr(),
                FILE_LIST_DIRECTORY | SYNCHRONIZE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        assert_ne!(root_handle, INVALID_HANDLE_VALUE);

        // Enumerate — should see target.txt as a file.
        let entries = enumerate_directory(root_handle, 4096).unwrap();
        assert!(entries.iter().any(|e| e.name == "target.txt"));
        let entry = entries.iter().find(|e| e.name == "target.txt").unwrap();
        assert_eq!(entry.kind, DirectoryEntryKind::File);

        // Now replace with a junction (reparse point) to a different directory.
        let outside = TempDir::new().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "leaked").unwrap();
        std::fs::remove_file(root_path.join("target.txt")).unwrap();
        std::os::windows::fs::symlink_dir(outside.path(), root_path.join("target.txt")).unwrap();

        // Open relative to directory handle — should detect reparse and deny.
        let result = open_any_relative(root_handle, "target.txt");
        match result {
            Ok(h) => {
                // If open succeeds, the reparse check should deny it.
                let check = deny_all_reparse_check(h.raw());
                assert!(
                    check.is_err(),
                    "reparse point should be denied after file-to-reparse swap"
                );
            }
            Err(WindowsFsError::NotFound) => {
                // Open may fail if the handle cannot follow the reparse.
                // This is also safe.
            }
            Err(_) => {
                // Other errors are acceptable — the key is that no bytes
                // outside the pinned root are served.
            }
        }

        // SAFETY: `root_handle` came from the `CreateFileW` above, is still
        // open, and is closed exactly once here — no other code path owns
        // or closes it.
        unsafe {
            CloseHandle(root_handle);
        }
    }

    #[test]
    #[ignore = "Race condition timing differs on CI; needs Windows VM qualification"]
    fn race_file_to_directory_type_change() {
        let tmp = TempDir::new().unwrap();
        let root_path = tmp.path().to_path_buf();
        std::fs::write(root_path.join("target.txt"), "original").unwrap();

        let root_utf16 = to_utf16_null(root_path.to_str().unwrap());
        // SAFETY: `root_utf16` is a null-terminated UTF-16 buffer that stays
        // alive for the call, and `CreateFileW` does not retain the pointer.
        // The two null arguments are the optional security-attributes and
        // template-handle parameters, which `CreateFileW` accepts as absent.
        let root_handle = unsafe {
            CreateFileW(
                root_utf16.as_ptr(),
                FILE_LIST_DIRECTORY | SYNCHRONIZE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        assert_ne!(root_handle, INVALID_HANDLE_VALUE);

        // Enumerate — should see target.txt as a file.
        let entries = enumerate_directory(root_handle, 4096).unwrap();
        let entry = entries.iter().find(|e| e.name == "target.txt");
        assert!(entry.is_some(), "target.txt should be in listing");
        assert_eq!(entry.unwrap().kind, DirectoryEntryKind::File);

        // Replace file with directory.
        std::fs::remove_file(root_path.join("target.txt")).unwrap();
        std::fs::create_dir(root_path.join("target.txt")).unwrap();

        // Open as file — should fail (directory, not file).
        let result = open_file_relative(root_handle, "target.txt");
        assert!(
            matches!(result, Err(WindowsFsError::NotADirectory)),
            "opening a directory as file should fail with NotADirectory after type change, got {:?}",
            result
        );

        // Open as directory — should succeed.
        let result = open_directory_relative(root_handle, "target.txt");
        assert!(
            result.is_ok(),
            "opening a directory as directory should succeed after type change, got {:?}",
            result
        );

        // SAFETY: `root_handle` came from the `CreateFileW` above, is still
        // open, and is closed exactly once here — no other code path owns
        // or closes it.
        unsafe {
            CloseHandle(root_handle);
        }
    }

    #[test]
    fn race_same_name_replacement_file() {
        let tmp = TempDir::new().unwrap();
        let root_path = tmp.path().to_path_buf();
        std::fs::write(root_path.join("target.txt"), "original").unwrap();

        let root_utf16 = to_utf16_null(root_path.to_str().unwrap());
        // SAFETY: `root_utf16` is a null-terminated UTF-16 buffer that stays
        // alive for the call, and `CreateFileW` does not retain the pointer.
        // The two null arguments are the optional security-attributes and
        // template-handle parameters, which `CreateFileW` accepts as absent.
        let root_handle = unsafe {
            CreateFileW(
                root_utf16.as_ptr(),
                FILE_LIST_DIRECTORY | SYNCHRONIZE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        assert_ne!(root_handle, INVALID_HANDLE_VALUE);

        // Open the file and verify original content.
        let file_handle = open_file_relative(root_handle, "target.txt").unwrap();
        let std_file = handle_to_std_file(file_handle).unwrap();
        let mut contents = String::new();
        std::io::Read::read_to_string(&mut std::io::BufReader::new(std_file), &mut contents)
            .unwrap();
        assert_eq!(contents, "original");

        // Replace with a different file (unlink + create = new inode).
        std::fs::remove_file(root_path.join("target.txt")).unwrap();
        std::fs::write(root_path.join("target.txt"), "replaced").unwrap();

        // Open again — should see the new content (new handle, new inode).
        let file_handle = open_file_relative(root_handle, "target.txt").unwrap();
        let std_file = handle_to_std_file(file_handle).unwrap();
        let mut contents = String::new();
        std::io::Read::read_to_string(&mut std::io::BufReader::new(std_file), &mut contents)
            .unwrap();
        assert_eq!(contents, "replaced");

        // SAFETY: `root_handle` came from the `CreateFileW` above, is still
        // open, and is closed exactly once here — no other code path owns
        // or closes it.
        unsafe {
            CloseHandle(root_handle);
        }
    }

    #[test]
    #[ignore = "Race condition timing differs on CI; needs Windows VM qualification"]
    fn race_delete_and_recreate() {
        let tmp = TempDir::new().unwrap();
        let root_path = tmp.path().to_path_buf();
        std::fs::write(root_path.join("target.txt"), "original").unwrap();

        let root_utf16 = to_utf16_null(root_path.to_str().unwrap());
        // SAFETY: `root_utf16` is a null-terminated UTF-16 buffer that stays
        // alive for the call, and `CreateFileW` does not retain the pointer.
        // The two null arguments are the optional security-attributes and
        // template-handle parameters, which `CreateFileW` accepts as absent.
        let root_handle = unsafe {
            CreateFileW(
                root_utf16.as_ptr(),
                FILE_LIST_DIRECTORY | SYNCHRONIZE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        assert_ne!(root_handle, INVALID_HANDLE_VALUE);

        // Enumerate — should see target.txt.
        let entries = enumerate_directory(root_handle, 4096).unwrap();
        assert!(entries.iter().any(|e| e.name == "target.txt"));

        // Delete the file.
        std::fs::remove_file(root_path.join("target.txt")).unwrap();

        // Open after deletion — should fail with NotFound.
        let result = open_file_relative(root_handle, "target.txt");
        assert!(
            matches!(result, Err(WindowsFsError::NotFound)),
            "opening deleted file should return NotFound, got {:?}",
            result
        );

        // Recreate with different content.
        std::fs::write(root_path.join("target.txt"), "recreated").unwrap();

        // Open after recreation — should succeed with new content.
        let file_handle = open_file_relative(root_handle, "target.txt").unwrap();
        let std_file = handle_to_std_file(file_handle).unwrap();
        let mut contents = String::new();
        std::io::Read::read_to_string(&mut std::io::BufReader::new(std_file), &mut contents)
            .unwrap();
        assert_eq!(contents, "recreated");

        // SAFETY: `root_handle` came from the `CreateFileW` above, is still
        // open, and is closed exactly once here — no other code path owns
        // or closes it.
        unsafe {
            CloseHandle(root_handle);
        }
    }

    #[test]
    #[ignore = "Race condition timing differs on CI; needs Windows VM qualification"]
    fn race_permission_change_after_enumeration() {
        let tmp = TempDir::new().unwrap();
        let root_path = tmp.path().to_path_buf();
        std::fs::write(root_path.join("target.txt"), "content").unwrap();

        let root_utf16 = to_utf16_null(root_path.to_str().unwrap());
        // SAFETY: `root_utf16` is a null-terminated UTF-16 buffer that stays
        // alive for the call, and `CreateFileW` does not retain the pointer.
        // The two null arguments are the optional security-attributes and
        // template-handle parameters, which `CreateFileW` accepts as absent.
        let root_handle = unsafe {
            CreateFileW(
                root_utf16.as_ptr(),
                FILE_LIST_DIRECTORY | SYNCHRONIZE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        assert_ne!(root_handle, INVALID_HANDLE_VALUE);

        // Enumerate — should see target.txt.
        let entries = enumerate_directory(root_handle, 4096).unwrap();
        assert!(entries.iter().any(|e| e.name == "target.txt"));

        // Remove read permission.
        let mut perms = std::fs::metadata(root_path.join("target.txt"))
            .unwrap()
            .permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(root_path.join("target.txt"), perms).unwrap();

        // Open — should still succeed (we opened with FILE_LIST_DIRECTORY on the
        // parent, and FILE_READ_DATA on the child; readonly doesn't prevent reading).
        let result = open_file_relative(root_handle, "target.txt");
        assert!(
            result.is_ok(),
            "opening a read-only file should succeed, got {:?}",
            result
        );

        // Restore permissions and verify content.
        let mut perms = std::fs::metadata(root_path.join("target.txt"))
            .unwrap()
            .permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        {
            perms.set_readonly(false);
        }
        std::fs::set_permissions(root_path.join("target.txt"), perms).unwrap();

        let file_handle = open_file_relative(root_handle, "target.txt").unwrap();
        let std_file = handle_to_std_file(file_handle).unwrap();
        let mut contents = String::new();
        std::io::Read::read_to_string(&mut std::io::BufReader::new(std_file), &mut contents)
            .unwrap();
        assert_eq!(contents, "content");

        // SAFETY: `root_handle` came from the `CreateFileW` above, is still
        // open, and is closed exactly once here — no other code path owns
        // or closes it.
        unsafe {
            CloseHandle(root_handle);
        }
    }

    #[test]
    #[ignore = "Race condition timing differs on CI; needs Windows VM qualification"]
    fn race_directory_entry_count_stability() {
        let tmp = TempDir::new().unwrap();
        let root_path = tmp.path().to_path_buf();
        // Create several files.
        for i in 0..10 {
            std::fs::write(
                root_path.join(format!("file{i}.txt")),
                format!("content{i}"),
            )
            .unwrap();
        }

        let root_utf16 = to_utf16_null(root_path.to_str().unwrap());
        // SAFETY: `root_utf16` is a null-terminated UTF-16 buffer that stays
        // alive for the call, and `CreateFileW` does not retain the pointer.
        // The two null arguments are the optional security-attributes and
        // template-handle parameters, which `CreateFileW` accepts as absent.
        let root_handle = unsafe {
            CreateFileW(
                root_utf16.as_ptr(),
                FILE_LIST_DIRECTORY | SYNCHRONIZE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null_mut(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        assert_ne!(root_handle, INVALID_HANDLE_VALUE);

        // Enumerate multiple times — should be stable.
        let entries1 = enumerate_directory(root_handle, 4096).unwrap();
        let entries2 = enumerate_directory(root_handle, 4096).unwrap();
        let entries3 = enumerate_directory(root_handle, 4096).unwrap();

        assert_eq!(entries1.len(), entries2.len());
        assert_eq!(entries2.len(), entries3.len());

        // Names should be consistent.
        let names1: Vec<&str> = entries1.iter().map(|e| e.name.as_str()).collect();
        let names2: Vec<&str> = entries2.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names1, names2);

        // SAFETY: `root_handle` came from the `CreateFileW` above, is still
        // open, and is closed exactly once here — no other code path owns
        // or closes it.
        unsafe {
            CloseHandle(root_handle);
        }
    }
}

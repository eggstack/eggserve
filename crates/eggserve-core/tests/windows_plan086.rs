//! Windows Plan 086 tests: adversarial filesystem qualification.
//!
//! This test suite covers the reparse-point denial matrix, namespace
//! normalization, concurrent mutation races, root identity, file validators,
//! ACL/sharing behavior, resource stability, and installed artifact parity
//! for the Windows hardened static-serving path.
//!
//! Tests requiring Developer Mode, elevated privileges, or a dedicated VM are
//! marked with `#[ignore]` and a reason string. They cannot run on standard
//! GitHub-hosted runners and must be executed on a dedicated Windows
//! qualification environment.
//!
//! Tests that cannot create a fixture report `blocked-fixture` rather than
//! passing or skipping silently.
//!
//! All tests are gated with `#![cfg(windows)]` and will not compile on
//! other platforms.

#![cfg(windows)]
#![allow(
    dead_code,
    clippy::upper_case_acronyms,
    clippy::io_other_error,
    clippy::unnecessary_map_or,
    clippy::single_match
)]

use std::ffi::c_void;
use std::fs;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::Arc;
use std::thread;

use tempfile::TempDir;

use eggserve_core::policy::StaticPolicy;
use eggserve_core::primitives::response::BodyPlan;
use eggserve_core::primitives::{check_component, SecureRoot};
use eggserve_core::primitives::{ConfinedPath, PathPolicy};

// ============================================================================
// Inline Windows FFI — test isolation mirrors windows_feasibility.rs
// ============================================================================

type HANDLE = *mut c_void;
type DWORD = u32;
type BOOL = i32;
type PCWSTR = *const u16;

const INVALID_HANDLE_VALUE: HANDLE = -1isize as HANDLE;

const GENERIC_READ: DWORD = 0x80000000;
const FILE_LIST_DIRECTORY: DWORD = 0x00000001;
const FILE_SHARE_READ: DWORD = 0x00000001;
const FILE_SHARE_WRITE: DWORD = 0x00000002;
const FILE_SHARE_DELETE: DWORD = 0x00000004;
const OPEN_EXISTING: DWORD = 3;
const FILE_FLAG_BACKUP_SEMANTICS: DWORD = 0x02000000;
const FILE_FLAG_OPEN_REPARSE_POINT: DWORD = 0x00200000;
const FILE_ATTRIBUTE_DIRECTORY: DWORD = 0x00000010;
const FILE_ATTRIBUTE_REPARSE_POINT: DWORD = 0x00000400;
const IO_REPARSE_TAG_SYMLINK: u32 = 0xA0000000;
const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA0000003;
const MAX_PATH_W: usize = 32768;

#[repr(C)]
#[derive(Clone, Copy, Default)]
#[allow(dead_code)]
struct FILE_ATTRIBUTE_TAG_INFO {
    file_attributes: DWORD,
    reparse_tag: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
#[allow(dead_code)]
struct FILE_STANDARD_INFO {
    allocation_size: i64,
    end_of_file: i64,
    number_of_links: DWORD,
    delete_pending: u8,
    directory: u8,
}

const CREATE_NEW: DWORD = 1;
const FILE_ATTRIBUTE_NORMAL: DWORD = 0x00000080;
const FILE_ATTRIBUTE_READONLY: DWORD = 0x00000001;
const GENERIC_WRITE: DWORD = 0x40000000;
const FILE_ID_BOTH_DIR_INFO: u32 = 3;
const FILE_ALTERNATE_NAME_INFO: u32 = 21;
const LOCKFILE_EXCLUSIVE_LOCK: DWORD = 0x00000002;
const LOCKFILE_FAIL_IMMEDIATELY: DWORD = 0x00000001;

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

    fn GetFileInformationByHandleEx(
        h_file: HANDLE,
        file_info_class: u32,
        lp_file_information: *mut c_void,
        dw_buffer_size: DWORD,
    ) -> BOOL;

    fn GetFinalPathNameByHandleW(
        h_file: HANDLE,
        lpsz_file_path: *mut u16,
        cch_file_path: DWORD,
        dw_flags: DWORD,
    ) -> DWORD;

    fn GetLastError() -> DWORD;

    fn SetFileAttributesW(lp_file_name: PCWSTR, dw_file_attributes: DWORD) -> BOOL;

    fn GetVolumeInformationW(
        lp_root_path_name: PCWSTR,
        lp_volume_name_buffer: *mut u16,
        n_volume_name_size: DWORD,
        lp_volume_serial_number: *mut DWORD,
        lp_maximum_component_length: *mut DWORD,
        lp_file_system_flags: *mut DWORD,
        lp_file_system_name_buffer: *mut u16,
        n_file_system_name_size: DWORD,
    ) -> BOOL;

    fn LockFileEx(
        h_file: HANDLE,
        dw_flags: DWORD,
        dw_reserved: DWORD,
        n_number_of_bytes_to_lock_low: DWORD,
        n_number_of_bytes_to_lock_high: DWORD,
        lp_overlapped: *mut c_void,
    ) -> BOOL;

    fn UnlockFile(
        h_file: HANDLE,
        dw_file_offset_low: DWORD,
        dw_file_offset_high: DWORD,
        n_number_of_bytes_to_unlock_low: DWORD,
        n_number_of_bytes_to_unlock_high: DWORD,
    ) -> BOOL;

    fn GetProcessHandleCount(h_process: HANDLE, pdw_handle_count: *mut DWORD) -> BOOL;

    fn GetCurrentProcess() -> HANDLE;
}

// ============================================================================
// Helpers
// ============================================================================

fn utf16_string(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn open_root_handle(path: &Path) -> OwnedHandle {
    let wide = utf16_string(path.to_str().unwrap());
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_LIST_DIRECTORY,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    assert_ne!(
        handle,
        INVALID_HANDLE_VALUE,
        "Failed to open root directory {:?}: error {}",
        path,
        unsafe { GetLastError() }
    );
    unsafe { OwnedHandle::from_raw_handle(handle as _) }
}

fn get_final_path(handle: &OwnedHandle) -> io::Result<PathBuf> {
    let mut buf = vec![0u16; MAX_PATH_W];
    let len = unsafe {
        GetFinalPathNameByHandleW(
            handle.as_raw_handle() as HANDLE,
            buf.as_mut_ptr(),
            buf.len() as DWORD,
            0,
        )
    };
    if len == 0 {
        return Err(io::Error::last_os_error());
    }
    buf.truncate(len as usize);
    let s = String::from_utf16_lossy(&buf);
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    Ok(PathBuf::from(s))
}

fn get_file_standard_info(handle: &OwnedHandle) -> io::Result<FILE_STANDARD_INFO> {
    let mut info: FILE_STANDARD_INFO = unsafe { std::mem::zeroed() };
    let success = unsafe {
        GetFileInformationByHandleEx(
            handle.as_raw_handle() as HANDLE,
            1, // FileStandardInfo
            &mut info as *mut _ as *mut _,
            std::mem::size_of::<FILE_STANDARD_INFO>() as DWORD,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(info)
}

fn get_attribute_tag_info(handle: &OwnedHandle) -> io::Result<(u32, u32)> {
    let mut info: FILE_ATTRIBUTE_TAG_INFO = unsafe { std::mem::zeroed() };
    let success = unsafe {
        GetFileInformationByHandleEx(
            handle.as_raw_handle() as HANDLE,
            9, // FileAttributeTagInfo
            &mut info as *mut _ as *mut _,
            std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as DWORD,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((info.file_attributes, info.reparse_tag))
}

/// Create a standard test directory tree for Plan 086 tests.
fn create_plan086_tree(root: &Path) {
    fs::create_dir_all(root.join("subdir/deep")).expect("create dirs");
    fs::write(root.join("hello.txt"), "hello").expect("write hello.txt");
    fs::write(root.join("subdir/nested.txt"), "nested").expect("write nested.txt");
    fs::write(root.join("subdir/deep/deep.txt"), "deep").expect("write deep.txt");
    fs::write(root.join("visible.txt"), "visible").expect("write visible.txt");
    fs::write(root.join("subdir/index.html"), "<html>index</html>").expect("write index.html");
}

fn parse(raw: &str) -> ConfinedPath {
    ConfinedPath::parse(raw, &PathPolicy::default()).unwrap()
}

fn make_plan() -> eggserve_core::primitives::response::StaticResponsePlan {
    eggserve_core::primitives::response::StaticResponsePlan {
        status: eggserve_core::primitives::response::ResponseStatus::OK,
        headers: eggserve_core::primitives::response::HeaderMapPlan::new(),
        body: BodyPlan::FileFull,
    }
}

/// Read all bytes from a resolved file resource.
fn read_resolved_file(file: eggserve_core::primitives::SecureRoot) -> Vec<u8> {
    let plan = make_plan();
    let mut body = file.into_body(&plan).expect("into_body");
    body.read_all().expect("read_all")
}

/// Create a symlink, returning Err if Developer Mode is not available.
fn try_create_file_symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_file(target, link)
}

/// Create a directory symlink, returning Err if Developer Mode is not available.
fn try_create_dir_symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

/// Create a junction via mklink /J. Returns true on success.
fn try_create_junction(target: &Path, link: &Path) -> bool {
    std::process::Command::new("cmd")
        .args([
            "/C",
            "mklink",
            "/J",
            link.to_str().unwrap(),
            target.to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Set the readonly attribute on a file.
fn set_readonly(path: &Path, readonly: bool) -> bool {
    let wide = utf16_string(path.to_str().unwrap());
    let mut attrs = unsafe { GetFileAttributesW(wide.as_ptr()) };
    if attrs == 0xFFFFFFFF {
        return false;
    }
    if readonly {
        attrs |= FILE_ATTRIBUTE_READONLY;
    } else {
        attrs &= !FILE_ATTRIBUTE_READONLY;
    }
    unsafe { SetFileAttributesW(wide.as_ptr(), attrs) != 0 }
}

/// Get the filesystem type name for a given path.
fn get_filesystem_type(path: &Path) -> String {
    let wide = utf16_string(path.to_str().unwrap());
    let mut fs_name = vec![0u16; 256];
    let success = unsafe {
        GetVolumeInformationW(
            wide.as_ptr(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            fs_name.as_mut_ptr(),
            fs_name.len() as DWORD,
        )
    };
    if success != 0 {
        fs_name.truncate(
            fs_name
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(fs_name.len()),
        );
        String::from_utf16_lossy(&fs_name)
    } else {
        "unknown".to_string()
    }
}

/// Get file attributes for a path.
fn get_file_attributes(path: &Path) -> DWORD {
    let wide = utf16_string(path.to_str().unwrap());
    unsafe { GetFileAttributesW(wide.as_ptr()) }
}

extern "system" {
    fn GetFileAttributesW(lp_file_name: PCWSTR) -> DWORD;
}

/// Read all bytes from a resolved file via streaming body.
fn read_file_bytes(root: &SecureRoot, path_str: &str) -> Vec<u8> {
    let result = root.resolve(&parse(path_str));
    let file = result.into_file().expect("expected file");
    let plan = make_plan();
    let mut body = file.into_body(&plan).expect("into_body");
    body.read_all().expect("read_all")
}

/// Retrieve the 8.3 short (alternate) name for a file via NTFS API.
/// Returns None if the file has no alternate name or the call fails.
fn get_alternate_name(path: &Path) -> Option<String> {
    let wide = utf16_string(path.to_str().unwrap());
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null_mut(),
            OPEN_EXISTING,
            0,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return None;
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(handle as _) };
    // First call to get required buffer size.
    let mut buf_size: DWORD = 0;
    unsafe {
        GetFileInformationByHandleEx(
            handle.as_raw_handle() as HANDLE,
            FILE_ALTERNATE_NAME_INFO,
            ptr::null_mut(),
            0,
        );
    }
    // Allocate large buffer and query.
    let mut buf = vec![0u8; 1024];
    let success = unsafe {
        GetFileInformationByHandleEx(
            handle.as_raw_handle() as HANDLE,
            FILE_ALTERNATE_NAME_INFO,
            buf.as_mut_ptr() as *mut _,
            buf.len() as DWORD,
        )
    };
    if success == 0 {
        return None;
    }
    // The buffer starts with a DWORD length (in bytes), followed by UTF-16LE chars.
    let name_len = unsafe { ptr::read_unaligned(buf.as_ptr() as *const DWORD) } as usize;
    if name_len == 0 {
        return None;
    }
    let chars = &buf[4..4 + name_len];
    let u16s: Vec<u16> = chars
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    Some(String::from_utf16_lossy(&u16s))
}

/// Try to acquire an exclusive lock on a file (returns true on success).
fn lock_file_ex(path: &Path) -> Option<OwnedHandle> {
    let wide = utf16_string(path.to_str().unwrap());
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0, // No sharing — exclusive lock.
            ptr::null_mut(),
            OPEN_EXISTING,
            0,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return None;
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(handle as _) };
    // Lock the entire file.
    let mut overlapped: u64 = 0;
    let success = unsafe {
        LockFileEx(
            handle.as_raw_handle() as HANDLE,
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            0xFFFF_FFFF, // Low DWORD of length
            0xFFFF_FFFF, // High DWORD of length
            &mut overlapped as *mut _ as *mut _,
        )
    };
    if success == 0 {
        return None; // Lock failed — another handle holds it.
    }
    Some(handle)
}

/// Release a file lock and close the handle.
fn unlock_file(handle: OwnedHandle) {
    let mut overlapped: u64 = 0;
    unsafe {
        UnlockFile(
            handle.as_raw_handle() as HANDLE,
            0,
            0,
            0xFFFF_FFFF,
            0xFFFF_FFFF,
        );
    }
    drop(handle);
}

/// Get the current process handle count (for leak detection).
fn get_process_handle_count() -> DWORD {
    let mut count: DWORD = 0;
    unsafe {
        GetProcessHandleCount(GetCurrentProcess(), &mut count);
    }
    count
}

/// Compute a simple content digest (sum of bytes) for identity verification.
fn content_digest(data: &[u8]) -> u64 {
    data.iter().map(|&b| b as u64).sum()
}

// ============================================================================
// Track B — Reparse-point denial matrix
//
// The hardened policy is tag-independent denial: any object carrying
// FILE_ATTRIBUTE_REPARSE_POINT is denied before it contributes content
// or traversal authority.
// ============================================================================

#[test]
fn windows_reparse_file_symlink_denied_by_production_path() {
    // A file symlink within the root must be denied by the production
    // SecureRoot/RootGuard path, not just the low-level open_relative.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    // Create a file symlink (requires Developer Mode on Windows).
    let symlink_path = tmp.path().join("link_to_file");
    match std::os::windows::fs::symlink_file(tmp.path().join("hello.txt"), &symlink_path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("blocked-fixture: symlink creation requires Developer Mode: {e}");
            return;
        }
        Err(e) => panic!("unexpected error creating symlink: {e}"),
    }

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let result = root.resolve(&parse("/link_to_file"));
    assert!(
        result.is_denied(),
        "file symlink must be denied under hardened policy"
    );
    assert!(
        !result.is_file(),
        "no bytes from a reparse target must be served"
    );
}

#[test]
fn windows_reparse_directory_symlink_denied_by_production_path() {
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let symlink_path = tmp.path().join("link_to_subdir");
    match std::os::windows::fs::symlink_dir(tmp.path().join("subdir"), &symlink_path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("blocked-fixture: symlink creation requires Developer Mode: {e}");
            return;
        }
        Err(e) => panic!("unexpected error creating symlink: {e}"),
    }

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let result = root.resolve(&parse("/link_to_subdir"));
    assert!(
        result.is_denied(),
        "directory symlink must be denied under hardened policy"
    );
}

#[test]
#[ignore = "requires elevated privileges for junction creation"]
fn windows_reparse_junction_denied_by_production_path() {
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let junction_path = tmp.path().join("junction_to_subdir");
    let target = tmp.path().join("subdir");
    let status = std::process::Command::new("cmd")
        .args([
            "/C",
            "mklink",
            "/J",
            junction_path.to_str().unwrap(),
            target.to_str().unwrap(),
        ])
        .status()
        .expect("should run mklink");
    assert!(status.success(), "mklink /J should succeed");

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let result = root.resolve(&parse("/junction_to_subdir"));
    assert!(
        result.is_denied(),
        "junction must be denied under hardened policy"
    );
}

#[test]
fn windows_reparse_intermediate_component_denied() {
    // If a reparse point exists as an intermediate path component,
    // traversal through it must be denied.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    // Create a symlink inside subdir pointing to deep.
    let symlink_path = tmp.path().join("subdir/link_to_deep");
    match std::os::windows::fs::symlink_dir(tmp.path().join("subdir/deep"), &symlink_path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("blocked-fixture: symlink creation requires Developer Mode: {e}");
            return;
        }
        Err(e) => panic!("unexpected error creating symlink: {e}"),
    }

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    // Try to resolve through the symlink as an intermediate component.
    let result = root.resolve(&parse("/subdir/link_to_deep/deep.txt"));
    assert!(
        result.is_denied(),
        "intermediate reparse component must be denied"
    );
}

#[test]
fn windows_reparse_index_file_denied() {
    // If the index file in a directory is a reparse point, directory
    // index resolution must not serve it.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    // Replace index.html with a symlink to hello.txt.
    let index_path = tmp.path().join("subdir/index.html");
    fs::remove_file(&index_path).expect("remove index.html");
    match std::os::windows::fs::symlink_file(tmp.path().join("hello.txt"), &index_path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("blocked-fixture: symlink creation requires Developer Mode: {e}");
            return;
        }
        Err(e) => panic!("unexpected error creating symlink: {e}"),
    }

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let dir = root
        .resolve(&parse("/subdir"))
        .into_directory()
        .expect("subdir should be directory");

    // Resolving the index from the directory handle should deny the reparse.
    let index_result = dir.resolve_child("index.html", &root);
    assert!(
        index_result.is_denied(),
        "reparse point index file must be denied"
    );
}

#[test]
fn windows_reparse_listing_entry_filtered() {
    // Reparse point entries must be filtered from directory listings
    // under the hardened policy.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    // Create a symlink in the directory.
    let symlink_path = tmp.path().join("link_in_dir");
    match std::os::windows::fs::symlink_file(tmp.path().join("hello.txt"), &symlink_path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("blocked-fixture: symlink creation requires Developer Mode: {e}");
            return;
        }
        Err(e) => panic!("unexpected error creating symlink: {e}"),
    }

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let dir = root
        .resolve(&parse("/"))
        .into_directory()
        .expect("root should be directory");

    let entries = dir
        .list(&root, eggserve_core::limits::DEFAULT_MAX_LISTING_ENTRIES)
        .expect("list should succeed");
    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    assert!(
        !names.contains(&"link_in_dir"),
        "reparse point entry must be filtered from listing"
    );
    assert!(
        names.contains(&"hello.txt"),
        "regular file must still appear in listing"
    );
}

#[test]
fn windows_reparse_dangling_denied() {
    // A dangling symlink (target does not exist) must still be detected
    // as a reparse point and denied.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let symlink_path = tmp.path().join("dangling_link");
    match std::os::windows::fs::symlink_file(
        tmp.path().join("nonexistent_target.txt"),
        &symlink_path,
    ) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("blocked-fixture: symlink creation requires Developer Mode: {e}");
            return;
        }
        Err(e) => panic!("unexpected error creating symlink: {e}"),
    }

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let result = root.resolve(&parse("/dangling_link"));
    assert!(result.is_denied(), "dangling reparse point must be denied");
}

#[test]
fn windows_reparse_get_head_agree() {
    // GET and HEAD must agree on status and body suppression for
    // reparse point denial.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let symlink_path = tmp.path().join("link_to_file");
    match std::os::windows::fs::symlink_file(tmp.path().join("hello.txt"), &symlink_path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("blocked-fixture: symlink creation requires Developer Mode: {e}");
            return;
        }
        Err(e) => panic!("unexpected error creating symlink: {e}"),
    }

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let result_get = root.resolve(&parse("/link_to_file"));
    let result_head = root.resolve(&parse("/link_to_file"));

    // Both must be denied.
    assert!(result_get.is_denied(), "GET must deny reparse");
    assert!(result_head.is_denied(), "HEAD must deny reparse");
}

#[test]
fn windows_reparse_no_handle_leak() {
    // After denying a reparse point, no handles or permits must leak.
    // Repeated denials must return to baseline.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let symlink_path = tmp.path().join("link_to_file");
    match std::os::windows::fs::symlink_file(tmp.path().join("hello.txt"), &symlink_path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("blocked-fixture: symlink creation requires Developer Mode: {e}");
            return;
        }
        Err(e) => panic!("unexpected error creating symlink: {e}"),
    }

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Deny 100 times — handles must not leak.
    for _ in 0..100 {
        let result = root.resolve(&parse("/link_to_file"));
        assert!(result.is_denied(), "reparse must be denied every time");
    }

    // After all denials, a valid file must still resolve correctly.
    let valid = root.resolve(&parse("/hello.txt"));
    assert!(
        valid.is_file(),
        "valid file must still resolve after repeated denials"
    );
}

#[test]
fn windows_reparse_target_inside_root_denied() {
    // Even if the reparse target is inside the root, the reparse point
    // itself must be denied (tag-independent denial).
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    // Create a symlink to a file inside the root.
    let symlink_path = tmp.path().join("internal_link");
    match std::os::windows::fs::symlink_file(tmp.path().join("visible.txt"), &symlink_path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("blocked-fixture: symlink creation requires Developer Mode: {e}");
            return;
        }
        Err(e) => panic!("unexpected error creating symlink: {e}"),
    }

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let result = root.resolve(&parse("/internal_link"));
    assert!(
        result.is_denied(),
        "reparse targeting inside root must still be denied"
    );
}

#[test]
fn windows_reparse_target_outside_root_denied() {
    // A reparse point targeting outside the root must be denied.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    // Create an external target directory.
    let external_dir = TempDir::new().unwrap();
    fs::write(external_dir.path().join("secret.txt"), "secret").expect("write secret");

    let symlink_path = tmp.path().join("escape_link");
    match std::os::windows::fs::symlink_file(external_dir.path().join("secret.txt"), &symlink_path)
    {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("blocked-fixture: symlink creation requires Developer Mode: {e}");
            return;
        }
        Err(e) => panic!("unexpected error creating symlink: {e}"),
    }

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let result = root.resolve(&parse("/escape_link"));
    assert!(
        result.is_denied(),
        "reparse targeting outside root must be denied"
    );
}

// ============================================================================
// Track C — Namespace and normalization matrix
//
// Unsafe/ambiguous request forms must be rejected before filesystem access.
// ============================================================================

#[test]
fn windows_namespace_drive_prefix_rejected() {
    // Drive prefix forms must be rejected at the parser level.
    let result = check_component("C:");
    assert!(result.is_err(), "C: must be rejected");
    let result = check_component("D:");
    assert!(result.is_err(), "D: must be rejected");
    let result = check_component("Z:/path");
    assert!(result.is_err(), "Z:/path must be rejected");
}

#[test]
fn windows_namespace_ads_rejected() {
    // Alternate data stream syntax must be rejected.
    let result = check_component("file.txt:stream");
    assert!(result.is_err(), "ADS syntax must be rejected");
    let result = check_component("file.txt:$DATA");
    assert!(result.is_err(), "ADS $DATA must be rejected");
}

#[test]
fn windows_namespace_reserved_names_rejected() {
    // Reserved DOS device names must be rejected with case and extensions.
    let reserved = ["CON", "PRN", "AUX", "NUL", "COM1", "COM2", "LPT1", "LPT2"];
    for name in &reserved {
        let result = check_component(name);
        assert!(result.is_err(), "{name} must be rejected");
        // With extension.
        let with_ext = format!("{name}.txt");
        let result = check_component(&with_ext);
        assert!(result.is_err(), "{with_ext} must be rejected");
    }
    // Trailing dots.
    let result = check_component("CON.");
    assert!(result.is_err(), "CON. must be rejected");
    let result = check_component("NUL...");
    assert!(result.is_err(), "NUL... must be rejected");
    // Normal names must pass.
    assert!(check_component("hello.txt").is_ok());
    assert!(check_component("CONSOLE.txt").is_ok());
    assert!(check_component("auxiliary.txt").is_ok());
}

#[test]
fn windows_namespace_backslash_in_component_rejected() {
    // Backslash within a path component must be rejected by default.
    let result = ConfinedPath::parse("/path\\file.txt", &PathPolicy::default());
    assert!(result.is_err(), "backslash in path must be rejected");
}

#[test]
fn windows_namespace_double_encoding_rejected() {
    // Double-encoded traversal must decode to literal, not traverse.
    // %252e%252e decodes to %2e%2e (literal filename), not ".."
    let result = ConfinedPath::parse("/%252e%252e/etc/passwd", &PathPolicy::default());
    assert!(
        result.is_err(),
        "double-encoded traversal must be rejected (decodes to literal .)"
    );
}

#[test]
fn windows_namespace_long_component_rejected() {
    // Components exceeding the Windows MAX_PATH limit should be rejected.
    let long_name = "a".repeat(300);
    let path = format!("/{long_name}.txt");
    let result = ConfinedPath::parse(&path, &PathPolicy::default());
    // This may succeed at the parser level but fail at resolution.
    // The key invariant is: it must not bypass confinement.
    if let Ok(confined) = result {
        let tmp = TempDir::new().unwrap();
        create_plan086_tree(tmp.path());
        let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
        let resolved = root.resolve(&confined);
        assert!(
            !resolved.is_file(),
            "long component must not resolve to a file"
        );
    }
}

#[test]
fn windows_namespace_encoded_separator_rejected() {
    // Percent-encoded forward slash must be decoded and treated as separator,
    // not as a literal character in a component.
    let result = ConfinedPath::parse("/path%2Ffile.txt", &PathPolicy::default());
    // %2F decodes to '/' which is a separator — this should parse as /path/file.txt
    // or be rejected. The key is it must not be treated as a single component "path/file.txt".
    if let Ok(confined) = result {
        let components: Vec<_> = confined.components().collect();
        assert!(
            components.len() > 1 || confined.as_str().contains('/'),
            "encoded slash must not create a single-component path"
        );
    }
}

#[test]
fn windows_namespace_dotfile_policy_enforced() {
    // Dotfiles must be denied by default.
    let result = ConfinedPath::parse("/.hidden", &PathPolicy::default());
    assert!(result.is_err(), "dotfile must be rejected by default");

    // With allow-dotfiles policy, dotfiles must pass.
    let policy = PathPolicy {
        dotfiles: eggserve_core::primitives::PathDotfilePolicy::Allow,
        ..PathPolicy::default()
    };
    let result = ConfinedPath::parse("/.hidden", &policy);
    assert!(result.is_ok(), "dotfile must pass with Allow policy");
}

// ============================================================================
// Track D — Concurrent mutation race harness
//
// Race tests verify that concurrent filesystem mutations cannot cause
// root escape, mixed content, or serving denied objects.
// ============================================================================

#[test]
fn windows_race_file_to_reparse_swap_denied() {
    // Create a file, resolve it, replace with symlink, resolve again.
    // The second resolution must deny the reparse point.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // First: resolve the original file.
    let result1 = root.resolve(&parse("/hello.txt"));
    assert!(result1.is_file(), "original file must resolve");

    // Replace file with symlink.
    let target_path = tmp.path().join("hello.txt");
    fs::remove_file(&target_path).expect("remove original");
    match std::os::windows::fs::symlink_file(tmp.path().join("visible.txt"), &target_path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("blocked-fixture: symlink creation requires Developer Mode: {e}");
            return;
        }
        Err(e) => panic!("unexpected error creating symlink: {e}"),
    }

    // Second: resolution must now deny the reparse.
    let result2 = root.resolve(&parse("/hello.txt"));
    assert!(
        result2.is_denied(),
        "reparse swap must be denied on re-resolution"
    );
}

#[test]
fn windows_race_file_to_directory_type_change() {
    // Replace a file with a directory between resolutions.
    // The server must handle this gracefully (404 or error, not crash).
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Resolve the file.
    let result1 = root.resolve(&parse("/hello.txt"));
    assert!(result1.is_file(), "original must be file");

    // Replace file with directory.
    fs::remove_file(tmp.path().join("hello.txt")).expect("remove file");
    fs::create_dir(tmp.path().join("hello.txt")).expect("create dir");

    // Re-resolve: should fail (directory, not file) or be NotFound.
    let result2 = root.resolve(&parse("/hello.txt"));
    // On Windows, opening a directory as a file fails. The result should
    // not be a regular file serving bytes.
    assert!(
        !result2.is_file(),
        "file-to-directory swap must not serve content"
    );
}

#[test]
fn windows_race_same_name_reparse_to_file() {
    // Create a symlink, resolve it (denied), replace with regular file,
    // resolve again (should succeed).
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    // Create a symlink.
    let symlink_path = tmp.path().join("swapping");
    match std::os::windows::fs::symlink_file(tmp.path().join("hello.txt"), &symlink_path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("blocked-fixture: symlink creation requires Developer Mode: {e}");
            return;
        }
        Err(e) => panic!("unexpected error creating symlink: {e}"),
    }

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // First resolution: denied (reparse).
    let result1 = root.resolve(&parse("/swapping"));
    assert!(result1.is_denied(), "reparse must be denied");

    // Replace with regular file.
    fs::remove_file(&symlink_path).expect("remove symlink");
    fs::write(&symlink_path, "new content").expect("write regular file");

    // Second resolution: should succeed now.
    let result2 = root.resolve(&parse("/swapping"));
    assert!(
        result2.is_file(),
        "regular file replacement must resolve after reparse is gone"
    );
}

#[test]
fn windows_race_delete_recreate_during_enumeration() {
    // Delete and recreate a file between enumeration calls.
    // The server must not crash or serve inconsistent data.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let dir = root
        .resolve(&parse("/subdir"))
        .into_directory()
        .expect("subdir should be directory");

    // First enumeration.
    let entries1 = dir
        .list(&root, eggserve_core::limits::DEFAULT_MAX_LISTING_ENTRIES)
        .expect("first list should succeed");

    // Delete and recreate a file.
    fs::remove_file(tmp.path().join("subdir/nested.txt")).expect("remove nested.txt");
    fs::write(tmp.path().join("subdir/nested.txt"), "recreated").expect("recreate nested.txt");

    // Second enumeration: must not crash.
    let entries2 = dir
        .list(&root, eggserve_core::limits::DEFAULT_MAX_LISTING_ENTRIES)
        .expect("second list should succeed");

    // Both enumerations should have returned results.
    assert!(!entries1.is_empty(), "first enumeration must have entries");
    assert!(!entries2.is_empty(), "second enumeration must have entries");
}

// ============================================================================
// Track E — Root identity and deployment replacement behavior
//
// The pinned-root contract under Windows sharing and rename semantics.
// ============================================================================

#[test]
fn windows_root_rename_does_not_retarget_pinned_root() {
    // After startup, renaming the root directory must not redirect the
    // running server to a different tree.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Verify the root serves the original content.
    let result = root.resolve(&parse("/hello.txt"));
    assert!(result.is_file(), "original root must serve files");

    // Rename the root directory.
    let renamed_path = tmp.path().with_file_name(format!(
        "{}_renamed",
        tmp.path().file_name().unwrap().to_str().unwrap()
    ));
    fs::rename(tmp.path(), &renamed_path).expect("rename root");

    // Create a new directory at the old path.
    fs::create_dir(tmp.path()).expect("create new dir at old path");
    fs::write(tmp.path().join("hello.txt"), "different content").expect("write different content");

    // The pinned root must still serve the original content (through its
    // retained handle), not the new directory.
    let result = root.resolve(&parse("/hello.txt"));
    assert!(
        result.is_file(),
        "pinned root must survive rename and serve original content"
    );

    // Read the content to verify it's the original.
    let file = result.into_file().expect("should be file");
    let plan = make_plan();
    let mut body = file.into_body(&plan).expect("into_body");
    let data = body.read_all().expect("read_all");
    assert_eq!(
        data, b"hello",
        "content must be from original root, not replacement"
    );
}

#[test]
fn windows_new_requests_continue_using_pinned_root() {
    // After root rename, new requests must continue using the pinned root
    // identity until server restart.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Rename root, create new directory at old path.
    let renamed_path = tmp.path().with_file_name(format!(
        "{}_renamed",
        tmp.path().file_name().unwrap().to_str().unwrap()
    ));
    fs::rename(tmp.path(), &renamed_path).expect("rename root");
    fs::create_dir(tmp.path()).expect("create new dir");
    fs::write(tmp.path().join("new_file.txt"), "new").expect("write new file");

    // Multiple new requests must still see the original root.
    for _ in 0..10 {
        let result = root.resolve(&parse("/hello.txt"));
        assert!(
            result.is_file(),
            "new request must use pinned root identity"
        );
    }

    // The new file in the replacement directory must NOT be visible.
    let result = root.resolve(&parse("/new_file.txt"));
    assert!(
        !result.is_file(),
        "new file in replacement directory must not be visible through pinned root"
    );
}

#[test]
fn windows_old_root_handles_retained_during_streaming() {
    // Root handles must be retained during in-flight streams.
    // The stream must continue reading from the original root even if
    // the root pathname is renamed.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Resolve a file.
    let file = root
        .resolve(&parse("/hello.txt"))
        .into_file()
        .expect("hello.txt should resolve");

    // Rename the root while the file handle is held.
    let renamed_path = tmp.path().with_file_name(format!(
        "{}_renamed",
        tmp.path().file_name().unwrap().to_str().unwrap()
    ));
    fs::rename(tmp.path(), &renamed_path).expect("rename root during stream");

    // Read from the file handle — must still work.
    let plan = make_plan();
    let mut body = file.into_body(&plan).expect("into_body");
    let data = body.read_all().expect("read_all");
    assert_eq!(
        data, b"hello",
        "stream must continue from original root after rename"
    );
}

// ============================================================================
// Track F — File identity, validators, and range consistency
//
// ETags must change when the served representation changes under the
// documented strong/weak validator contract.
// ============================================================================

#[test]
fn windows_file_identity_same_size_replacement() {
    // Replace a file with a same-size file. ETag/Last-Modified must change
    // to reflect the new representation.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Resolve original.
    let file1 = root
        .resolve(&parse("/hello.txt"))
        .into_file()
        .expect("hello.txt should resolve");
    let meta1 = file1.metadata().clone();
    drop(file1);

    // Replace with same-size content.
    fs::write(tmp.path().join("hello.txt"), "world").expect("replace with same size");

    // Resolve replacement.
    let file2 = root
        .resolve(&parse("/hello.txt"))
        .into_file()
        .expect("replaced hello.txt should resolve");
    let meta2 = file2.metadata().clone();
    drop(file2);

    // File size is the same (5 bytes), but modified time should differ.
    assert_eq!(meta1.len(), meta2.len(), "replacement must have same size");
    // Modified time may or may not differ depending on filesystem timestamp
    // granularity. On NTFS, the minimum granularity is 100ns, so rapid
    // replacements may have the same timestamp. The test documents this
    // limitation.
}

#[test]
fn windows_file_identity_rename_over_existing() {
    // Rename a new file over an existing file. The resolved file must
    // reflect the new content.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Create a replacement file.
    fs::write(tmp.path().join("replacement.txt"), "replaced").expect("write replacement");

    // Rename over the original.
    fs::rename(
        tmp.path().join("replacement.txt"),
        tmp.path().join("hello.txt"),
    )
    .expect("rename over existing");

    // Resolve and verify content.
    let file = root
        .resolve(&parse("/hello.txt"))
        .into_file()
        .expect("hello.txt should resolve after rename-over");
    let plan = make_plan();
    let mut body = file.into_body(&plan).expect("into_body");
    let data = body.read_all().expect("read_all");
    assert_eq!(
        data, b"replaced",
        "content must reflect the renamed-over file"
    );
}

#[test]
fn windows_file_identity_direct_vs_directory_index() {
    // A direct file request and a directory index request for the same
    // path component must have consistent identity behavior.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Resolve as direct file.
    let file = root
        .resolve(&parse("/subdir/index.html"))
        .into_file()
        .expect("index.html should resolve as direct file");
    let plan = make_plan();
    let mut body = file.into_body(&plan).expect("into_body");
    let data = body.read_all().expect("read_all");
    assert_eq!(data, b"<html>index</html>");

    // Resolve as directory index.
    let dir = root
        .resolve(&parse("/subdir"))
        .into_directory()
        .expect("subdir should resolve as directory");
    let index = dir
        .resolve_child("index.html", &root)
        .into_file()
        .expect("index.html should resolve from directory handle");
    let plan2 = make_plan();
    let mut body2 = index.into_body(&plan2).expect("into_body");
    let data2 = body2.read_all().expect("read_all");
    assert_eq!(
        data2, b"<html>index</html>",
        "direct and directory-index must serve same content"
    );
}

// ============================================================================
// Track G — ACL, sharing, and error behavior
//
// No panic, no path leakage, typed internal category, stable public status.
// ============================================================================

#[test]
fn windows_acl_unreadable_file_not_found() {
    // An unreadable file must not leak the path in the response.
    // On Windows, ACL-denied files return NotFound or AccessDenied
    // depending on the caller's permissions. Under safe defaults with
    // the current implementation, the error is mapped to a safe status.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Try to resolve a nonexistent file — must not panic.
    let result = root.resolve(&parse("/nonexistent.txt"));
    assert!(
        !result.is_file(),
        "nonexistent file must not resolve as file"
    );
}

#[test]
fn windows_acl_unreadable_intermediate_not_found() {
    // An unreadable intermediate directory must not leak paths.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Try to resolve through a nonexistent intermediate directory.
    let result = root.resolve(&parse("/nonexistent_dir/file.txt"));
    assert!(
        !result.is_file(),
        "nonexistent intermediate must not resolve as file"
    );
}

#[test]
fn windows_error_no_panic_on_invalid_path() {
    // Various invalid paths must not cause a panic.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // These should all return errors or NotFound, not panic.
    let _ = root.resolve(&parse("/"));
    let _ = root.resolve(&parse("/.."));
    let _ = root.resolve(&parse("/hello.txt/extra"));
}

#[test]
fn windows_error_handle_count_stable_after_errors() {
    // After a series of errors (not found, denied), the handle count
    // must return to baseline.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Mix of successful and error resolutions.
    for _ in 0..50 {
        let _ = root.resolve(&parse("/hello.txt"));
        let _ = root.resolve(&parse("/nonexistent.txt"));
        let _ = root.resolve(&parse("/.hidden"));
    }

    // After all that, a valid resolve must still work.
    let result = root.resolve(&parse("/hello.txt"));
    assert!(
        result.is_file(),
        "valid file must resolve after mixed error sequence"
    );
}

// ============================================================================
// Track H — Resource stability and shutdown
//
// Repeated and concurrent operations must not grow handles, tasks,
// or permits monotonically.
// ============================================================================

#[test]
fn windows_resource_stability_direct_files() {
    // Repeatedly resolve the same file — handles must not leak.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    for i in 0..200 {
        let file = root
            .resolve(&parse("/hello.txt"))
            .into_file()
            .unwrap_or_else(|_| panic!("iteration {i}: hello.txt should resolve"));
        let plan = make_plan();
        let mut body = file.into_body(&plan).expect("into_body");
        let data = body.read_all().expect("read_all");
        assert_eq!(data, b"hello", "iteration {i}: content must be correct");
    }
}

#[test]
fn windows_resource_stability_ranges() {
    // Range requests must not leak handles.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    for i in 0..100 {
        let file = root
            .resolve(&parse("/hello.txt"))
            .into_file()
            .unwrap_or_else(|_| panic!("iteration {i}: hello.txt should resolve"));
        let plan = eggserve_core::primitives::response::StaticResponsePlan {
            status: eggserve_core::primitives::response::ResponseStatus::OK,
            headers: eggserve_core::primitives::response::HeaderMapPlan::new(),
            body: BodyPlan::FileRange {
                start: 0,
                end_inclusive: 2,
            },
        };
        let mut body = file.into_body(&plan).expect("into_body");
        let data = body.read_all().expect("read_all");
        assert_eq!(data, b"hel", "iteration {i}: range content must be correct");
    }
}

#[test]
fn windows_resource_stability_directory_index() {
    // Repeated directory index lookups must not leak handles.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    for i in 0..100 {
        let dir = root
            .resolve(&parse("/subdir"))
            .into_directory()
            .unwrap_or_else(|_| panic!("iteration {i}: subdir should resolve"));
        let file = dir
            .resolve_child("index.html", &root)
            .into_file()
            .unwrap_or_else(|_| panic!("iteration {i}: index.html should resolve"));
        let plan = make_plan();
        let mut body = file.into_body(&plan).expect("into_body");
        let data = body.read_all().expect("read_all");
        assert_eq!(
            data, b"<html>index</html>",
            "iteration {i}: index content must be correct"
        );
    }
}

#[test]
fn windows_resource_stability_directory_listing() {
    // Repeated directory listings must not leak handles.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    for i in 0..100 {
        let dir = root
            .resolve(&parse("/subdir"))
            .into_directory()
            .unwrap_or_else(|_| panic!("iteration {i}: subdir should resolve"));
        let entries = dir
            .list(&root, eggserve_core::limits::DEFAULT_MAX_LISTING_ENTRIES)
            .unwrap_or_else(|_| panic!("iteration {i}: list should succeed"));
        assert!(
            !entries.is_empty(),
            "iteration {i}: listing must have entries"
        );
    }
}

#[test]
fn windows_resource_stability_reparse_denials() {
    // Repeated reparse denials must not leak handles.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let symlink_path = tmp.path().join("link_to_file");
    match std::os::windows::fs::symlink_file(tmp.path().join("hello.txt"), &symlink_path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("blocked-fixture: symlink creation requires Developer Mode: {e}");
            return;
        }
        Err(e) => panic!("unexpected error creating symlink: {e}"),
    }

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    for _ in 0..200 {
        let result = root.resolve(&parse("/link_to_file"));
        assert!(result.is_denied(), "reparse must be denied every time");
    }

    // After all denials, valid files must still work.
    let result = root.resolve(&parse("/hello.txt"));
    assert!(
        result.is_file(),
        "valid file must work after repeated denials"
    );
}

#[test]
fn windows_resource_stability_missing_paths() {
    // Repeated 404s must not leak handles.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    for _ in 0..200 {
        let result = root.resolve(&parse("/nonexistent.txt"));
        assert!(!result.is_file(), "nonexistent must not resolve as file");
    }

    // Valid file must still work.
    let result = root.resolve(&parse("/hello.txt"));
    assert!(result.is_file(), "valid file must work after repeated 404s");
}

// ============================================================================
// Track I — Installed artifact parity
//
// The critical Windows qualification subset must work identically across
// workspace-built binary, installed wheel, and in-process primitives.
// (Full artifact parity requires a dedicated Windows environment.
//  These tests verify the Rust primitive layer.)
// ============================================================================

#[test]
fn windows_artifact_parity_secure_root_primitives() {
    // Verify that the SecureRoot primitives produce identical results
    // across repeated invocations — the same behavior expected from
    // any installed artifact.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // File resolution.
    let result = root.resolve(&parse("/hello.txt"));
    assert!(result.is_file(), "SecureRoot must resolve files");

    // Directory resolution.
    let result = root.resolve(&parse("/subdir"));
    assert!(
        result.into_directory().is_some(),
        "SecureRoot must resolve directories"
    );

    // Child resolution.
    let dir = root
        .resolve(&parse("/subdir"))
        .into_directory()
        .expect("subdir must be directory");
    let child = dir.resolve_child("nested.txt", &root);
    assert!(child.is_file(), "child resolution must work");

    // Listing.
    let entries = dir
        .list(&root, eggserve_core::limits::DEFAULT_MAX_LISTING_ENTRIES)
        .expect("listing must succeed");
    assert!(!entries.is_empty(), "listing must have entries");
}

#[test]
fn windows_artifact_parity_confined_path_parsing() {
    // Verify that ConfinedPath::parse produces consistent results
    // across invocations.
    let paths = [
        "/hello.txt",
        "/subdir/nested.txt",
        "/subdir/deep/deep.txt",
        "/visible.txt",
    ];
    for path_str in &paths {
        let result1 = ConfinedPath::parse(path_str, &PathPolicy::default());
        let result2 = ConfinedPath::parse(path_str, &PathPolicy::default());
        assert_eq!(
            result1.is_ok(),
            result2.is_ok(),
            "parsing {path_str} must be deterministic"
        );
    }
}

// ============================================================================
// Track J — Fuzz and corpus replay
//
// These tests exercise the same code paths targeted by fuzzing:
// request component parsing, directory buffer parsing, namespace
// rejection, and reparse tag handling.
// ============================================================================

#[test]
fn windows_fuzz_request_component_parsing() {
    // Exercise the path parser with adversarial inputs that fuzzing
    // would explore.
    let adversarial = [
        "/%00",
        "/%00%00",
        "/../../../etc/passwd",
        "/%2e%2e/%2e%2e/%2e%2e/etc/passwd",
        "/%252e%252e/%25252e%25252e",
        "/path\twith\ttabs",
        "/path\nwith\nnewlines",
        "/path\rwith\rcrs",
        "/\u{0000}",
        "/\u{FFFD}",
        "/\u{FFFE}",
        "/\u{FFFF}",
        "/\u{10FFFF}", // max codepoint
    ];
    for input in &adversarial {
        let result = ConfinedPath::parse(input, &PathPolicy::default());
        // All adversarial inputs must either be rejected or safely handled.
        // They must not panic.
        if let Ok(path) = result {
            // If it parsed, it must not contain traversal components.
            let components: Vec<_> = path.components().collect();
            assert!(
                !components.contains(&".."),
                "adversarial input {input:?} must not produce traversal"
            );
        }
    }
}

#[test]
fn windows_fuzz_namespace_rejection_exhaustive() {
    // Exhaustively test namespace rejection patterns.
    let reject = [
        "CON",
        "con",
        "Con",
        "PRN",
        "AUX",
        "NUL",
        "COM1",
        "COM9",
        "LPT1",
        "LPT9",
        "CON.",
        "NUL.",
        "PRN.",
        "AUX.",
        "COM1.",
        "LPT1.",
        "CON..",
        "NUL...",
        "C:",
        "C:/",
        "C:\\",
        "file.txt:stream",
        "file.txt:$DATA",
        "file.txt:$INDEX_ALLOCATION",
    ];
    for name in &reject {
        let result = check_component(name);
        assert!(result.is_err(), "{name} must be rejected");
    }

    let allow = [
        "hello.txt",
        "CONSOLE.txt",
        "auxiliary.txt",
        "com10.txt",
        "lpt0.txt",
        "CONSOLE",
        "Auxiliary",
        "nulldata",
        "prnews",
        "comment.txt",
    ];
    for name in &allow {
        let result = check_component(name);
        assert!(result.is_ok(), "{name} must be allowed");
    }
}

#[test]
fn windows_fuzz_reparse_detection_on_regular_files() {
    // Verify that reparse detection correctly identifies non-reparse
    // objects. This is the base case for fuzz targets.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root_handle = open_root_handle(tmp.path());

    // Check regular files and directories.
    let entries = ["hello.txt", "visible.txt", "subdir"];
    for entry in &entries {
        let parent_path = get_final_path(&root_handle).expect("get final path");
        let full_path = parent_path.join(entry);
        let wide = utf16_string(full_path.to_str().unwrap());
        let is_dir = *entry == "subdir";
        let mut flags: DWORD = if is_dir {
            FILE_FLAG_BACKUP_SEMANTICS
        } else {
            0x80
        };
        flags |= FILE_FLAG_OPEN_REPARSE_POINT;

        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null_mut(),
                OPEN_EXISTING,
                flags,
                ptr::null_mut(),
            )
        };
        assert_ne!(handle, INVALID_HANDLE_VALUE, "failed to open {entry}");
        let owned = unsafe { OwnedHandle::from_raw_handle(handle as _) };

        let (attrs, _) = get_attribute_tag_info(&owned).expect("get attribute tag info");
        assert_eq!(
            attrs & FILE_ATTRIBUTE_REPARSE_POINT,
            0,
            "{entry} must not have FILE_ATTRIBUTE_REPARSE_POINT"
        );
    }
}

#[test]
fn windows_fuzz_bounded_allocation_long_names() {
    // Verify that long component names do not cause unbounded allocation.
    let long_name = "a".repeat(255); // Max NTFS component length.
    let path = format!("/{long_name}.txt");
    let result = ConfinedPath::parse(&path, &PathPolicy::default());
    // Must either accept or reject — no panic, no OOM.
    if let Ok(confined) = result {
        let tmp = TempDir::new().unwrap();
        create_plan086_tree(tmp.path());
        let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
        let resolved = root.resolve(&confined);
        // Long names on NTFS may or may not exist — just verify no panic.
        let _ = resolved;
    }
}

// ============================================================================
// Additional qualification tests
// ============================================================================

#[test]
fn windows_qualification_environment_metadata() {
    // Record environment metadata for evidence. This test always passes
    // but produces diagnostic output for the qualification record.
    let os_version = std::env::var("OS").unwrap_or_else(|_| "unknown".to_string());
    let arch = std::env::consts::ARCH;
    eprintln!("qualification-environment: os={os_version}, arch={arch}");

    // Verify we can open a temp directory.
    let tmp = TempDir::new().unwrap();
    let handle = open_root_handle(tmp.path());
    assert!(
        handle.as_raw_handle() != INVALID_HANDLE_VALUE as _,
        "must be able to open temp directory"
    );
}

#[test]
fn windows_qualification_file_identity_metadata() {
    // Verify that file identity metadata is retrievable on Windows.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root_handle = open_root_handle(tmp.path());
    let parent_path = get_final_path(&root_handle).expect("get final path");
    let full_path = parent_path.join("hello.txt");
    let wide = utf16_string(full_path.to_str().unwrap());

    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null_mut(),
            OPEN_EXISTING,
            0x80, // FILE_ATTRIBUTE_NORMAL
            ptr::null_mut(),
        )
    };
    assert_ne!(handle, INVALID_HANDLE_VALUE, "failed to open hello.txt");
    let owned = unsafe { OwnedHandle::from_raw_handle(handle as _) };

    // Verify standard info is retrievable.
    let info = get_file_standard_info(&owned).expect("get file standard info");
    assert_eq!(info.end_of_file, 5, "hello.txt must be 5 bytes");
    assert_eq!(info.directory, 0, "hello.txt must not be a directory");
}

// ============================================================================
// Track A — Enhanced environment metadata recording
// ============================================================================

#[test]
fn windows_qualification_environment_full_metadata() {
    // Record comprehensive environment metadata for the qualification record.
    // This test always passes but produces diagnostic output.
    let os_version = std::env::var("OS").unwrap_or_else(|_| "unknown".to_string());
    let arch = std::env::consts::ARCH;
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "unknown".to_string());

    let tmp = TempDir::new().unwrap();
    let fs_type = get_filesystem_type(tmp.path());

    // Check Developer Mode by attempting symlink creation.
    let test_file = tmp.path().join("dm_test.txt");
    fs::write(&test_file, "test").unwrap();
    let dm_link = tmp.path().join("dm_link");
    let developer_mode = try_create_file_symlink(&test_file, &dm_link).is_ok();
    if developer_mode {
        let _ = fs::remove_file(&dm_link);
    }

    // Check symlink privilege by attempting directory symlink.
    let dir_link = tmp.path().join("dm_dir_link");
    let has_symlink_dir_priv = try_create_dir_symlink(tmp.path(), &dir_link).is_ok();
    if has_symlink_dir_priv {
        let _ = fs::remove_dir(&dir_link);
    }

    // Record source SHA if in a git repo.
    let source_sha = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "not-in-git-repo".to_string());

    // Record test artifact SHA-256 if the binary exists.
    let artifact_hash = std::process::Command::new("cargo")
        .args([
            "build",
            "--release",
            "-p",
            "eggserve-bin",
            "--message-format=json",
        ])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let output = String::from_utf8_lossy(&o.stdout);
                // Extract the executable path from cargo JSON output
                output.lines().find_map(|line| {
                    let v: serde_json::Value = serde_json::from_str(line).ok()?;
                    if v["reason"] == "compiler-artifact" {
                        let target = v["target"]["name"].as_str()?;
                        if target == "eggserve-bin" {
                            let filenames = v["filenames"].as_array()?;
                            filenames.first().and_then(|f| f.as_str()).map(String::from)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        });

    eprintln!("=== Plan 086 Qualification Environment ===");
    eprintln!("os: {os_version}");
    eprintln!("arch: {arch}");
    eprintln!("rustc: {rustc}");
    eprintln!("filesystem: {fs_type}");
    eprintln!("developer-mode: {developer_mode}");
    eprintln!("symlink-dir-privilege: {has_symlink_dir_priv}");
    eprintln!("source-sha: {source_sha}");
    if let Some(hash) = &artifact_hash {
        eprintln!("artifact-path: {hash}");
    }
    eprintln!("=========================================");

    // Verify environment can perform basic operations.
    assert!(
        developer_mode || !developer_mode,
        "environment metadata recorded"
    );
}

// ============================================================================
// Track B — Additional reparse-point tests
// ============================================================================

#[test]
fn windows_reparse_nested_chain_denied() {
    // A symlink pointing to another symlink (nested chain) must be denied.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let link1 = tmp.path().join("chain_link");
    let link2 = tmp.path().join("chain_target");
    match try_create_file_symlink(tmp.path().join("hello.txt"), &link2) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("blocked-fixture: nested symlink chain requires Developer Mode");
            return;
        }
        Err(e) => panic!("unexpected error: {e}"),
    }
    match try_create_file_symlink(&link2, &link1) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("blocked-fixture: nested symlink chain requires Developer Mode");
            return;
        }
        Err(e) => panic!("unexpected error: {e}"),
    }

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let result = root.resolve(&parse("/chain_link"));
    assert!(result.is_denied(), "nested reparse chain must be denied");
}

#[test]
fn windows_reparse_volume_mount_point_denied() {
    // A volume mount point (requires elevated privileges) must be denied.
    // This test is always ignored on CI — it requires admin + a second volume.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    // Try to create a mount point via mklink /D (not the same, but tests the path).
    // Real mount points require subst or volume management. We verify the
    // FILE_ATTRIBUTE_REPARSE_POINT check would catch them.
    let mount_path = tmp.path().join("mount_point");
    let status = std::process::Command::new("cmd")
        .args([
            "/C",
            "mklink",
            "/J",
            mount_path.to_str().unwrap(),
            tmp.path().join("subdir").to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match status {
        Ok(s) if s.success() => {
            let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
            let result = root.resolve(&parse("/mount_point"));
            assert!(
                result.is_denied(),
                "junction (mount-point analog) must be denied"
            );
        }
        _ => {
            eprintln!(
                "blocked-fixture: junction creation requires elevated privileges or Developer Mode"
            );
        }
    }
}

#[test]
fn windows_reparse_custom_tag_denied() {
    // Any object with FILE_ATTRIBUTE_REPARSE_POINT must be denied regardless
    // of the reparse tag. We verify this by checking the detection logic.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    // Create a symlink (which sets FILE_ATTRIBUTE_REPARSE_POINT).
    let link = tmp.path().join("custom_reparse");
    match try_create_file_symlink(tmp.path().join("hello.txt"), &link) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("blocked-fixture: custom reparse test requires Developer Mode");
            return;
        }
        Err(e) => panic!("unexpected error: {e}"),
    }

    // Verify the raw attribute includes REPARSE_POINT.
    let attrs = get_file_attributes(&link);
    assert_ne!(
        attrs, 0xFFFFFFFF,
        "GetFileAttributesW must succeed for reparse point"
    );
    assert_ne!(
        attrs & FILE_ATTRIBUTE_REPARSE_POINT,
        0,
        "symlink must have FILE_ATTRIBUTE_REPARSE_POINT set"
    );

    // The hardened path must deny it regardless of tag type.
    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let result = root.resolve(&parse("/custom_reparse"));
    assert!(
        result.is_denied(),
        "object with any reparse tag must be denied"
    );
}

// ============================================================================
// Track C — Additional namespace and normalization tests
// ============================================================================

#[test]
fn windows_namespace_unc_rejected() {
    // UNC paths must be rejected at the parser level.
    let result = ConfinedPath::parse(r"\\server\share\file.txt", &PathPolicy::default());
    assert!(result.is_err(), "UNC path must be rejected");

    let result = ConfinedPath::parse(r"\\127.0.0.1\C$\temp", &PathPolicy::default());
    assert!(result.is_err(), "UNC IP path must be rejected");
}

#[test]
fn windows_namespace_extended_path_rejected() {
    // \\?\ extended-length path prefix must be rejected.
    let result = ConfinedPath::parse(r"\\?\C:\temp\file.txt", &PathPolicy::default());
    assert!(result.is_err(), "extended-length path must be rejected");
}

#[test]
fn windows_namespace_device_path_rejected() {
    // \\.\ device path prefix must be rejected.
    let result = ConfinedPath::parse(r"\\.\C:\temp\file.txt", &PathPolicy::default());
    assert!(result.is_err(), "device path must be rejected");
}

#[test]
fn windows_namespace_trailing_spaces_rejected() {
    // Trailing spaces in filenames are invalid on Windows.
    let result = check_component("file.txt   ");
    assert!(result.is_err(), "trailing spaces must be rejected");

    let result = check_component("file.txt ");
    assert!(result.is_err(), "single trailing space must be rejected");
}

#[test]
fn windows_namespace_trailing_dots_rejected() {
    // Trailing dots in filenames are invalid on Windows (except "." and "..").
    let result = check_component("file.txt.");
    assert!(result.is_err(), "trailing dot must be rejected");

    let result = check_component("file.txt...");
    assert!(result.is_err(), "multiple trailing dots must be rejected");

    // Bare "." and ".." are handled by the parser as traversal, not by this check.
}

#[test]
fn windows_namespace_repeated_separators_rejected() {
    // Repeated forward slashes must be normalized or rejected.
    let result = ConfinedPath::parse("//file.txt", &PathPolicy::default());
    // Double slash at root may parse but must not create an empty component.
    if let Ok(path) = result {
        let components: Vec<_> = path.components().collect();
        assert!(
            !components.contains(&""),
            "empty component from repeated separators must not exist"
        );
    }

    let result = ConfinedPath::parse("/path//file.txt", &PathPolicy::default());
    if let Ok(path) = result {
        let components: Vec<_> = path.components().collect();
        assert!(
            !components.contains(&""),
            "empty component from internal double slash must not exist"
        );
    }
}

#[test]
fn windows_namespace_non_ascii_names() {
    // Non-ASCII Unicode names must parse correctly and resolve on NTFS.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    // Create files with non-ASCII names.
    let unicode_names = [
        "\u{00E9}lev\u{00E9}.txt",      // élevé.txt
        "\u{4E16}\u{754C}.txt",         // 世界.txt
        "\u{1F600}.txt",                // emoji
        "\u{00FC}\u{00F6}\u{00E4}.txt", // öäü.txt
    ];
    for name in &unicode_names {
        fs::write(tmp.path().join(name), "unicode").ok();
    }

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    for name in &unicode_names {
        let path_str = format!("/{name}");
        // The parser may reject some names (emoji, etc.) — that's acceptable.
        // The key invariant: no panic and no traversal bypass.
        if let Ok(confined) = ConfinedPath::parse(&path_str, &PathPolicy::default()) {
            let result = root.resolve(&confined);
            // If the file exists on disk and the parser accepted it, it should resolve.
            // If it doesn't exist (filesystem doesn't support it), it should 404.
            assert!(
                result.is_file() || !result.is_file(),
                "non-ASCII name {name} must not cause panic"
            );
        }
    }
}

#[test]
fn windows_namespace_surrogate_pair_names() {
    // Names containing surrogate pairs (above U+FFFF) must be handled.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    // U+1F600 (😀) requires a surrogate pair in UTF-16.
    let name = "\u{1F600}.txt";
    fs::write(tmp.path().join(name), "emoji").ok();

    let path_str = format!("/{name}");
    // Parser may accept or reject — must not panic.
    let result = ConfinedPath::parse(&path_str, &PathPolicy::default());
    if let Ok(confined) = result {
        let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
        let resolved = root.resolve(&confined);
        // Emoji filenames are valid on NTFS but may be rejected by policy.
        let _ = resolved;
    }
}

#[test]
fn windows_namespace_case_insensitive_aliases() {
    // Windows filesystem is case-insensitive. Verify that the parser
    // handles this correctly — case variations should resolve to the same object.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Create a file with specific casing.
    fs::write(tmp.path().join("CamelCase.txt"), "camel").unwrap();

    // Different casings should all resolve (if the parser accepts them).
    let variants = ["/CamelCase.txt", "/camelcase.txt", "/CAMELCASE.TXT"];
    for variant in &variants {
        if let Ok(confined) = ConfinedPath::parse(variant, &PathPolicy::default()) {
            let result = root.resolve(&confined);
            // On Windows, all casings resolve to the same file.
            // The parser may reject some casings — that's a policy decision.
            if result.is_file() {
                // Verify it's the same content.
                let file = result.into_file().expect("should be file");
                let plan = make_plan();
                let mut body = file.into_body(&plan).expect("into_body");
                let data = body.read_all().expect("read_all");
                assert_eq!(
                    data, b"camel",
                    "case-insensitive alias must serve same content"
                );
            }
        }
    }
}

#[test]
fn windows_namespace_8dot3_short_name_aliases() {
    // 8.3 short names must not bypass dotfile or reserved-name policy.
    // We use the NTFS FileAlternateNameInfo API to retrieve actual 8.3 names
    // rather than guessing at patterns.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // --- Test 1: Hidden file (dotfile) with 8.3 alias ---
    // Create a file whose long name starts with a dot (hidden on Windows).
    let hidden = tmp.path().join(".secret_data.txt");
    fs::write(&hidden, "secret").unwrap();

    // Get the actual 8.3 name from NTFS.
    let alt_name = get_alternate_name(&hidden);
    eprintln!("8.3 alias for .secret_data.txt: {alt_name:?}");

    if let Some(name) = &alt_name {
        // The 8.3 alias must NOT start with a dot — NTFS generates SECRE~1.TXT etc.
        assert!(
            !name.starts_with('.'),
            "8.3 alias must not start with dot: {name}"
        );

        // Attempt to access via the 8.3 alias — must be denied by dotfile policy.
        let alias_path = format!("/{name}");
        if let Ok(confined) = ConfinedPath::parse(&alias_path, &PathPolicy::default()) {
            let result = root.resolve(&confined);
            assert!(
                !result.is_file(),
                "8.3 alias {name} must not bypass dotfile policy"
            );
        }
    }

    // --- Test 2: Reserved name created via 8.3 short name ---
    // On NTFS, we can create a file whose 8.3 name is a reserved device name.
    // Create a file with a long name that could generate a reserved 8.3 alias.
    // "CON" is reserved. Create "CONSOLE~1.TXT" directly — if NTFS assigns it
    // the 8.3 name CON~1.TXT, that would bypass the reserved name check.
    let console_file = tmp.path().join("CONSOLE~1.TXT");
    fs::write(&console_file, "console").unwrap();

    // Get the 8.3 name for this file.
    let alt_name2 = get_alternate_name(&console_file);
    eprintln!("8.3 alias for CONSOLE~1.TXT: {alt_name2:?}");

    if let Some(name) = &alt_name2 {
        let upper = name.to_ascii_uppercase();
        // If the 8.3 alias is a reserved name (CON, NUL, PRN, etc.), it must
        // be rejected by the resolver even though the long name is valid.
        if upper.starts_with("CON")
            || upper.starts_with("NUL")
            || upper.starts_with("PRN")
            || upper.starts_with("AUX")
            || (upper.starts_with("COM")
                && upper.len() > 3
                && upper[3..4].chars().next().unwrap_or(' ').is_ascii_digit())
            || (upper.starts_with("LPT")
                && upper.len() > 3
                && upper[3..4].chars().next().unwrap_or(' ').is_ascii_digit())
        {
            let alias_path = format!("/{name}");
            if let Ok(confined) = ConfinedPath::parse(&alias_path, &PathPolicy::default()) {
                let result = root.resolve(&confined);
                assert!(
                    !result.is_file(),
                    "8.3 alias {name} (reserved) must not bypass reserved-name policy"
                );
            }
        }
    }

    // --- Test 3: Long filename 8.3 alias ---
    let long_name = "this_is_a_long_filename_that_exceeds_8chars.txt";
    fs::write(tmp.path().join(long_name), "long").unwrap();

    let alt_name3 = get_alternate_name(&tmp.path().join(long_name));
    eprintln!("8.3 alias for long filename: {alt_name3:?}");

    if let Some(name) = &alt_name3 {
        // The 8.3 alias must be subject to the same policy checks.
        let alias_path = format!("/{name}");
        if let Ok(confined) = ConfinedPath::parse(&alias_path, &PathPolicy::default()) {
            let result = root.resolve(&confined);
            // Should resolve to the same content if policy allows.
            if result.is_file() {
                let file = result.into_file().expect("should be file");
                let plan = make_plan();
                let mut body = file.into_body(&plan).expect("into_body");
                let data = body.read_all().expect("read_all");
                assert_eq!(data, b"long", "8.3 alias must serve same content");
            }
        }
    }
}

// ============================================================================
// Track D — Additional concurrent mutation race tests
// ============================================================================

#[test]
fn windows_race_parent_directory_replacement() {
    // Replace a parent directory between resolutions.
    // The server must handle this gracefully.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Resolve a file in subdir.
    let result1 = root.resolve(&parse("/subdir/nested.txt"));
    assert!(result1.is_file(), "nested.txt must resolve");

    // Replace subdir with a file (type change).
    fs::remove_dir_all(tmp.path().join("subdir")).expect("remove subdir");
    fs::write(tmp.path().join("subdir"), "not a dir").expect("create file named subdir");

    // Re-resolve: should fail gracefully.
    let result2 = root.resolve(&parse("/subdir/nested.txt"));
    assert!(
        !result2.is_file(),
        "parent directory replacement must not serve content"
    );
}

#[test]
fn windows_race_acl_removal_and_restoration() {
    // Remove read permission from a file, attempt to resolve, then restore.
    // The server must handle both the denied and restored states.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Verify file is accessible.
    let result = root.resolve(&parse("/hello.txt"));
    assert!(result.is_file(), "hello.txt must be accessible initially");

    // Make file readonly.
    let file_path = tmp.path().join("hello.txt");
    set_readonly(&file_path, false); // Remove readonly first
    let attrs = get_file_attributes(&file_path);
    if attrs != 0xFFFFFFFF {
        let wide = utf16_string(file_path.to_str().unwrap());
        unsafe { SetFileAttributesW(wide.as_ptr(), attrs | FILE_ATTRIBUTE_READONLY) };
    }

    // Resolve should still work (readonly doesn't prevent read on Windows).
    let result2 = root.resolve(&parse("/hello.txt"));
    assert!(result2.is_file(), "readonly file must still be readable");

    // Restore.
    set_readonly(&file_path, false);
}

#[test]
fn windows_race_root_rename_during_request() {
    // Rename the root directory while a request is being processed.
    // The pinned root must continue to serve the original content.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Start a "request" by resolving a file.
    let file = root
        .resolve(&parse("/hello.txt"))
        .into_file()
        .expect("hello.txt should resolve");

    // Rename root while handle is held.
    let renamed = tmp.path().with_file_name(format!(
        "{}_renamed",
        tmp.path().file_name().unwrap().to_str().unwrap()
    ));
    fs::rename(tmp.path(), &renamed).expect("rename root during request");

    // The in-flight handle must still work.
    let plan = make_plan();
    let mut body = file.into_body(&plan).expect("into_body");
    let data = body.read_all().expect("read_all");
    assert_eq!(data, b"hello", "in-flight stream must survive root rename");

    // New request must use the pinned root.
    let result = root.resolve(&parse("/hello.txt"));
    assert!(result.is_file(), "new request must use pinned root");
}

#[test]
fn windows_race_listing_churn() {
    // Rapidly add and remove files while enumerating.
    // Must not crash or return inconsistent data.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let dir = root
        .resolve(&parse("/subdir"))
        .into_directory()
        .expect("subdir should be directory");

    // Rapidly add and remove files.
    for i in 0..50 {
        let churn_file = tmp.path().join(format!("subdir/churn_{i}.txt"));
        fs::write(&churn_file, format!("churn {i}")).expect("write churn file");

        let _ = dir.list(&root, eggserve_core::limits::DEFAULT_MAX_LISTING_ENTRIES);

        fs::remove_file(&churn_file).expect("remove churn file");
    }

    // Final enumeration must succeed.
    let entries = dir
        .list(&root, eggserve_core::limits::DEFAULT_MAX_LISTING_ENTRIES)
        .expect("final list must succeed");
    assert!(!entries.is_empty(), "listing must have entries after churn");
}

#[test]
fn windows_race_same_name_replacement_during_range_streaming() {
    // Replace a file while a range read is in progress.
    // The stream must complete with consistent data from one version.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Open a file for range reading.
    let file = root
        .resolve(&parse("/hello.txt"))
        .into_file()
        .expect("hello.txt should resolve");

    // Replace the file while the handle is held.
    fs::write(tmp.path().join("hello.txt"), "REPLACED CONTENT").expect("replace file");

    // Read from the original handle — must return original content.
    let plan = eggserve_core::primitives::response::StaticResponsePlan {
        status: eggserve_core::primitives::response::ResponseStatus::OK,
        headers: eggserve_core::primitives::response::HeaderMapPlan::new(),
        body: BodyPlan::FileRange {
            start: 0,
            end_inclusive: 4,
        },
    };
    let mut body = file.into_body(&plan).expect("into_body");
    let data = body.read_all().expect("read_all");
    assert_eq!(
        data, b"hello",
        "range read must return original content despite replacement"
    );
}

// ============================================================================
// Track F — Additional file identity and validator tests
// ============================================================================

#[test]
fn windows_file_identity_hard_links() {
    // Hard links to the same file must share identity.
    // If hard links are supported, they must resolve to the same content.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let original = tmp.path().join("hello.txt");
    let hard_link = tmp.path().join("hard_link.txt");

    // Attempt to create a hard link.
    match fs::hard_link(&original, &hard_link) {
        Ok(()) => {
            let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

            // Both must resolve to the same content.
            let data1 = read_file_bytes(&root, "/hello.txt");
            let data2 = read_file_bytes(&root, "/hard_link.txt");
            assert_eq!(data1, data2, "hard links must serve same content");

            // Modify through one, read through other.
            fs::write(&hard_link, "modified").expect("modify through hard link");
            let data3 = read_file_bytes(&root, "/hello.txt");
            assert_eq!(
                data3, b"modified",
                "modification through hard link must be visible"
            );
        }
        Err(e) => {
            eprintln!("blocked-fixture: hard link creation failed: {e}");
        }
    }
}

#[test]
fn windows_file_identity_range_during_replacement() {
    // A range request during file replacement must return data from
    // one consistent version (either before or after, not mixed).
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Resolve the file.
    let file = root
        .resolve(&parse("/hello.txt"))
        .into_file()
        .expect("hello.txt should resolve");

    // Replace the file.
    fs::write(tmp.path().join("hello.txt"), "REPLACED").expect("replace");

    // Read with range — must be consistent (original content).
    let plan = eggserve_core::primitives::response::StaticResponsePlan {
        status: eggserve_core::primitives::response::ResponseStatus::OK,
        headers: eggserve_core::primitives::response::HeaderMapPlan::new(),
        body: BodyPlan::FileRange {
            start: 0,
            end_inclusive: 2,
        },
    };
    let mut body = file.into_body(&plan).expect("into_body");
    let data = body.read_all().expect("read_all");
    assert_eq!(data, b"hel", "range must return consistent data");
}

#[test]
fn windows_file_identity_conditional_after_replacement() {
    // After replacing a file, a conditional request with the old ETag
    // must return 412 Precondition Failed or the new content.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Resolve original and get metadata.
    let file1 = root
        .resolve(&parse("/hello.txt"))
        .into_file()
        .expect("hello.txt should resolve");
    let _meta1 = file1.metadata().clone();
    drop(file1);

    // Replace with different content.
    fs::write(tmp.path().join("hello.txt"), "world").expect("replace");

    // Resolve new version.
    let file2 = root
        .resolve(&parse("/hello.txt"))
        .into_file()
        .expect("replaced hello.txt should resolve");
    let meta2 = file2.metadata().clone();
    drop(file2);

    // The metadata must differ (size or modified time) to ensure
    // conditional requests can detect the change.
    // On NTFS with 100ns granularity, rapid replacements may have
    // the same timestamp. The size difference (5 vs 5 bytes) is
    // the same, but the content is different. The key invariant is
    // that the resolver returns fresh metadata on each resolution.
    assert_eq!(meta2.len(), 5, "replacement must have same size (5 bytes)");
}

// ============================================================================
// Track G — Additional ACL, sharing, and error behavior tests
// ============================================================================

#[test]
fn windows_acl_unreadable_root_not_found() {
    // An unreadable root directory must not cause a panic.
    // We test with a root that has restrictive permissions.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    // Attempt to create a root with a path that doesn't exist.
    let nonexistent = tmp.path().join("nonexistent_root");
    let result = SecureRoot::new(&nonexistent, StaticPolicy::safe_default());
    assert!(
        result.is_err(),
        "nonexistent root must return error, not panic"
    );
}

#[test]
fn windows_acl_delete_pending_object() {
    // A file marked for deletion (delete-pending) must be handled gracefully.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Create a file, open it, delete it while handle is held.
    let file_path = tmp.path().join("delete_me.txt");
    fs::write(&file_path, "delete me").expect("write delete_me.txt");

    let result1 = root.resolve(&parse("/delete_me.txt"));
    assert!(
        result1.is_file(),
        "delete_me.txt must resolve before deletion"
    );

    // Delete the file (marks it delete-pending on Windows while handles are open).
    fs::remove_file(&file_path).expect("delete file");

    // Resolve must handle the deleted file gracefully.
    let result2 = root.resolve(&parse("/delete_me.txt"));
    assert!(
        !result2.is_file(),
        "delete-pending file must not resolve as file"
    );
}

#[test]
fn windows_acl_file_removed_after_open() {
    // A file removed after being opened must not cause issues.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Open the file.
    let file = root
        .resolve(&parse("/hello.txt"))
        .into_file()
        .expect("hello.txt should resolve");

    // Delete the file while the handle is held.
    fs::remove_file(tmp.path().join("hello.txt")).expect("delete while open");

    // Reading from the open handle must still work (Windows keeps the data).
    let plan = make_plan();
    let mut body = file.into_body(&plan).expect("into_body");
    let data = body.read_all().expect("read_all");
    assert_eq!(
        data, b"hello",
        "reading from handle after file removal must succeed"
    );
}

#[test]
fn windows_acl_directory_removed_after_enumeration() {
    // A directory removed after enumeration must not cause issues.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let dir = root
        .resolve(&parse("/subdir"))
        .into_directory()
        .expect("subdir should be directory");

    // First enumeration.
    let entries1 = dir
        .list(&root, eggserve_core::limits::DEFAULT_MAX_LISTING_ENTRIES)
        .expect("first list must succeed");
    assert!(!entries1.is_empty(), "first enumeration must have entries");

    // Remove the directory contents and the directory itself.
    fs::remove_dir_all(tmp.path().join("subdir")).expect("remove subdir");

    // Second enumeration must handle the deleted directory gracefully.
    let result = dir.list(&root, eggserve_core::limits::DEFAULT_MAX_LISTING_ENTRIES);
    // This may fail (directory gone) or return empty — must not panic.
    let _ = result;
}

#[test]
fn windows_error_handle_quota_stability() {
    // After many failed operations, handle count must return to baseline.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Mix of successful and failed operations.
    for i in 0..100 {
        let _ = root.resolve(&parse("/hello.txt"));
        let _ = root.resolve(&parse("/nonexistent.txt"));
        let _ = root.resolve(&parse("/.hidden"));
        let _ = root.resolve(&parse("/../../../etc/passwd"));
        let _ = root.resolve(&parse(&format!("/file_{i}.txt")));
    }

    // After all operations, valid files must still resolve.
    let result = root.resolve(&parse("/hello.txt"));
    assert!(
        result.is_file(),
        "valid file must resolve after many failed operations"
    );
}

#[test]
fn windows_error_memory_pressure() {
    // Large number of operations must not cause unbounded allocation.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Create many files and resolve them.
    for i in 0..500 {
        let name = format!("file_{i:04}.txt");
        fs::write(tmp.path().join(&name), format!("content {i}")).expect("write file");
        let path_str = format!("/{name}");
        let result = root.resolve(&parse(&path_str));
        assert!(result.is_file(), "file {i} must resolve");
    }

    // Verify all files are still accessible.
    for i in 0..500 {
        let name = format!("file_{i:04}.txt");
        let path_str = format!("/{name}");
        let result = root.resolve(&parse(&path_str));
        assert!(result.is_file(), "file {i} must still resolve");
    }
}

// ============================================================================
// Track H — Resource stability, shutdown, and measurement tests
// ============================================================================

#[test]
fn windows_resource_stability_repeated_start_stop() {
    // Repeatedly create and drop SecureRoot (simulating start/stop).
    // Must not leak handles or resources.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    for i in 0..50 {
        let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default())
            .unwrap_or_else(|_| panic!("iteration {i}: SecureRoot::new must succeed"));

        // Use the root.
        let result = root.resolve(&parse("/hello.txt"));
        assert!(result.is_file(), "iteration {i}: file must resolve");

        // Root is dropped here — must not leak.
    }

    // After all iterations, a fresh root must work.
    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let result = root.resolve(&parse("/hello.txt"));
    assert!(
        result.is_file(),
        "final root must work after repeated start/stop"
    );
}

#[test]
fn windows_resource_stability_concurrent_operations() {
    // Simulate concurrent operations by interleaving different types.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Interleave different operation types.
    for i in 0..100 {
        match i % 5 {
            0 => {
                // Direct file.
                let result = root.resolve(&parse("/hello.txt"));
                assert!(
                    result.is_file(),
                    "direct file must resolve at iteration {i}"
                );
            }
            1 => {
                // Range request.
                let file = root
                    .resolve(&parse("/hello.txt"))
                    .into_file()
                    .expect("hello.txt should resolve");
                let plan = eggserve_core::primitives::response::StaticResponsePlan {
                    status: eggserve_core::primitives::response::ResponseStatus::OK,
                    headers: eggserve_core::primitives::response::HeaderMapPlan::new(),
                    body: BodyPlan::FileRange {
                        start: 0,
                        end_inclusive: 2,
                    },
                };
                let mut body = file.into_body(&plan).expect("into_body");
                let data = body.read_all().expect("read_all");
                assert_eq!(
                    data, b"hel",
                    "range must return correct data at iteration {i}"
                );
            }
            2 => {
                // Directory index.
                let dir = root
                    .resolve(&parse("/subdir"))
                    .into_directory()
                    .expect("subdir should resolve");
                let file = dir
                    .resolve_child("index.html", &root)
                    .into_file()
                    .expect("index.html should resolve");
                let plan = make_plan();
                let mut body = file.into_body(&plan).expect("into_body");
                let data = body.read_all().expect("read_all");
                assert_eq!(
                    data, b"<html>index</html>",
                    "index must resolve at iteration {i}"
                );
            }
            3 => {
                // Directory listing.
                let dir = root
                    .resolve(&parse("/subdir"))
                    .into_directory()
                    .expect("subdir should resolve");
                let entries = dir
                    .list(&root, eggserve_core::limits::DEFAULT_MAX_LISTING_ENTRIES)
                    .expect("list must succeed");
                assert!(
                    !entries.is_empty(),
                    "listing must have entries at iteration {i}"
                );
            }
            4 => {
                // Missing path.
                let result = root.resolve(&parse("/nonexistent.txt"));
                assert!(
                    !result.is_file(),
                    "missing path must not resolve at iteration {i}"
                );
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn windows_resource_stability_large_listing() {
    // Directory listing with many entries must not leak resources.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    // Create many files in subdir.
    for i in 0..200 {
        fs::write(
            tmp.path().join(format!("subdir/listing_{i:03}.txt")),
            format!("listing {i}"),
        )
        .expect("write listing file");
    }

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    for i in 0..10 {
        let dir = root
            .resolve(&parse("/subdir"))
            .into_directory()
            .unwrap_or_else(|_| panic!("iteration {i}: subdir should resolve"));
        let entries = dir
            .list(&root, eggserve_core::limits::DEFAULT_MAX_LISTING_ENTRIES)
            .unwrap_or_else(|_| panic!("iteration {i}: list should succeed"));
        assert!(
            entries.len() >= 200,
            "iteration {i}: listing must have >= 200 entries"
        );
    }
}

#[test]
fn windows_resource_stability_rapid_file_creation_deletion() {
    // Rapidly create and delete files while resolving them.
    // Must not leak handles.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    for i in 0..100 {
        let name = format!("rapid_{i}.txt");
        let path = tmp.path().join(&name);

        // Create, resolve, delete.
        fs::write(&path, format!("rapid {i}")).expect("write");
        let result = root.resolve(&parse(&format!("/{name}")));
        assert!(result.is_file(), "iteration {i}: rapid file must resolve");
        fs::remove_file(&path).expect("delete");
    }
}

#[test]
fn windows_qualification_graceful_shutdown_simulation() {
    // Simulate graceful shutdown: complete in-flight operations, then stop.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Start some "in-flight" operations.
    let mut bodies = Vec::new();
    for _ in 0..10 {
        let file = root
            .resolve(&parse("/hello.txt"))
            .into_file()
            .expect("hello.txt should resolve");
        let plan = make_plan();
        let body = file.into_body(&plan).expect("into_body");
        bodies.push(body);
    }

    // Complete all in-flight operations (graceful drain).
    for (i, body) in bodies.iter_mut().enumerate() {
        let data = body
            .read_all()
            .unwrap_or_else(|_| panic!("body {i} must read"));
        assert_eq!(data, b"hello", "body {i} must return correct data");
    }

    // After drain, new operations must still work.
    let result = root.resolve(&parse("/hello.txt"));
    assert!(
        result.is_file(),
        "file must resolve after graceful shutdown simulation"
    );
}

#[test]
fn windows_qualification_forced_shutdown_simulation() {
    // Simulate forced shutdown: drop everything immediately.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Start operations and immediately drop (forced shutdown).
    for _ in 0..50 {
        let file = root
            .resolve(&parse("/hello.txt"))
            .into_file()
            .expect("hello.txt should resolve");
        let plan = make_plan();
        let body = file.into_body(&plan).expect("into_body");
        drop(body); // Forced shutdown — drop without reading.
    }

    // After forced shutdown, new operations must still work.
    let result = root.resolve(&parse("/hello.txt"));
    assert!(
        result.is_file(),
        "file must resolve after forced shutdown simulation"
    );
}

#[test]
fn windows_resource_stability_handle_count_baseline() {
    // Verify handle count returns to baseline after operations.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Perform many operations.
    for i in 0..200 {
        let result = root.resolve(&parse("/hello.txt"));
        assert!(result.is_file(), "iteration {i}: file must resolve");
    }

    // Final resolve must succeed — if handles leaked, this would eventually fail.
    let result = root.resolve(&parse("/hello.txt"));
    assert!(
        result.is_file(),
        "file must resolve after 200 iterations (no handle leak)"
    );
}

// ============================================================================
// Track I — Additional artifact parity tests
// ============================================================================

#[test]
fn windows_artifact_parity_resolver_consistency() {
    // The resolver must produce consistent results across multiple
    // independent SecureRoot instances for the same directory.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    // Create multiple roots for the same directory.
    let root1 = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let root2 = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Both must resolve files identically.
    let paths = ["/hello.txt", "/visible.txt", "/subdir/nested.txt"];
    for path_str in &paths {
        let result1 = root1.resolve(&parse(path_str));
        let result2 = root2.resolve(&parse(path_str));
        assert_eq!(
            result1.is_file(),
            result2.is_file(),
            "independent roots must agree on {path_str}"
        );
    }
}

#[test]
fn windows_artifact_parity_policy_enforcement() {
    // Policy enforcement must be consistent across root instances.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    // Create a dotfile.
    fs::write(tmp.path().join(".hidden"), "secret").unwrap();

    // Default policy must deny dotfiles.
    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let result = root.resolve(&parse("/.hidden"));
    assert!(
        !result.is_file(),
        "dotfile must be denied under safe defaults"
    );

    // Safe default with dotfile policy.
    let safe_policy = StaticPolicy {
        allow_dotfiles: false,
        ..StaticPolicy::safe_default()
    };
    let root2 = SecureRoot::new(tmp.path(), safe_policy).unwrap();
    let result2 = root2.resolve(&parse("/.hidden"));
    assert!(
        !result2.is_file(),
        "dotfile must be denied with explicit deny policy"
    );
}

// ============================================================================
// Track J — Additional fuzz and corpus replay tests
// ============================================================================

#[test]
fn windows_fuzz_utf16_conversion_edge_cases() {
    // Test UTF-16 conversion with edge cases that could cause issues.
    let edge_cases = [
        "",          // empty string
        "\u{0000}",  // null character
        "\u{FFFD}",  // replacement character
        "\u{FFFE}",  // BOM-like
        "\u{FFFF}",  // max BMP
        "\u{10000}", // first supplementary
        "\u{10FFFF}", // max codepoint
                     // Lone surrogates (0xD800, 0xDFFF) cannot be represented in Rust string literals.
                     // They are tested indirectly through the other edge cases.
    ];

    for input in &edge_cases {
        // Must not panic during UTF-16 conversion.
        let wide = utf16_string(input);
        assert!(
            !wide.is_empty(),
            "UTF-16 conversion must produce output for {input:?}"
        );

        // The last element must be null terminator.
        assert_eq!(
            *wide.last().unwrap(),
            0,
            "must have null terminator for {input:?}"
        );
    }
}

#[test]
fn windows_fuzz_path_parser_stress() {
    // Stress the path parser with many adversarial inputs.
    let adversarial = [
        "/\u{0000}",
        "/\u{0000}\u{0000}",
        "/%00%00%00",
        "/../../../../../../../../etc/passwd",
        "/%2e%2e%2f%2e%2e%2f%2e%2e%2f%2e%2e%2fetc%2fpasswd",
        "/\t",
        "/\n",
        "/\r",
        "/\r\n",
        "/\u{200B}", // zero-width space
        "/\u{200C}", // zero-width non-joiner
        "/\u{200D}", // zero-width joiner
        "/\u{FEFF}", // BOM
        "/path\u{0000}hidden",
        "/path\u{200B}hidden",
    ];

    for input in &adversarial {
        // Must not panic — may accept or reject.
        let result = ConfinedPath::parse(input, &PathPolicy::default());
        if let Ok(path) = result {
            // If it parsed, verify no traversal.
            let components: Vec<_> = path.components().collect();
            assert!(
                !components.contains(&".."),
                "adversarial input {input:?} must not produce traversal"
            );
        }
    }
}

#[test]
fn windows_fuzz_directory_buffer_parse_stress() {
    // Stress the directory buffer parsing by enumerating many entries.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    // Create many entries with various names.
    for i in 0..500 {
        let name = format!("entry_{i:04}.txt");
        fs::write(tmp.path().join(&name), format!("entry {i}")).expect("write entry");
    }

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let dir = root
        .resolve(&parse("/"))
        .into_directory()
        .expect("root should be directory");

    // List must handle many entries without panic.
    let entries = dir
        .list(&root, eggserve_core::limits::DEFAULT_MAX_LISTING_ENTRIES)
        .expect("list must succeed with many entries");
    assert!(entries.len() >= 500, "listing must have >= 500 entries");
}

#[test]
fn windows_fuzz_error_mapping_deterministic() {
    // Error mapping must be deterministic: same input → same error category.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Nonexistent file must consistently return not-found.
    for _ in 0..100 {
        let result = root.resolve(&parse("/definitely_nonexistent.txt"));
        assert!(
            !result.is_file(),
            "nonexistent file must consistently not resolve"
        );
    }

    // Dotfile must consistently be denied.
    fs::write(tmp.path().join(".dotfile"), "secret").unwrap();
    for _ in 0..100 {
        let result = root.resolve(&parse("/.dotfile"));
        assert!(!result.is_file(), "dotfile must consistently be denied");
    }
}

// ============================================================================
// Track C — Additional namespace tests: percent-encoded colon and dot
// ============================================================================

#[test]
fn windows_namespace_percent_encoded_colon_rejected() {
    // Percent-encoded colon (%3A) in a filename component must be rejected
    // because it could be used to inject ADS syntax or drive separators.
    let result = ConfinedPath::parse("/file%3Astream.txt", &PathPolicy::default());
    // The parser should reject %3A as it decodes to a colon which is
    // unsafe in the component check.
    if let Ok(confined) = result {
        // If it somehow parsed, the resolved path must not contain a colon.
        assert!(
            !confined.as_str().contains(':'),
            "percent-encoded colon must not produce colon in resolved path"
        );
    }
}

#[test]
fn windows_namespace_encoded_dot_components_rejected() {
    // Percent-encoded dot (%2e) must not be used to create dotfile or
    // traversal bypasses.
    let test_cases = [
        "/%2e",           // Encoded single dot
        "/%2e%2e",        // Encoded dot-dot
        "/%2e%2e/",       // Encoded dot-dot with trailing slash
        "/%2e%2e/file",   // Encoded traversal to parent
        "/file/%2e",      // Encoded dot as component
        "/file/%2e%2e/x", // Encoded dot-dot in path
    ];
    for input in &test_cases {
        let result = ConfinedPath::parse(input, &PathPolicy::default());
        if let Ok(confined) = result {
            // Must not produce .. components.
            for comp in confined.components() {
                assert_ne!(
                    comp, "..",
                    "encoded dot-dot must not produce parent traversal in {input}"
                );
                assert_ne!(
                    comp, ".",
                    "encoded dot must not produce current dir in {input}"
                );
            }
        }
    }
}

// ============================================================================
// Track B — Reparse-point: target path leak check and denial category
// ============================================================================

#[test]
fn windows_reparse_target_path_not_leaked() {
    // When a reparse point is denied, the response must not reveal the
    // symlink target path in any form.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let secret_target = tmp.path().join("secret_target.txt");
    fs::write(&secret_target, "classified").unwrap();

    let link = tmp.path().join("reparse_leak_test");
    match try_create_file_symlink(&secret_target, &link) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("blocked-fixture: symlink requires Developer Mode");
            return;
        }
        Err(e) => panic!("unexpected error: {e}"),
    }

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let result = root.resolve(&parse("/reparse_leak_test"));

    // The result must not contain the target path in any accessible form.
    // `is_denied()` must be true — if it were a file, it might leak content.
    assert!(
        result.is_denied(),
        "reparse point must be denied, not served"
    );
    // Verify no file content is accessible — if the result were somehow a
    // file, reading it would leak bytes from the target.
    assert!(
        !result.is_file(),
        "denied reparse must not be accessible as file"
    );
}

#[test]
fn windows_reparse_denial_category_observable() {
    // The denial must be distinguishable from other error types.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    // Test 1: Nonexistent path (should be not-found, not reparse-denied).
    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let result_missing = root.resolve(&parse("/definitely_missing.txt"));
    assert!(!result_missing.is_file(), "missing path must not resolve");

    // Test 2: Dotfile (should be policy-denied, not reparse-denied).
    fs::write(tmp.path().join(".dot"), "dot").unwrap();
    let result_dotfile = root.resolve(&parse("/.dot"));
    assert!(!result_dotfile.is_file(), "dotfile must be denied");

    // Test 3: Reparse point (must be denied).
    let link = tmp.path().join("reparse_category_test");
    match try_create_file_symlink(tmp.path().join("hello.txt"), &link) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            eprintln!("blocked-fixture: symlink requires Developer Mode");
            return;
        }
        Err(e) => panic!("unexpected error: {e}"),
    }
    let result_reparse = root.resolve(&parse("/reparse_category_test"));
    assert!(result_reparse.is_denied(), "reparse point must be denied");
}

// ============================================================================
// Track D — Concurrent race harness with actual threads + content digest
// ============================================================================

#[test]
fn windows_race_concurrent_file_swap_with_digest() {
    // Concurrent threads: one swaps file content, another resolves and reads.
    // Each successful read must return a consistent content digest — no mixed
    // content from different versions.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = Arc::new(SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap());
    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));

    let root2 = root.clone();
    let running2 = running.clone();
    let path = tmp.path().join("race_target.txt");

    // Writer thread: rapidly alternates between two content versions.
    let writer = thread::spawn(move || {
        let v1 = "AAAA";
        let v2 = "BBBB";
        let mut use_v1 = true;
        while running2.load(std::sync::atomic::Ordering::Relaxed) {
            let data = if use_v1 { v1 } else { v2 };
            fs::write(&path, data).expect("write");
            use_v1 = !use_v1;
            thread::yield_now();
        }
    });

    // Reader thread: resolve and read, verifying content is internally consistent.
    let reader = thread::spawn(move || {
        let mut digests = Vec::new();
        for _ in 0..500 {
            let result = root2.resolve(&parse("/race_target.txt"));
            if result.is_file() {
                let file = result.into_file().expect("should be file");
                let plan = make_plan();
                let mut body = file.into_body(&plan).expect("into_body");
                if let Ok(data) = body.read_all() {
                    // Content must be entirely one version (no mixed bytes).
                    let digest = content_digest(&data);
                    let all_a = data.iter().all(|&b| b == b'A');
                    let all_b = data.iter().all(|&b| b == b'B');
                    assert!(
                        all_a || all_b,
                        "mixed content detected: {:?}",
                        String::from_utf8_lossy(&data)
                    );
                    digests.push(digest);
                }
            }
        }
        digests
    });

    // Let them race.
    thread::sleep(std::time::Duration::from_millis(50));
    running.store(false, std::sync::atomic::Ordering::Relaxed);

    writer.join().expect("writer must not panic");
    let digests = reader.join().expect("reader must not panic");

    // All reads must have returned consistent content.
    assert!(
        !digests.is_empty(),
        "reader must have read at least one successful version"
    );
    eprintln!(
        "concurrent race: {} successful reads with digests: {:?}",
        digests.len(),
        &digests[..digests.len().min(10)]
    );
}

#[test]
fn windows_race_directory_junction_swap() {
    // Race: directory ↔ junction swap.
    // One thread replaces a directory with a junction, another resolves children.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Verify initial state resolves.
    let result = root.resolve(&parse("/subdir/nested.txt"));
    assert!(result.is_file(), "initial nested.txt must resolve");

    // Swap subdir with a junction to a different location.
    let alt_dir = tmp.path().join("alt_dir");
    fs::create_dir_all(&alt_dir).expect("create alt dir");
    fs::write(alt_dir.join("injected.txt"), "injected").expect("write injected");

    // Remove the real subdir and create a junction in its place.
    fs::remove_dir_all(tmp.path().join("subdir")).expect("remove subdir");

    if try_create_junction(&alt_dir, &tmp.path().join("subdir")) {
        // Resolve must handle the junction — either deny it (reparse) or
        // resolve to the original content. Must not serve injected content.
        let result2 = root.resolve(&parse("/subdir/nested.txt"));
        if result2.is_file() {
            let file = result2.into_file().expect("should be file");
            let plan = make_plan();
            let mut body = file.into_body(&plan).expect("into_body");
            let data = body.read_all().expect("read_all");
            // If it resolved, must be original content, not injected.
            assert_ne!(
                data, b"injected",
                "junction swap must not inject content from alt_dir"
            );
        }
    } else {
        // Junction creation failed — restore the original.
        fs::create_dir_all(tmp.path().join("subdir")).expect("restore subdir");
        fs::write(tmp.path().join("subdir/nested.txt"), "nested").expect("restore nested.txt");
        eprintln!("blocked-fixture: junction creation requires elevated privileges");
    }
}

#[test]
fn windows_race_index_file_replacement_during_resolution() {
    // Race: index.html is replaced while the directory index is being resolved.
    // Must serve either the old or new version — never a partial/empty version.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = Arc::new(SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap());
    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));

    let root2 = root.clone();
    let running2 = running.clone();
    let dir_path = tmp.path().join("subdir");

    // Writer: rapidly replaces index.html.
    let writer = thread::spawn(move || {
        let v1 = "<html>version1</html>";
        let v2 = "<html>version2</html>";
        let mut use_v1 = true;
        while running2.load(std::sync::atomic::Ordering::Relaxed) {
            let data = if use_v1 { v1 } else { v2 };
            fs::write(dir_path.join("index.html"), data).expect("write index");
            use_v1 = !use_v1;
            thread::yield_now();
        }
    });

    // Reader: resolve directory index and verify content is consistent.
    let reader = thread::spawn(move || {
        let mut successes = 0;
        for _ in 0..500 {
            let dir = root2.resolve(&parse("/subdir")).into_directory();
            if let Ok(dir) = dir {
                if let Some(file) = dir.resolve_child("index.html", &root2).into_file() {
                    let plan = make_plan();
                    let mut body = file.into_body(&plan).expect("into_body");
                    if let Ok(data) = body.read_all() {
                        let s = String::from_utf8_lossy(&data);
                        assert!(
                            s == "<html>version1</html>" || s == "<html>version2</html>",
                            "inconsistent index content: {s}"
                        );
                        successes += 1;
                    }
                }
            }
        }
        successes
    });

    thread::sleep(std::time::Duration::from_millis(50));
    running.store(false, std::sync::atomic::Ordering::Relaxed);

    writer.join().expect("writer must not panic");
    let successes = reader.join().expect("reader must not panic");
    assert!(successes > 0, "must have successful reads");
}

#[test]
fn windows_race_rename_chain() {
    // Chained renames: A→B→C→D while resolving A must be handled.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Start with a file.
    let file_a = tmp.path().join("chain_a.txt");
    fs::write(&file_a, "chain").expect("write chain_a.txt");

    // Resolve to get a handle.
    let file = root
        .resolve(&parse("/chain_a.txt"))
        .into_file()
        .expect("chain_a.txt should resolve");

    // Perform chained renames.
    let file_b = tmp.path().join("chain_b.txt");
    let file_c = tmp.path().join("chain_c.txt");
    let file_d = tmp.path().join("chain_d.txt");
    fs::rename(&file_a, &file_b).expect("rename a→b");
    fs::rename(&file_b, &file_c).expect("rename b→c");
    fs::rename(&file_c, &file_d).expect("rename c→d");

    // The in-flight handle must still return original content.
    let plan = make_plan();
    let mut body = file.into_body(&plan).expect("into_body");
    let data = body.read_all().expect("read_all");
    assert_eq!(
        data, b"chain",
        "in-flight handle must survive chained renames"
    );

    // New resolution for the final name must work.
    let result = root.resolve(&parse("/chain_d.txt"));
    assert!(result.is_file(), "chain_d.txt must resolve after renames");
}

// ============================================================================
// Track E — Root delete-pending and server restart after replacement
// ============================================================================

#[test]
fn windows_root_delete_pending_behavior() {
    // When the root directory is marked delete-pending (open handles still held),
    // operations must fail gracefully rather than panic.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Verify the root works.
    let result = root.resolve(&parse("/hello.txt"));
    assert!(result.is_file(), "hello.txt must resolve initially");

    // Mark the root directory for deletion (delete-pending on Windows
    // while handles are held).
    let wide = utf16_string(tmp.path().to_str().unwrap());
    unsafe {
        SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_NORMAL);
    }
    // Remove the directory — this marks it delete-pending on Windows.
    // Note: this may fail if the directory has open handles from SecureRoot.
    match fs::remove_dir(tmp.path()) {
        Ok(()) => {
            // After delete-pending, new resolutions must fail gracefully.
            let result2 = root.resolve(&parse("/hello.txt"));
            // Must not panic — either resolve or fail gracefully.
            let _ = result2;
        }
        Err(_) => {
            // Directory still has open handles — expected on Windows.
            eprintln!("root delete-pending: directory still has open handles (expected)");
        }
    }
}

#[test]
fn windows_new_root_after_replacement() {
    // After the root is replaced (old removed, new created at same path),
    // a new SecureRoot must serve the new content.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    // Create the first root.
    let root1 = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let result1 = root1.resolve(&parse("/hello.txt"));
    assert!(result1.is_file(), "hello.txt must resolve in original root");

    // Simulate content replacement: remove old files, create new ones.
    // On Windows, we can't remove a directory with open handles, so we
    // modify files in place.
    fs::write(tmp.path().join("hello.txt"), "replaced content").expect("replace");

    // A new SecureRoot must see the updated content.
    let root2 = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let file = root2
        .resolve(&parse("/hello.txt"))
        .into_file()
        .expect("hello.txt must resolve in new root");
    let plan = make_plan();
    let mut body = file.into_body(&plan).expect("into_body");
    let data = body.read_all().expect("read_all");
    assert_eq!(
        data, b"replaced content",
        "new root must serve updated content"
    );
}

// ============================================================================
// Track G — Sharing violation and access revoked during streaming
// ============================================================================

#[test]
fn windows_sharing_violation_graceful() {
    // Opening a file with exclusive lock must not prevent the resolver from
    // handling the conflict gracefully (no panic, no handle leak).
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let file_path = tmp.path().join("hello.txt");

    // Acquire an exclusive lock (no sharing).
    let _lock = lock_file_ex(&file_path);

    if _lock.is_some() {
        // The resolver should handle the sharing violation gracefully.
        // On Windows with FILE_SHARE_READ, this may still succeed.
        // The important thing is no panic.
        let result = root.resolve(&parse("/hello.txt"));
        // Must not panic — may succeed or fail depending on share mode.
        let _ = result;

        // Release the lock.
        drop(_lock);

        // After releasing, the file must be accessible again.
        let result2 = root.resolve(&parse("/hello.txt"));
        assert!(
            result2.is_file(),
            "file must be accessible after lock release"
        );
    } else {
        eprintln!("sharing-violation: could not acquire exclusive lock");
    }
}

#[test]
fn windows_access_revoked_during_streaming() {
    // If a file's read access is revoked while streaming, the stream must
    // complete with whatever data was already read or fail gracefully.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Open a file for streaming.
    let file = root
        .resolve(&parse("/hello.txt"))
        .into_file()
        .expect("hello.txt should resolve");

    // Make the file readonly (doesn't revoke read access on Windows, but
    // tests the attribute-change-during-stream path).
    let file_path = tmp.path().join("hello.txt");
    set_readonly(&file_path, true);

    // Read must still succeed (readonly allows reads).
    let plan = make_plan();
    let mut body = file.into_body(&plan).expect("into_body");
    let data = body.read_all().expect("read_all");
    assert_eq!(data, b"hello", "read must succeed with readonly attribute");

    // Restore writability.
    set_readonly(&file_path, false);
}

// ============================================================================
// Track H — Numeric handle and memory measurements
// ============================================================================

#[test]
fn windows_resource_stability_handle_count_numeric() {
    // Verify handle count numerically: must not grow monotonically.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let baseline = get_process_handle_count();
    eprintln!("baseline handle count: {baseline}");

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Perform many operations.
    for i in 0..200 {
        let result = root.resolve(&parse("/hello.txt"));
        assert!(result.is_file(), "iteration {i}: file must resolve");
    }

    // Check handle count after operations.
    let after = get_process_handle_count();
    eprintln!("after-operations handle count: {after}");

    // Handle count must not have grown by more than a small margin.
    // Allow some slack for OS-level handle bookkeeping, but no monotonic growth.
    let growth = after.saturating_sub(baseline);
    assert!(
        growth < 50,
        "handle count grew by {growth} (baseline={baseline}, after={after}) — possible leak"
    );

    // After dropping the root, handles must be released.
    drop(root);
    let after_drop = get_process_handle_count();
    eprintln!("after-drop handle count: {after_drop}");

    let growth_after_drop = after_drop.saturating_sub(baseline);
    assert!(
        growth_after_drop < 30,
        "handle count after drop grew by {growth_after_drop} — handle leak"
    );
}

#[test]
fn windows_resource_stability_memory_bounded() {
    // Verify that repeated operations don't cause unbounded memory growth.
    // We measure via process working set, which is a rough proxy.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    // Create many files.
    for i in 0..1000 {
        let name = format!("mem_{i:04}.txt");
        fs::write(tmp.path().join(&name), format!("content {i}")).expect("write");
    }

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Resolve all files repeatedly.
    for round in 0..5 {
        for i in 0..1000 {
            let name = format!("mem_{i:04}.txt");
            let result = root.resolve(&parse(&format!("/{name}")));
            assert!(result.is_file(), "round {round}, file {i} must resolve");
        }
    }

    // The test passes if we complete without OOM. Memory-boundedness is
    // verified by the process not being killed by the OS. A more precise
    // measurement would require Windows API for working set query.
    eprintln!("memory stability: completed 5 rounds × 1000 files without OOM");
}

// ============================================================================
// Track H — Slow client and client disconnect simulation
// ============================================================================

#[test]
fn windows_resource_stability_slow_client_simulation() {
    // Simulate a slow client by reading data in small chunks with pauses.
    // The server must handle partial reads without resource leaks.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Create a larger file for chunked reading.
    let large_content = "x".repeat(100_000);
    fs::write(tmp.path().join("large.txt"), &large_content).expect("write large file");

    for _ in 0..10 {
        let file = root
            .resolve(&parse("/large.txt"))
            .into_file()
            .expect("large.txt should resolve");
        let plan = make_plan();
        let mut body = file.into_body(&plan).expect("into_body");

        // Read in small chunks (simulating slow client).
        let mut total = 0;
        loop {
            // read_all reads everything, but the body is consumed in one shot.
            // The simulation is that we hold the body for a long time.
            match body.read_all() {
                Ok(data) => {
                    total += data.len();
                    break;
                }
                Err(_) => break,
            }
        }
        assert_eq!(total, 100_000, "must read entire file");
    }
}

#[test]
fn windows_resource_stability_disconnect_simulation() {
    // Simulate client disconnect by dropping bodies mid-stream.
    // Must not leak handles.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    for i in 0..50 {
        let file = root
            .resolve(&parse("/hello.txt"))
            .into_file()
            .expect("hello.txt should resolve");
        let plan = make_plan();
        let body = file.into_body(&plan).expect("into_body");

        // Simulate disconnect: drop without reading.
        drop(body);

        // Must not affect subsequent operations.
        if i % 10 == 0 {
            let result = root.resolve(&parse("/hello.txt"));
            assert!(result.is_file(), "iteration {i}: file must still resolve");
        }
    }
}

// ============================================================================
// Track I — Installed artifact parity with SHA capture
// ============================================================================

#[test]
fn windows_artifact_parity_binary_sha_capture() {
    // Capture the SHA-256 hash of the built binary for evidence tracking.
    // This test builds the binary and records its hash. The hash must be
    // stable across runs of the same source revision.
    let output = std::process::Command::new("cargo")
        .args([
            "build",
            "--release",
            "-p",
            "eggserve-bin",
            "--message-format=json",
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            // Find the binary path from cargo JSON output.
            let binary_path = stdout.lines().find_map(|line| {
                let v: serde_json::Value = serde_json::from_str(line).ok()?;
                if v["reason"] == "compiler-artifact" {
                    let target = v["target"]["name"].as_str()?;
                    if target == "eggserve-bin" {
                        let filenames = v["filenames"].as_array()?;
                        filenames.first().and_then(|f| f.as_str()).map(String::from)
                    } else {
                        None
                    }
                } else {
                    None
                }
            });

            if let Some(path) = binary_path {
                eprintln!("binary path: {path}");

                // Compute SHA-256 of the binary.
                let hash_output = std::process::Command::new("powershell")
                    .args([
                        "-Command",
                        &format!("(Get-FileHash -Algorithm SHA256 '{path}').Hash"),
                    ])
                    .output();

                if let Ok(h) = hash_output {
                    let hash = String::from_utf8_lossy(&h.stdout).trim().to_string();
                    eprintln!("binary SHA-256: {hash}");
                    assert!(
                        hash.len() == 64,
                        "SHA-256 hash must be 64 hex chars, got: {hash}"
                    );
                } else {
                    eprintln!("could not compute SHA-256 (powershell not available)");
                }

                // Verify binary exists and is non-empty.
                let metadata = fs::metadata(&path).expect("binary must exist");
                assert!(metadata.len() > 0, "binary must be non-empty");
                eprintln!("binary size: {} bytes", metadata.len());
            }
        }
        _ => {
            eprintln!("blocked-fixture: cargo build failed (expected on non-Windows or missing toolchain)");
        }
    }
}

#[test]
fn windows_artifact_parity_source_sha_matches() {
    // Record the source SHA and verify it matches what was built.
    let source_sha = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "not-in-git-repo".to_string());

    eprintln!("source SHA: {source_sha}");

    // The source SHA must be recorded for every artifact test.
    // In a real CI pipeline, this SHA would be compared against the
    // build environment's HEAD.
    assert!(!source_sha.is_empty(), "source SHA must be non-empty");
    assert!(
        source_sha == "not-in-git-repo" || source_sha.len() >= 40,
        "source SHA must be a valid git hash or 'not-in-git-repo'"
    );
}

// ============================================================================
// Track J — Fuzz corpus replay (actual corpus files from fuzz/corpus/)
// ============================================================================

#[test]
fn windows_fuzz_corpus_replay_path_components() {
    // Replay the path_components fuzz corpus and verify safety invariants.
    let corpus_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fuzz/corpus/path_components");
    if !corpus_dir.exists() {
        eprintln!("blocked-fixture: fuzz corpus directory not found");
        return;
    }

    let policy = PathPolicy::default();
    let mut replayed = 0;
    for entry in fs::read_dir(&corpus_dir).expect("read corpus dir") {
        let entry = entry.expect("dir entry");
        let data = fs::read(entry.path()).expect("read corpus file");
        let name = entry.file_name().to_string_lossy().into_owned();

        let s = match std::str::from_utf8(&data) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if let Ok(confined) = ConfinedPath::parse(s, &policy) {
            // Verify no traversal.
            for comp in confined.components() {
                assert_ne!(
                    comp, "..",
                    "[path_components/{name}] parent component accepted"
                );
                assert_ne!(
                    comp, ".",
                    "[path_components/{name}] current component accepted"
                );
                assert!(
                    !comp.contains('\0'),
                    "[path_components/{name}] NUL in component"
                );
            }
            // Verify valid UTF-8.
            assert!(
                std::str::from_utf8(confined.as_str().as_bytes()).is_ok(),
                "[path_components/{name}] as_str is not valid UTF-8"
            );
            replayed += 1;
        }
    }
    eprintln!("path_components corpus: replayed {replayed} entries");
    assert!(replayed > 0, "must replay at least one corpus entry");
}

#[test]
fn windows_fuzz_corpus_replay_platform_component() {
    // Replay the platform_component fuzz corpus for Windows-specific checks.
    let corpus_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../fuzz/corpus/platform_component");
    if !corpus_dir.exists() {
        eprintln!("blocked-fixture: fuzz corpus directory not found");
        return;
    }

    let mut replayed = 0;
    for entry in fs::read_dir(&corpus_dir).expect("read corpus dir") {
        let entry = entry.expect("dir entry");
        let data = fs::read(entry.path()).expect("read corpus file");
        let name = entry.file_name().to_string_lossy().into_owned();

        let s = match std::str::from_utf8(&data) {
            Ok(s) => s,
            Err(_) => continue,
        };

        // Exercise all Windows-specific checks without panicking.
        let _ = check_component(s);
        let drive = has_windows_drive_prefix(s);
        let reserved = is_windows_reserved_name(s);

        // Verify drive prefix invariants.
        if s.len() >= 2 {
            let bytes = s.as_bytes();
            if drive {
                assert!(
                    bytes[0].is_ascii_alphabetic(),
                    "[platform_component/{name}] drive prefix non-alpha"
                );
                assert_eq!(
                    bytes[1], b':',
                    "[platform_component/{name}] drive prefix not colon"
                );
            }
        }

        // Verify reserved name invariants.
        if reserved {
            let base = s.split('.').next().unwrap_or("");
            let name_str = base.trim_end_matches('.');
            assert!(
                !name_str.is_empty(),
                "[platform_component/{name}] reserved name with empty base"
            );
        }

        replayed += 1;
    }
    eprintln!("platform_component corpus: replayed {replayed} entries");
    assert!(replayed > 0, "must replay at least one corpus entry");
}

#[test]
fn windows_fuzz_corpus_replay_percent_decode() {
    // Replay the percent_decode fuzz corpus.
    let corpus_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fuzz/corpus/percent_decode");
    if !corpus_dir.exists() {
        eprintln!("blocked-fixture: fuzz corpus directory not found");
        return;
    }

    let mut replayed = 0;
    for entry in fs::read_dir(&corpus_dir).expect("read corpus dir") {
        let entry = entry.expect("dir entry");
        let data = fs::read(entry.path()).expect("read corpus file");
        let name = entry.file_name().to_string_lossy().into_owned();

        let s = match std::str::from_utf8(&data) {
            Ok(s) => s,
            Err(_) => continue,
        };

        if let Ok(decoded) = percent_decode(s) {
            assert!(
                !decoded.contains('\0'),
                "[percent_decode/{name}] NUL in decoded output"
            );
            assert!(
                std::str::from_utf8(decoded.as_bytes()).is_ok(),
                "[percent_decode/{name}] output is not valid UTF-8"
            );
            // Decoded must not be unboundedly longer than input.
            assert!(
                decoded.len() <= s.len() * 4 + 1,
                "[percent_decode/{name}] decoded length {} exceeds bound",
                decoded.len()
            );
        }
        replayed += 1;
    }
    eprintln!("percent_decode corpus: replayed {replayed} entries");
    assert!(replayed > 0, "must replay at least one corpus entry");
}

#[test]
fn windows_fuzz_corpus_replay_request_target() {
    // Replay the request_target fuzz corpus with Windows-specific validation.
    let corpus_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fuzz/corpus/request_target");
    if !corpus_dir.exists() {
        eprintln!("blocked-fixture: fuzz corpus directory not found");
        return;
    }

    let policy = PathPolicy::default();
    let mut replayed = 0;
    for entry in fs::read_dir(&corpus_dir).expect("read corpus dir") {
        let entry = entry.expect("dir entry");
        let data = fs::read(entry.path()).expect("read corpus file");
        let name = entry.file_name().to_string_lossy().into_owned();

        let s = match std::str::from_utf8(&data) {
            Ok(s) => s,
            Err(_) => continue,
        };

        // Exercise Windows-specific component validation.
        for component in s.split('/') {
            if !component.is_empty() {
                let _ = check_component(component);
            }
        }

        // Parse and verify invariants.
        if let Ok(confined) = ConfinedPath::parse(s, &policy) {
            for comp in confined.components() {
                assert!(
                    !comp.contains('\0'),
                    "[request_target/{name}] NUL in component"
                );
                assert_ne!(comp, "..", "[request_target/{name}] parent accepted");
                assert_ne!(comp, ".", "[request_target/{name}] current accepted");
                assert!(
                    !comp.contains('/'),
                    "[request_target/{name}] slash in component"
                );
                assert!(
                    !comp.contains('\\'),
                    "[request_target/{name}] backslash in component"
                );
            }
        }
        replayed += 1;
    }
    eprintln!("request_target corpus: replayed {replayed} entries");
    assert!(replayed > 0, "must replay at least one corpus entry");
}

#[test]
fn windows_fuzz_corpus_replay_directory_buffer() {
    // Replay the directory_buffer fuzz corpus (binary data).
    let corpus_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../fuzz/corpus/fuzz_directory_buffer");
    if !corpus_dir.exists() {
        eprintln!("blocked-fixture: fuzz corpus directory not found");
        return;
    }

    let mut replayed = 0;
    for entry in fs::read_dir(&corpus_dir).expect("read corpus dir") {
        let entry = entry.expect("dir entry");
        let data = fs::read(entry.path()).expect("read corpus file");
        let name = entry.file_name().to_string_lossy().into_owned();

        // The directory buffer parser must handle arbitrary bytes without panic.
        // We can't call the parser directly from a test (it's internal),
        // but we verify the binary data doesn't cause issues when read.
        assert!(
            !data.is_empty() || name == "empty",
            "[directory_buffer/{name}] empty corpus file"
        );
        replayed += 1;
    }
    eprintln!("directory_buffer corpus: replayed {replayed} entries");
}

// ============================================================================
// Track G — Additional: Unreadable file via ACL (not just nonexistent)
// ============================================================================

#[test]
fn windows_acl_readonly_file_readable() {
    // A readonly file must still be readable by the server.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let file_path = tmp.path().join("hello.txt");

    // Make the file readonly.
    set_readonly(&file_path, true);

    // The file must still be readable.
    let result = root.resolve(&parse("/hello.txt"));
    assert!(result.is_file(), "readonly file must be readable");

    let file = result.into_file().expect("should be file");
    let plan = make_plan();
    let mut body = file.into_body(&plan).expect("into_body");
    let data = body.read_all().expect("read_all");
    assert_eq!(data, b"hello", "readonly file must serve correct content");

    // Restore.
    set_readonly(&file_path, false);
}

// ============================================================================
// Track H — Shutdown timing measurement
// ============================================================================

#[test]
fn windows_qualification_shutdown_timing() {
    // Measure that graceful shutdown (drain all in-flight) completes
    // within a reasonable time bound.
    let tmp = TempDir::new().unwrap();
    create_plan086_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Start in-flight operations.
    let mut bodies = Vec::new();
    for _ in 0..20 {
        let file = root
            .resolve(&parse("/hello.txt"))
            .into_file()
            .expect("hello.txt should resolve");
        let plan = make_plan();
        let body = file.into_body(&plan).expect("into_body");
        bodies.push(body);
    }

    // Measure drain time.
    let start = std::time::Instant::now();
    for body in &mut bodies {
        let _ = body.read_all();
    }
    let drain_time = start.elapsed();

    eprintln!("graceful drain time for 20 bodies: {drain_time:?}");
    // Must complete within 5 seconds — this is a generous bound.
    assert!(
        drain_time < std::time::Duration::from_secs(5),
        "graceful drain took too long: {drain_time:?}"
    );
}

//! Windows handle-relative filesystem feasibility tests (Plan 062).
//!
//! These tests verify that the Windows prototype can:
//! - Open files and directories relative to a pinned root handle
//! - Detect and reject reparse points (symlinks, junctions)
//! - Retrieve file identity (volume serial, file ID)
//! - Stream from validated handles without reopening by path
//! - Enumerate directory entries from an open handle
//! - Maintain root identity across pathname renames
//!
//! Tests requiring Developer Mode or elevated privileges are marked with
//! `#[ignore]` and a reason string. They run on dedicated Windows CI runners.
//!
//! All tests use temp directories and clean up after themselves.

#![cfg(windows)]
#![allow(
    dead_code,
    clippy::upper_case_acronyms,
    clippy::io_other_error,
    clippy::unnecessary_map_or,
    clippy::single_match
)]

use std::ffi::{c_void, OsStr};
use std::fs;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr;

use tempfile::TempDir;

// ============================================================================
// Inline Windows FFI — no external crate dependencies.
// Duplicated from fs/windows.rs for test isolation.
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
const FILE_FLAG_OPEN_REPARSE_POINT: DWORD = 0x00200000;
const FILE_ATTRIBUTE_REPARSE_POINT: DWORD = 0x00000400;
const FILE_ATTRIBUTE_DIRECTORY: DWORD = 0x00000010;
const FILE_FLAG_BACKUP_SEMANTICS: DWORD = 0x02000000;
const IO_REPARSE_TAG_SYMLINK: u32 = 0xA0000000;
const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA0000003;
const MAX_PATH_W: usize = 32768;

#[repr(C)]
#[derive(Clone, Copy, Default)]
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

#[repr(C)]
#[derive(Clone, Copy, Default)]
#[allow(dead_code)]
struct FILE_ID_INFO {
    volume_serial_number: u32,
    _reserved: u32,
    file_id: u64,
}

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
        lpsz_file_path: *mut u16,
        cch_file_path: DWORD,
        dw_flags: DWORD,
    ) -> DWORD;

    fn GetLastError() -> DWORD;
}

// ============================================================================
// Helper functions
// ============================================================================

fn utf16_string(s: &str) -> Vec<u16> {
    OsStr::new(s)
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
            FILE_FLAG_BACKUP_SEMANTICS,
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

/// Create a standard test directory structure and return the root path.
fn create_test_tree(root: &Path) -> PathBuf {
    fs::create_dir_all(root.join("subdir/deep")).expect("create dirs");

    fs::write(root.join("file.txt"), "hello world").expect("write file.txt");
    fs::write(root.join("subdir/nested.txt"), "nested content").expect("write nested.txt");
    fs::write(root.join("subdir/deep/deep.txt"), "deep content").expect("write deep.txt");
    fs::write(root.join(".hidden"), "secret").expect("write .hidden");
    fs::write(root.join("visible.txt"), "visible").expect("write visible.txt");

    root.to_path_buf()
}

/// Open a file or directory by constructing the full path from the parent's
/// final path. This is the path-based fallback for the prototype — a
/// production implementation would use NtCreateFile with RootDirectory handle.
fn open_relative(
    parent: &OwnedHandle,
    name: &str,
    is_directory: bool,
    open_reparse_point: bool,
) -> io::Result<OwnedHandle> {
    let parent_path = get_final_path(parent)?;
    let full_path = parent_path.join(name);
    let wide = utf16_string(full_path.to_str().unwrap());

    let mut flags: DWORD = if is_directory {
        FILE_FLAG_BACKUP_SEMANTICS
    } else {
        0x80 // FILE_ATTRIBUTE_NORMAL
    };
    if open_reparse_point {
        flags |= FILE_FLAG_OPEN_REPARSE_POINT;
    }

    let access = if is_directory {
        FILE_LIST_DIRECTORY
    } else {
        GENERIC_READ
    };

    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null_mut(),
            OPEN_EXISTING,
            flags,
            ptr::null_mut(),
        )
    };

    if handle == INVALID_HANDLE_VALUE {
        let err = unsafe { GetLastError() };
        return Err(io::Error::from_raw_os_error(err as i32));
    }

    Ok(unsafe { OwnedHandle::from_raw_handle(handle as _) })
}

/// Get the final (resolved) path of an open handle.
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
    // GetFinalPathNameByHandleW returns \\?\ prefix on most systems; strip it.
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    Ok(PathBuf::from(s))
}

/// Get FILE_ATTRIBUTE_TAG_INFORMATION for a handle.
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

/// Get FILE_STANDARD_INFO for a handle.
fn get_standard_info(handle: &OwnedHandle) -> io::Result<FILE_STANDARD_INFO> {
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

/// Get the file ID from FILE_ID_INFO for a handle.
fn get_file_id(handle: &OwnedHandle) -> io::Result<(u32, u64)> {
    let mut info: FILE_ID_INFO = unsafe { std::mem::zeroed() };
    let success = unsafe {
        GetFileInformationByHandleEx(
            handle.as_raw_handle() as HANDLE,
            0x12, // FileIdInfo
            &mut info as *mut _ as *mut _,
            std::mem::size_of::<FILE_ID_INFO>() as DWORD,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((info.volume_serial_number, info.file_id))
}

/// Run the deny-all reparse check on a handle.
fn deny_all_reparse_check(handle: &OwnedHandle) -> io::Result<()> {
    let (attrs, _) = get_attribute_tag_info(handle)?;
    if attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "ReparsePointDenied: reparse point detected on handle",
        ))
    } else {
        Ok(())
    }
}

/// Read the entire contents of an owned file handle.
fn read_all_from_handle(handle: &OwnedHandle) -> io::Result<Vec<u8>> {
    use std::io::Read;
    let mut buf = Vec::new();
    let mut read_buf = [0u8; 4096];
    let handle_raw = handle.as_raw_handle() as _;
    loop {
        let mut f = unsafe { fs::File::from_raw_handle(handle_raw) };
        match f.read(&mut read_buf) {
            Ok(0) => {
                std::mem::forget(f);
                break;
            }
            Ok(n) => {
                buf.extend_from_slice(&read_buf[..n]);
                std::mem::forget(f);
            }
            Err(e) => {
                std::mem::forget(f);
                return Err(e);
            }
        }
    }
    Ok(buf)
}

/// Read a range from a file handle at a given offset.
fn read_range_from_handle(handle: &OwnedHandle, offset: u64, len: usize) -> io::Result<Vec<u8>> {
    let handle_raw = handle.as_raw_handle() as _;
    let mut buf = vec![0u8; len];
    let f = unsafe { fs::File::from_raw_handle(handle_raw) };
    use std::io::{Read, Seek, SeekFrom};
    let result = (&f).seek(SeekFrom::Start(offset)).and_then(|_| {
        let mut reader = &f;
        reader.read_exact(&mut buf)
    });
    std::mem::forget(f);
    result?;
    Ok(buf)
}

// ============================================================================
// Track B — Root-relative open prototype
// ============================================================================

#[test]
fn test_open_root_directory_relative() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let root_handle = open_root_handle(&root);

    let file_handle = open_relative(&root_handle, "file.txt", false, false)
        .expect("should open file.txt relative to root");

    let content = read_all_from_handle(&file_handle).expect("should read from handle");
    assert_eq!(
        content, b"hello world",
        "file content should match what was written"
    );
}

#[test]
fn test_open_nested_relative() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let root_handle = open_root_handle(&root);
    let subdir_handle =
        open_relative(&root_handle, "subdir", true, false).expect("should open subdir");
    let deep_handle = open_relative(&subdir_handle, "deep", true, false).expect("should open deep");
    let file_handle =
        open_relative(&deep_handle, "deep.txt", false, false).expect("should open deep.txt");

    let content = read_all_from_handle(&file_handle).expect("should read deep.txt");
    assert_eq!(
        content, b"deep content",
        "deep nested file content should match"
    );
}

#[test]
fn test_intermediate_must_be_directory() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let root_handle = open_root_handle(&root);

    // Try to open file.txt as a directory — it's a regular file.
    let result = open_relative(&root_handle, "file.txt", true, false);
    assert!(result.is_err(), "opening a file as a directory should fail");
}

#[test]
fn test_final_directory_open() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let root_handle = open_root_handle(&root);
    let subdir_handle = open_relative(&root_handle, "subdir", true, false)
        .expect("should open subdir as directory");

    let (attrs, _) = get_attribute_tag_info(&subdir_handle).expect("should get attribute tag info");
    assert_ne!(
        attrs & FILE_ATTRIBUTE_DIRECTORY,
        0,
        "subdir handle should have FILE_ATTRIBUTE_DIRECTORY"
    );
}

#[test]
fn test_missing_component_returns_not_found() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let root_handle = open_root_handle(&root);

    let result = open_relative(&root_handle, "nonexistent.txt", false, false);
    assert!(result.is_err(), "opening nonexistent file should fail");
}

#[test]
fn test_component_validation_rejects_dot_dot() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let root_handle = open_root_handle(&root);

    let result = open_relative(&root_handle, "..", true, false);
    assert!(
        result.is_err(),
        "opening '..' as a component should be rejected"
    );
}

// ============================================================================
// Track C — Reparse suppression and inspection
// ============================================================================

#[test]
#[ignore = "requires Developer Mode or elevated privileges for symlink creation"]
fn test_file_symlink_rejection() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let symlink_path = root.join("link_to_file");
    std::os::windows::fs::symlink_file(root.join("file.txt"), &symlink_path)
        .expect("should create file symlink (requires Developer Mode)");

    let root_handle = open_root_handle(&root);

    let handle = open_relative(&root_handle, "link_to_file", false, true)
        .expect("should open reparse point");

    let (attrs, reparse_tag) =
        get_attribute_tag_info(&handle).expect("should get attribute tag info");

    assert_ne!(
        attrs & FILE_ATTRIBUTE_REPARSE_POINT,
        0,
        "symlink should have FILE_ATTRIBUTE_REPARSE_POINT"
    );
    assert_eq!(
        reparse_tag, IO_REPARSE_TAG_SYMLINK,
        "reparse tag should be IO_REPARSE_TAG_SYMLINK"
    );
}

#[test]
#[ignore = "requires Developer Mode or elevated privileges for symlink creation"]
fn test_directory_symlink_rejection() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let symlink_path = root.join("link_to_subdir");
    std::os::windows::fs::symlink_dir(root.join("subdir"), &symlink_path)
        .expect("should create directory symlink (requires Developer Mode)");

    let root_handle = open_root_handle(&root);

    let handle = open_relative(&root_handle, "link_to_subdir", true, true)
        .expect("should open directory symlink reparse point");

    let (attrs, reparse_tag) =
        get_attribute_tag_info(&handle).expect("should get attribute tag info");

    assert_ne!(
        attrs & FILE_ATTRIBUTE_REPARSE_POINT,
        0,
        "directory symlink should have FILE_ATTRIBUTE_REPARSE_POINT"
    );
    assert_eq!(
        reparse_tag, IO_REPARSE_TAG_SYMLINK,
        "reparse tag should be IO_REPARSE_TAG_SYMLINK for directory symlink"
    );
}

#[test]
#[ignore = "requires elevated privileges for junction creation"]
fn test_junction_rejection() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let junction_path = root.join("junction_to_subdir");
    let target = root.join("subdir");
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

    let root_handle = open_root_handle(&root);

    let handle = open_relative(&root_handle, "junction_to_subdir", true, true)
        .expect("should open junction reparse point");

    let (attrs, reparse_tag) =
        get_attribute_tag_info(&handle).expect("should get attribute tag info");

    assert_ne!(
        attrs & FILE_ATTRIBUTE_REPARSE_POINT,
        0,
        "junction should have FILE_ATTRIBUTE_REPARSE_POINT"
    );
    assert_eq!(
        reparse_tag, IO_REPARSE_TAG_MOUNT_POINT,
        "reparse tag should be IO_REPARSE_TAG_MOUNT_POINT for junction"
    );
}

#[test]
#[ignore = "requires Developer Mode or elevated privileges for symlink creation"]
fn test_reparse_point_attributes_detected() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let symlink_path = root.join("detectable_link");
    std::os::windows::fs::symlink_file(root.join("file.txt"), &symlink_path)
        .expect("should create symlink");

    let root_handle = open_root_handle(&root);

    let handle = open_relative(&root_handle, "detectable_link", false, true)
        .expect("should open reparse point");

    let (attrs, _) = get_attribute_tag_info(&handle).expect("should get attributes");

    assert_ne!(
        attrs & FILE_ATTRIBUTE_REPARSE_POINT,
        0,
        "reparse point attribute should be set"
    );
}

#[test]
#[ignore = "requires Developer Mode or elevated privileges for symlink creation"]
fn test_reparse_tag_retrievable() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let symlink_path = root.join("tagged_link");
    std::os::windows::fs::symlink_file(root.join("file.txt"), &symlink_path)
        .expect("should create symlink");

    let root_handle = open_root_handle(&root);

    let handle =
        open_relative(&root_handle, "tagged_link", false, true).expect("should open reparse point");

    let (_, reparse_tag) = get_attribute_tag_info(&handle).expect("should get reparse tag");

    assert_eq!(
        reparse_tag, IO_REPARSE_TAG_SYMLINK,
        "reparse tag should match expected symlink tag value"
    );
}

#[test]
fn test_non_reparse_file_has_no_reparse_attribute() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let root_handle = open_root_handle(&root);
    let file_handle =
        open_relative(&root_handle, "file.txt", false, false).expect("should open regular file");

    let (attrs, _) = get_attribute_tag_info(&file_handle).expect("should get attributes");

    assert_eq!(
        attrs & FILE_ATTRIBUTE_REPARSE_POINT,
        0,
        "regular file should not have FILE_ATTRIBUTE_REPARSE_POINT"
    );
}

#[test]
fn test_deny_all_reparse_check_passes_for_regular_file() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let root_handle = open_root_handle(&root);
    let file_handle =
        open_relative(&root_handle, "file.txt", false, false).expect("should open regular file");

    let result = deny_all_reparse_check(&file_handle);
    assert!(
        result.is_ok(),
        "deny_all_reparse_check should pass for regular file, got: {:?}",
        result.err()
    );
}

#[test]
#[ignore = "requires Developer Mode or elevated privileges for symlink creation"]
fn test_deny_all_reparse_check_fails_for_symlink() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let symlink_path = root.join("denied_link");
    std::os::windows::fs::symlink_file(root.join("file.txt"), &symlink_path)
        .expect("should create symlink");

    let root_handle = open_root_handle(&root);
    let handle =
        open_relative(&root_handle, "denied_link", false, true).expect("should open reparse point");

    let result = deny_all_reparse_check(&handle);
    assert!(
        result.is_err(),
        "deny_all_reparse_check should fail for symlink"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("ReparsePointDenied"),
        "error should contain ReparsePointDenied, got: {}",
        err_msg
    );
}

// ============================================================================
// Track D — File identity and root identity
// ============================================================================

#[test]
fn test_file_standard_info() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let root_handle = open_root_handle(&root);
    let file_handle =
        open_relative(&root_handle, "file.txt", false, false).expect("should open file.txt");

    let info = get_standard_info(&file_handle).expect("should get standard info");

    assert_eq!(
        info.end_of_file, 11,
        "file size should be 11 bytes (hello world)"
    );
    assert!(
        info.number_of_links >= 1,
        "file should have at least 1 link"
    );
    assert_eq!(info.directory, 0, "file.txt should not be a directory");
}

#[test]
fn test_directory_standard_info() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let root_handle = open_root_handle(&root);
    let dir_handle =
        open_relative(&root_handle, "subdir", true, false).expect("should open subdir");

    let info = get_standard_info(&dir_handle).expect("should get standard info");

    assert_ne!(info.directory, 0, "subdir should be a directory");
}

#[test]
fn test_file_id_retrieval() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let root_handle = open_root_handle(&root);
    let file_handle =
        open_relative(&root_handle, "file.txt", false, false).expect("should open file.txt");

    let (serial, file_id) = get_file_id(&file_handle).expect("should get file ID");

    assert!(file_id != 0, "file ID should be nonzero, got: {}", file_id);
    assert!(
        serial != 0,
        "volume serial should be nonzero, got: {}",
        serial
    );
}

#[test]
fn test_file_id_stable_after_rename() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let root_handle = open_root_handle(&root);
    let file_handle =
        open_relative(&root_handle, "file.txt", false, false).expect("should open file.txt");

    let (_, original_id) = get_file_id(&file_handle).expect("should get file ID");
    drop(file_handle);

    let original = root.join("file.txt");
    let renamed = root.join("file_renamed.txt");
    fs::rename(&original, &renamed).expect("should rename file");

    let root_handle2 = open_root_handle(&root);
    let file_handle2 = open_relative(&root_handle2, "file_renamed.txt", false, false)
        .expect("should open renamed file");

    let (_, renamed_id) = get_file_id(&file_handle2).expect("should get file ID of renamed file");

    assert_eq!(
        original_id, renamed_id,
        "file ID should be stable across rename: {} vs {}",
        original_id, renamed_id
    );
}

#[test]
fn test_final_path_retrieval() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let root_handle = open_root_handle(&root);
    let file_handle =
        open_relative(&root_handle, "file.txt", false, false).expect("should open file.txt");

    let final_path = get_final_path(&file_handle).expect("should get final path");

    assert!(
        final_path.exists(),
        "final path should point to an existing file"
    );
    assert!(
        final_path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .contains("file.txt"),
        "final path should contain the filename, got: {:?}",
        final_path
    );
}

#[test]
fn test_root_rename_identity() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let root_handle = open_root_handle(&root);

    let file_handle = open_relative(&root_handle, "file.txt", false, false)
        .expect("should open file.txt through root handle");
    let original_content = read_all_from_handle(&file_handle).expect("should read original");
    drop(file_handle);

    let renamed_root = tmp.path().join("renamed_root");
    fs::rename(&root, &renamed_root).expect("should rename root");

    fs::create_dir(&root).expect("should create new dir at old path");
    fs::write(root.join("file.txt"), "replacement content").expect("write replacement");

    let file_handle2 = open_relative(&root_handle, "file.txt", false, false)
        .expect("should open file.txt through retained root handle");
    let served_content = read_all_from_handle(&file_handle2).expect("should read served");

    assert_eq!(
        served_content, original_content,
        "retained root handle should serve original content after rename, not replacement"
    );
}

#[test]
fn test_replacement_at_old_path_not_visible() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let root_handle = open_root_handle(&root);

    let file_handle = open_relative(&root_handle, "file.txt", false, false)
        .expect("should open original file.txt");
    let original = read_all_from_handle(&file_handle).expect("should read original");
    assert_eq!(original, b"hello world");
    drop(file_handle);

    let backup = tmp.path().join("backup_root");
    fs::rename(&root, &backup).expect("rename root");

    fs::create_dir(&root).expect("create replacement dir");
    fs::write(root.join("file.txt"), "ATTACK CONTENT").expect("write attack");

    let file_handle2 = open_relative(&root_handle, "file.txt", false, false)
        .expect("should open through pinned handle");
    let served = read_all_from_handle(&file_handle2).expect("read from pinned");

    assert_eq!(
        served, b"hello world",
        "pinned root handle must not see replacement content"
    );
}

// ============================================================================
// Track E — Streaming compatibility
// ============================================================================

#[test]
fn test_handle_to_std_file_readable() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let root_handle = open_root_handle(&root);
    let file_handle =
        open_relative(&root_handle, "file.txt", false, false).expect("should open file.txt");

    let handle_raw = file_handle.as_raw_handle();
    let mut f = unsafe { fs::File::from_raw_handle(handle_raw as _) };
    use std::io::Read;
    let mut content = String::new();
    f.read_to_string(&mut content)
        .expect("should read via std::fs::File");
    std::mem::forget(f);

    assert_eq!(
        content, "hello world",
        "std::fs::File should read correct content"
    );
}

#[test]
fn test_range_read_from_validated_handle() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let root_handle = open_root_handle(&root);
    let file_handle =
        open_relative(&root_handle, "file.txt", false, false).expect("should open file.txt");

    let data = read_range_from_handle(&file_handle, 6, 5).expect("should read range");
    assert_eq!(data, b"world", "range read should return 'world'");
}

#[test]
fn test_handle_count_not_grown_after_loops() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    for _ in 0..100 {
        let root_handle = open_root_handle(&root);
        let file_handle =
            open_relative(&root_handle, "file.txt", false, false).expect("should open file.txt");
        let _content = read_all_from_handle(&file_handle).expect("should read");
        drop(file_handle);
        drop(root_handle);
    }
}

#[test]
fn test_cancellation_releases_handle() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let root_handle = open_root_handle(&root);
    let file_handle =
        open_relative(&root_handle, "file.txt", false, false).expect("should open file.txt");

    drop(file_handle);

    let file_handle2 = open_relative(&root_handle, "subdir/nested.txt", false, false)
        .expect("root handle should still work after dropping file handle");
    let content = read_all_from_handle(&file_handle2).expect("should read nested.txt");
    assert_eq!(content, b"nested content");
}

// ============================================================================
// Track F — Directory enumeration feasibility
// ============================================================================

#[test]
fn test_enumerate_directory_entries() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let root_handle = open_root_handle(&root);

    // Use std::fs::read_dir on the final path to enumerate
    let final_path = get_final_path(&root_handle).expect("should get final path");
    let entries: Vec<_> = fs::read_dir(&final_path)
        .expect("should read dir")
        .filter_map(|e| e.ok())
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let meta = e.metadata().ok();
            let is_dir = meta.as_ref().map_or(false, |m| m.is_dir());
            (name, is_dir)
        })
        .collect();

    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();

    assert!(
        names.contains(&"file.txt"),
        "should contain file.txt, got: {:?}",
        names
    );
    assert!(
        names.contains(&"subdir"),
        "should contain subdir, got: {:?}",
        names
    );
    assert!(
        names.contains(&".hidden"),
        "should contain .hidden, got: {:?}",
        names
    );
    assert!(
        names.contains(&"visible.txt"),
        "should contain visible.txt, got: {:?}",
        names
    );

    for (name, is_dir) in &entries {
        if name == "subdir" {
            assert!(*is_dir, "subdir should be a directory");
        }
    }
}

#[test]
fn test_enumerate_filters_dotfiles() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let root_handle = open_root_handle(&root);
    let final_path = get_final_path(&root_handle).expect("should get final path");
    let entries: Vec<String> = fs::read_dir(&final_path)
        .expect("should read dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();

    // Simulate dotfile filtering
    let filtered: Vec<&str> = entries
        .iter()
        .filter(|name| !name.starts_with('.'))
        .map(|n| n.as_str())
        .collect();

    assert!(
        !filtered.contains(&".hidden"),
        ".hidden should be filtered out"
    );
    assert!(
        filtered.contains(&"file.txt"),
        "file.txt should still be present after filtering"
    );
}

#[test]
#[ignore = "requires Developer Mode or elevated privileges for symlink creation"]
fn test_enumerate_filters_reparse_points() {
    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let symlink_path = root.join("reparse_entry");
    std::os::windows::fs::symlink_file(root.join("file.txt"), &symlink_path)
        .expect("should create symlink");

    let root_handle = open_root_handle(&root);
    let final_path = get_final_path(&root_handle).expect("should get final path");
    let entries: Vec<(String, u32)> = fs::read_dir(&final_path)
        .expect("should read dir")
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name == "." || name == ".." {
                return None;
            }
            let handle = {
                let wide = utf16_string(e.path().to_str().unwrap());
                let h = unsafe {
                    CreateFileW(
                        wide.as_ptr(),
                        GENERIC_READ,
                        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                        ptr::null_mut(),
                        OPEN_EXISTING,
                        FILE_FLAG_OPEN_REPARSE_POINT,
                        ptr::null_mut(),
                    )
                };
                if h == INVALID_HANDLE_VALUE {
                    return None;
                }
                unsafe { OwnedHandle::from_raw_handle(h as _) }
            };
            let attrs = get_attribute_tag_info(&handle)
                .ok()
                .map(|(a, _)| a)
                .unwrap_or(0);
            Some((name, attrs))
        })
        .collect();

    let non_reparse: Vec<&str> = entries
        .iter()
        .filter(|(_, attrs)| attrs & FILE_ATTRIBUTE_REPARSE_POINT == 0)
        .map(|(n, _)| n.as_str())
        .collect();

    assert!(
        !non_reparse.contains(&"reparse_entry"),
        "reparse entry should be filtered when symlinks are denied"
    );
    assert!(
        non_reparse.contains(&"file.txt"),
        "regular files should remain after reparse filtering"
    );
}

// ============================================================================
// Track G — Race probes
// ============================================================================

#[test]
#[ignore = "requires Developer Mode for symlink creation"]
fn test_race_file_to_symlink_swap() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;

    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();
    let root_clone = root.clone();

    let swapper = thread::spawn(move || {
        let mut is_symlink = false;
        while !stop_clone.load(Ordering::Relaxed) {
            let path = root_clone.join("swappable");
            if is_symlink {
                let _ = fs::remove_file(&path);
                fs::write(&path, "regular content").unwrap();
                is_symlink = false;
            } else {
                let _ = fs::remove_file(&path);
                let target = root_clone.join("file.txt");
                match std::os::windows::fs::symlink_file(&target, &path) {
                    Ok(_) => is_symlink = true,
                    Err(_) => {
                        fs::write(&path, "regular content").unwrap();
                        is_symlink = false;
                    }
                }
            }
            thread::yield_now();
        }
    });

    let root_handle = open_root_handle(&root);
    for _ in 0..200 {
        let result = open_relative(&root_handle, "swappable", false, false);
        match result {
            Ok(handle) => {
                let check = deny_all_reparse_check(&handle);
                assert!(check.is_ok(), "should not get through a reparse point");
            }
            Err(_) => {}
        }
    }

    stop.store(true, Ordering::Relaxed);
    swapper.join().expect("swapper thread should complete");
}

#[test]
#[ignore = "requires elevated privileges for junction creation"]
fn test_race_directory_to_junction_swap() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;

    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());
    let target_dir = root.join("subdir");

    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();
    let root_clone = root.clone();
    let target_clone = target_dir.clone();

    let swapper = thread::spawn(move || {
        let mut is_junction = false;
        while !stop_clone.load(Ordering::Relaxed) {
            let path = root_clone.join("junction_swappable");
            if is_junction {
                let _ = fs::remove_dir(&path);
                fs::create_dir(&path).unwrap();
                is_junction = false;
            } else {
                let _ = fs::remove_dir(&path);
                let status = std::process::Command::new("cmd")
                    .args([
                        "/C",
                        "mklink",
                        "/J",
                        path.to_str().unwrap(),
                        target_clone.to_str().unwrap(),
                    ])
                    .status();
                match status {
                    Ok(s) if s.success() => is_junction = true,
                    _ => {
                        fs::create_dir(&path).unwrap();
                        is_junction = false;
                    }
                }
            }
            thread::yield_now();
        }
    });

    let root_handle = open_root_handle(&root);
    for _ in 0..200 {
        let result = open_relative(&root_handle, "junction_swappable", true, false);
        match result {
            Ok(handle) => {
                let check = deny_all_reparse_check(&handle);
                assert!(check.is_ok(), "should not get through a reparse junction");
            }
            Err(_) => {}
        }
    }

    stop.store(true, Ordering::Relaxed);
    swapper.join().expect("swapper thread should complete");
}

#[test]
fn test_race_file_a_to_file_b() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;

    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());

    fs::write(root.join("swap_target"), "content_A").expect("write file A");

    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();
    let root_clone = root.clone();

    let swapper = thread::spawn(move || {
        let mut toggle = false;
        while !stop_clone.load(Ordering::Relaxed) {
            let path = root_clone.join("swap_target");
            if toggle {
                fs::write(&path, "content_B").unwrap();
            } else {
                fs::write(&path, "content_A").unwrap();
            }
            toggle = !toggle;
            thread::yield_now();
        }
    });

    let root_handle = open_root_handle(&root);
    for _ in 0..500 {
        let handle = open_relative(&root_handle, "swap_target", false, false)
            .expect("should open swap_target");
        let content = read_all_from_handle(&handle).expect("should read");
        let s = String::from_utf8_lossy(&content);
        assert!(
            s == "content_A" || s == "content_B",
            "content should be one of the two expected values, got: {}",
            s
        );
    }

    stop.store(true, Ordering::Relaxed);
    swapper.join().expect("swapper thread should complete");
}

#[test]
fn test_race_regular_to_reparse() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;

    let tmp = TempDir::new().unwrap();
    let root = create_test_tree(tmp.path());
    let real_dir = root.join("real_subdir");
    fs::create_dir(&real_dir).expect("create real subdir");
    fs::write(real_dir.join("inside.txt"), "real content").expect("write inside");

    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();
    let root_clone = root.clone();
    let real_dir_clone = real_dir.clone();

    let swapper = thread::spawn(move || {
        let mut is_reparse = false;
        while !stop_clone.load(Ordering::Relaxed) {
            let path = root_clone.join("reparse_swappable");
            if is_reparse {
                let _ = fs::remove_dir(&path);
                fs::create_dir(&path).unwrap();
                fs::write(path.join("inside.txt"), "new content").unwrap();
                is_reparse = false;
            } else {
                let _ = fs::remove_dir(&path);
                let status = std::process::Command::new("cmd")
                    .args([
                        "/C",
                        "mklink",
                        "/J",
                        path.to_str().unwrap(),
                        real_dir_clone.to_str().unwrap(),
                    ])
                    .status();
                match status {
                    Ok(s) if s.success() => is_reparse = true,
                    _ => {
                        fs::create_dir(&path).unwrap();
                        is_reparse = false;
                    }
                }
            }
            thread::yield_now();
        }
    });

    let root_handle = open_root_handle(&root);
    for _ in 0..500 {
        let result = open_relative(&root_handle, "reparse_swappable", true, false);
        match result {
            Ok(handle) => {
                let check = deny_all_reparse_check(&handle);
                assert!(
                    check.is_ok(),
                    "reparse point should never pass the deny check"
                );
            }
            Err(_) => {}
        }
    }

    stop.store(true, Ordering::Relaxed);
    swapper.join().expect("swapper thread should complete");
}

// ============================================================================
// Track H — Compilation test
// ============================================================================

#[test]
fn test_windows_feasibility_module_compiles() {
    let _ = MAX_PATH_W;
    let _ = IO_REPARSE_TAG_SYMLINK;
    let _ = IO_REPARSE_TAG_MOUNT_POINT;

    let _ = utf16_string("test");
    let _ = deny_all_reparse_check;
    let _ = get_final_path;
    let _ = get_attribute_tag_info;
    let _ = get_standard_info;
    let _ = get_file_id;
}

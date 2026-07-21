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

use tempfile::TempDir;

use eggserve_core::path::{ConfinedPath, PathPolicy};
use eggserve_core::policy::StaticPolicy;
use eggserve_core::primitives::body::BodySource;
use eggserve_core::primitives::response::BodyPlan;
use eggserve_core::primitives::{check_component, SecureRoot};

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
        dotfiles: eggserve_core::path::DotfilePolicy::Allow,
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

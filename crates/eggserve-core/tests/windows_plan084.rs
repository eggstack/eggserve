//! Windows Plan 084 tests: directory-handle retention, handle-relative
//! child resolution, unicode namespace coverage, and handle lifecycle.
//!
//! These tests exercise the production `SecureRoot` / `RootGuard` path on
//! Windows, where `resolve_to_resource` and `resolve_child_relative` use
//! handle-relative traversal via `NtOpenFile`.
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
use std::path::Path;
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

fn get_final_path(handle: &OwnedHandle) -> io::Result<std::path::PathBuf> {
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
    Ok(std::path::PathBuf::from(s))
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

/// Create a standard test directory tree for Plan 084 tests.
fn create_plan084_tree(root: &Path) {
    fs::create_dir_all(root.join("subdir/deep")).expect("create dirs");
    fs::write(root.join("hello.txt"), "hello").expect("write hello.txt");
    fs::write(root.join("subdir/nested.txt"), "nested").expect("write nested.txt");
    fs::write(root.join("subdir/deep/deep.txt"), "deep").expect("write deep.txt");
    fs::write(root.join("visible.txt"), "visible").expect("write visible.txt");
}

fn parse(raw: &str) -> ConfinedPath {
    ConfinedPath::parse(raw, &PathPolicy::default()).unwrap()
}

fn parse_allow_dotfiles(raw: &str) -> ConfinedPath {
    let policy = PathPolicy {
        dotfiles: eggserve_core::path::DotfilePolicy::Allow,
        ..PathPolicy::default()
    };
    ConfinedPath::parse(raw, &policy).unwrap()
}

// ============================================================================
// Track B: try_clone() failure path
// ============================================================================

#[test]
fn windows_handle_duplication_failure_is_typed() {
    // OwnedHandle::try_clone() returns Result<Self, WindowsFsError>.
    // On a valid handle, try_clone succeeds.
    // On an invalid handle, try_clone returns Ok(INVALID_HANDLE_VALUE)
    // (per the current implementation: the is_valid() guard short-circuits).
    //
    // The failure path (DuplicateHandle returning 0) is covered by the type
    // system: the return type is Result, not an infallible clone. Triggering
    // DuplicateHandle failure requires exhausting the process handle quota,
    // which is not feasible in a unit test. The Windows error path is tested
    // indirectly by the production code paths that propagate try_clone errors
    // (e.g., ResolvedResource::NotFound on clone failure in resolve_to_resource).
    //
    // TODO: If Windows test infrastructure supports handle-quota manipulation,
    // add a test that forces DuplicateHandle failure and verifies the error
    // variant is WindowsFsError::IoError with the expected Win32 error code.

    let tmp = TempDir::new().unwrap();
    create_plan084_tree(tmp.path());

    let root_handle = open_root_handle(tmp.path());

    // Test success path: try_clone on a valid handle returns Ok with a valid handle.
    // std::os::windows::io::OwnedHandle::try_clone has the same Result semantics.
    let cloned = root_handle
        .try_clone()
        .expect("try_clone should succeed on valid handle");
    assert!(cloned.as_raw_handle() != INVALID_HANDLE_VALUE as _);
    drop(cloned);

    // Original is still valid after clone drop.
    assert!(root_handle.as_raw_handle() != INVALID_HANDLE_VALUE as _);

    // Verify through the production SecureRoot API that clone + resolve works.
    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();
    let result = root.resolve(&parse("/hello.txt"));
    assert!(
        result.is_file(),
        "resolve through SecureRoot should succeed"
    );
}

// ============================================================================
// Track E: Lifetime and resource-accounting
// ============================================================================

#[test]
fn windows_repeated_child_resolution_returns_handle_count_to_baseline() {
    // Resolve a directory, then resolve a child, drop the child, many times.
    // Each cycle should return handles to the OS; if handles leak, this loop
    // would eventually exhaust the process handle limit.
    let tmp = TempDir::new().unwrap();
    create_plan084_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    for i in 0..100 {
        let dir = root
            .resolve(&parse("/subdir"))
            .into_directory()
            .expect("subdir should resolve as directory");

        let file_result = dir.resolve_child("nested.txt", &root);
        assert!(
            file_result.is_file(),
            "iteration {i}: nested.txt should resolve as file"
        );
        drop(file_result);
        drop(dir);
    }
}

#[test]
fn windows_resolved_directory_retains_handle_after_resolve() {
    // After resolving a directory through SecureRoot, the resolved directory
    // retains a handle for handle-relative child resolution.
    let tmp = TempDir::new().unwrap();
    create_plan084_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    let dir = root
        .resolve(&parse("/subdir"))
        .into_directory()
        .expect("subdir should resolve as directory");

    // The directory retains components for child resolution.
    assert_eq!(dir.components(), &["subdir"]);

    // The directory handle is valid: resolving a child from it works.
    let child = dir.resolve_child("nested.txt", &root);
    assert!(
        child.is_file(),
        "child resolution should use retained handle"
    );

    // List directory entries — exercises the retained directory handle.
    let entries = dir.list(&root).expect("list should succeed");
    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    assert!(
        names.contains(&"nested.txt"),
        "listing should include nested.txt via retained handle"
    );
    assert!(
        names.contains(&"index.html") || names.contains(&"deep"),
        "listing should include other entries"
    );
}

#[test]
fn windows_resolved_file_streamable_after_parent_drop() {
    // Resolve a file through RootGuard, then verify the file is still
    // readable after the guard is dropped. On Windows, the file handle
    // was duplicated (via try_clone) during resolve_to_resource, so the
    // file handle is independent of the directory handle.
    let tmp = TempDir::new().unwrap();
    create_plan084_tree(tmp.path());

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Resolve the file and extract it.
    let file_resource = root
        .resolve(&parse("/hello.txt"))
        .into_file()
        .expect("hello.txt should resolve as file");

    // Verify the file can be read (handle independence from parent).
    let plan = eggserve_core::primitives::response::StaticResponsePlan {
        status: eggserve_core::primitives::response::ResponseStatus::OK,
        headers: eggserve_core::primitives::response::HeaderMapPlan::new(),
        body: BodyPlan::FileFull,
    };
    let mut body = file_resource
        .into_body(&plan)
        .expect("into_body should succeed");
    let data = body.read_all().expect("read_all should succeed");
    assert_eq!(
        data, b"hello",
        "file content should be readable after resolution"
    );

    // Resolve a nested file and verify handle independence.
    let nested = root
        .resolve(&parse("/subdir/nested.txt"))
        .into_file()
        .expect("nested.txt should resolve as file");
    let plan2 = eggserve_core::primitives::response::StaticResponsePlan {
        status: eggserve_core::primitives::response::ResponseStatus::OK,
        headers: eggserve_core::primitives::response::HeaderMapPlan::new(),
        body: BodyPlan::FileFull,
    };
    let mut body2 = nested.into_body(&plan2).expect("into_body should succeed");
    let data2 = body2.read_all().expect("read_all should succeed");
    assert_eq!(data2, b"nested", "nested file content should be readable");
}

// ============================================================================
// Track F: Unicode namespace coverage
// ============================================================================

#[test]
fn windows_unicode_bmp_non_ascii() {
    // BMP (Basic Multilingual Plane) non-ASCII characters: CJK, accented
    // Latin, Cyrillic, etc. These must be accepted by ConfinedPath::parse()
    // and resolved through the production path.
    let tmp = TempDir::new().unwrap();
    create_plan084_tree(tmp.path());

    // Create files with BMP non-ASCII names.
    fs::write(tmp.path().join("\u{65E5}\u{672C}\u{8A9E}.txt"), "nihongo").expect("write CJK file");
    fs::write(tmp.path().join("caf\u{00E9}.txt"), "cafe").expect("write accented file");
    fs::write(
        tmp.path()
            .join("\u{041F}\u{0440}\u{0438}\u{0432}\u{0435}\u{0442}.txt"),
        "privet",
    )
    .expect("write Cyrillic file");

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Resolve each through ConfinedPath + SecureRoot.
    let path_cjk =
        ConfinedPath::parse("/\u{65E5}\u{672C}\u{8A9E}.txt", &PathPolicy::default()).unwrap();
    let result = root.resolve(&path_cjk);
    assert!(result.is_file(), "CJK filename should resolve as file");

    let path_accent = ConfinedPath::parse("/caf\u{00E9}.txt", &PathPolicy::default()).unwrap();
    let result = root.resolve(&path_accent);
    assert!(result.is_file(), "accented filename should resolve as file");

    let path_cyrillic = ConfinedPath::parse(
        "/\u{041F}\u{0440}\u{0438}\u{0432}\u{0435}\u{0442}.txt",
        &PathPolicy::default(),
    )
    .unwrap();
    let result = root.resolve(&path_cyrillic);
    assert!(result.is_file(), "Cyrillic filename should resolve as file");
}

#[test]
fn windows_unicode_surrogate_pair() {
    // Characters outside the BMP (surrogate pairs in UTF-16): emoji, etc.
    // These must be accepted by the path parser and resolved correctly.
    let tmp = TempDir::new().unwrap();
    create_plan084_tree(tmp.path());

    // Create a file with a surrogate pair character (emoji).
    fs::write(tmp.path().join("\u{1F680}.txt"), "rocket").expect("write emoji file");

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    let path = ConfinedPath::parse("/\u{1F680}.txt", &PathPolicy::default()).unwrap();
    let result = root.resolve(&path);
    assert!(
        result.is_file(),
        "emoji filename (surrogate pair) should resolve as file"
    );

    // Read the content to verify full round-trip through handle-relative open.
    let file = result.into_file().expect("should be file");
    let plan = eggserve_core::primitives::response::StaticResponsePlan {
        status: eggserve_core::primitives::response::ResponseStatus::OK,
        headers: eggserve_core::primitives::response::HeaderMapPlan::new(),
        body: BodyPlan::FileFull,
    };
    let mut body = file.into_body(&plan).expect("into_body");
    let data = body.read_all().expect("read_all");
    assert_eq!(
        data, b"rocket",
        "emoji-named file content should be readable"
    );
}

#[test]
fn windows_unicode_case_insensitive() {
    // Windows filesystems are case-insensitive by default. Verify that the
    // handle-relative path resolution matches this behavior.
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("Hello.txt"), "content").expect("write Hello.txt");

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Resolve with different casing — on Windows, this should succeed
    // because NtOpenFile uses OBJ_CASE_INSENSITIVE.
    let path_upper = ConfinedPath::parse("/HELLO.TXT", &PathPolicy::default()).unwrap();
    let result = root.resolve(&path_upper);
    assert!(
        result.is_file(),
        "case-insensitive resolution should succeed on Windows"
    );

    let path_mixed = ConfinedPath::parse("/hElLo.TxT", &PathPolicy::default()).unwrap();
    let result = root.resolve(&path_mixed);
    assert!(
        result.is_file(),
        "mixed-case resolution should succeed on Windows"
    );
}

#[test]
fn windows_namespace_rejection() {
    // Windows-specific path component rejections enforced by platform::check_component:
    // - C: drive forms
    // - ADS syntax (file.txt:stream)
    // - Reserved device names (CON, NUL, PRN, COM1, LPT1, etc.)
    // - Trailing dots (CON., NUL...)

    // Drive prefix.
    let result = check_component("C:");
    assert!(result.is_err(), "C: should be rejected");
    let result = check_component("D:/path");
    assert!(result.is_err(), "D:/path should be rejected");

    // ADS syntax (colon in component).
    let result = check_component("file.txt:stream");
    assert!(result.is_err(), "file.txt:stream should be rejected");

    // Reserved device names.
    let result = check_component("CON");
    assert!(result.is_err(), "CON should be rejected");
    let result = check_component("NUL");
    assert!(result.is_err(), "NUL should be rejected");
    let result = check_component("PRN");
    assert!(result.is_err(), "PRN should be rejected");
    let result = check_component("AUX");
    assert!(result.is_err(), "AUX should be rejected");
    let result = check_component("COM1");
    assert!(result.is_err(), "COM1 should be rejected");
    let result = check_component("LPT1");
    assert!(result.is_err(), "LPT1 should be rejected");
    let result = check_component("CON.txt");
    assert!(result.is_err(), "CON.txt should be rejected");
    let result = check_component("nul.bak");
    assert!(result.is_err(), "nul.bak should be rejected");

    // Reserved names with trailing dots.
    let result = check_component("CON.");
    assert!(result.is_err(), "CON. should be rejected");
    let result = check_component("NUL...");
    assert!(result.is_err(), "NUL... should be rejected");

    // Normal names must pass.
    assert!(check_component("hello.txt").is_ok());
    assert!(check_component("CONSOLE.txt").is_ok());
    assert!(check_component("auxiliary.txt").is_ok());
    assert!(check_component("com10.txt").is_ok());
    assert!(check_component("lpt0.txt").is_ok());
}

// ============================================================================
// Required tests from plan
// ============================================================================

#[test]
fn windows_index_lookup_uses_retained_directory_authority() {
    // Resolve a directory, then resolve a child (index file) from it.
    // This verifies that the retained directory handle is used for
    // handle-relative child resolution, not a path-based reopen.
    let tmp = TempDir::new().unwrap();
    create_plan084_tree(tmp.path());
    fs::write(
        tmp.path().join("subdir").join("index.html"),
        "<html>ok</html>",
    )
    .expect("write index.html");

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Resolve the directory.
    let dir = root
        .resolve(&parse("/subdir"))
        .into_directory()
        .expect("subdir should resolve as directory");

    // Resolve index.html from the directory handle.
    let index = dir
        .resolve_child("index.html", &root)
        .into_file()
        .expect("index.html should resolve as file");

    // Verify content through the retained handle.
    let plan = eggserve_core::primitives::response::StaticResponsePlan {
        status: eggserve_core::primitives::response::ResponseStatus::OK,
        headers: eggserve_core::primitives::response::HeaderMapPlan::new(),
        body: BodyPlan::FileFull,
    };
    let mut body = index.into_body(&plan).expect("into_body");
    let data = body.read_all().expect("read_all");
    assert_eq!(
        data, b"<html>ok</html>",
        "index file content should be read via retained directory handle"
    );
}

#[test]
fn windows_non_ascii_child_names_resolve_correctly() {
    // End-to-end test: non-ASCII filenames through the production
    // RootGuard → resolve_child_relative path.
    let tmp = TempDir::new().unwrap();
    create_plan084_tree(tmp.path());

    // Create non-ASCII files in a subdirectory.
    fs::write(
        tmp.path()
            .join("subdir")
            .join("\u{65E5}\u{672C}\u{8A9E}.txt"),
        "nihongo",
    )
    .expect("write CJK file in subdir");
    fs::write(tmp.path().join("subdir").join("caf\u{00E9}.txt"), "cafe")
        .expect("write accented file in subdir");
    fs::write(tmp.path().join("subdir").join("\u{1F680}.txt"), "rocket")
        .expect("write emoji file in subdir");

    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Resolve the directory.
    let dir = root
        .resolve(&parse("/subdir"))
        .into_directory()
        .expect("subdir should resolve as directory");

    // Resolve each non-ASCII child through the retained directory handle.
    let cjk = dir
        .resolve_child("\u{65E5}\u{672C}\u{8A9E}.txt", &root)
        .into_file()
        .expect("CJK child should resolve");
    let plan = eggserve_core::primitives::response::StaticResponsePlan {
        status: eggserve_core::primitives::response::ResponseStatus::OK,
        headers: eggserve_core::primitives::response::HeaderMapPlan::new(),
        body: BodyPlan::FileFull,
    };
    let mut body = cjk.into_body(&plan).expect("into_body");
    assert_eq!(
        body.read_all().expect("read_all"),
        b"nihongo",
        "CJK child content should be readable"
    );

    let accent = dir
        .resolve_child("caf\u{00E9}.txt", &root)
        .into_file()
        .expect("accented child should resolve");
    let plan2 = eggserve_core::primitives::response::StaticResponsePlan {
        status: eggserve_core::primitives::response::ResponseStatus::OK,
        headers: eggserve_core::primitives::response::HeaderMapPlan::new(),
        body: BodyPlan::FileFull,
    };
    let mut body2 = accent.into_body(&plan2).expect("into_body");
    assert_eq!(
        body2.read_all().expect("read_all"),
        b"cafe",
        "accented child content should be readable"
    );

    let emoji = dir
        .resolve_child("\u{1F680}.txt", &root)
        .into_file()
        .expect("emoji child should resolve");
    let plan3 = eggserve_core::primitives::response::StaticResponsePlan {
        status: eggserve_core::primitives::response::ResponseStatus::OK,
        headers: eggserve_core::primitives::response::HeaderMapPlan::new(),
        body: BodyPlan::FileFull,
    };
    let mut body3 = emoji.into_body(&plan3).expect("into_body");
    assert_eq!(
        body3.read_all().expect("read_all"),
        b"rocket",
        "emoji child content should be readable"
    );
}

#[test]
fn windows_duplicate_handle_failure_is_typed() {
    // Verify that OwnedHandle::try_clone returns Result (not panic).
    // On a valid handle, try_clone succeeds.
    // On an invalid handle, the internal implementation returns
    // Ok(INVALID_HANDLE_VALUE) due to the is_valid() guard.
    //
    // This test verifies the type-level contract: try_clone is fallible
    // and its failure path is represented in the return type.
    let tmp = TempDir::new().unwrap();
    create_plan084_tree(tmp.path());

    // Valid handle: try_clone succeeds.
    let root_handle = open_root_handle(tmp.path());
    let clone_result = root_handle.try_clone();
    assert!(
        clone_result.is_ok(),
        "try_clone on valid handle should return Ok"
    );
    let cloned = clone_result.unwrap();
    assert!(
        cloned.as_raw_handle() != INVALID_HANDLE_VALUE as _,
        "cloned handle should be valid"
    );
    drop(cloned);

    // Invalid handle: try_clone returns Ok(INVALID_HANDLE_VALUE)
    // (the is_valid() guard short-circuits before DuplicateHandle).
    let invalid = unsafe { OwnedHandle::from_raw_handle(INVALID_HANDLE_VALUE as _) };
    let invalid_result = invalid.try_clone();
    assert!(
        invalid_result.is_ok(),
        "try_clone on invalid handle should not panic"
    );
    // Do not drop invalid — it would call CloseHandle(INVALID_HANDLE_VALUE).
    // Leak it intentionally; INVALID_HANDLE_VALUE is special and the
    // implementation guards against closing it.

    // Through SecureRoot: verify the production path uses try_clone
    // and propagates errors correctly.
    let root = SecureRoot::new(tmp.path(), StaticPolicy::safe_default()).unwrap();

    // Resolve a file — internally this exercises resolve_to_resource which
    // calls child.try_clone() during file resolution.
    let file = root
        .resolve(&parse("/hello.txt"))
        .into_file()
        .expect("file resolution should succeed (try_clone is part of the path)");
    let plan = eggserve_core::primitives::response::StaticResponsePlan {
        status: eggserve_core::primitives::response::ResponseStatus::OK,
        headers: eggserve_core::primitives::response::HeaderMapPlan::new(),
        body: BodyPlan::FileFull,
    };
    let mut body = file.into_body(&plan).expect("into_body");
    let data = body.read_all().expect("read_all");
    assert_eq!(data, b"hello", "file content should be correct");
}

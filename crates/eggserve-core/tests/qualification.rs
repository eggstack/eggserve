//! Windows qualification infrastructure for Plan 090 Track D.
//!
//! Provides capability detection, qualification-mode gating, and the
//! `blocked!` macro for tests that cannot create required fixtures.
//!
//! # Modes
//!
//! - **Standard CI** (`EGGSERVE_WINDOWS_QUALIFY` not set): Tests that
//!   require Developer Mode, elevated privileges, or specialized fixtures
//!   are expected to be blocked. The `blocked!` macro produces a
//!   `blocked-fixture:` message on stderr and panics, which CI evidence
//!   collection interprets as a blocked (non-failing) result.
//!
//! - **Qualification** (`EGGSERVE_WINDOWS_QUALIFY=1`): All tests run.
//!   Fixtures that still cannot be created produce a real test failure.

#[cfg(windows)]
use std::fs;
use std::path::Path;

/// Returns `true` if the environment is in Windows qualification mode.
///
/// Qualification mode is activated by setting `EGGSERVE_WINDOWS_QUALIFY=1`.
/// In this mode, tests that normally block due to missing capabilities
/// instead run and fail if the fixture cannot be created.
pub fn is_qualification_mode() -> bool {
    std::env::var("EGGSERVE_WINDOWS_QUALIFY")
        .map(|v| v == "1" || v == "true")
        .unwrap_or(false)
}

/// Record of environment capabilities for the qualification preflight.
#[derive(Debug, Clone)]
pub struct Capabilities {
    pub os_version: String,
    pub arch: String,
    pub filesystem_type: String,
    pub developer_mode: bool,
    pub symlink_dir_privilege: bool,
    pub junction_creation: bool,
    pub is_ntfs: bool,
    pub is_local_volume: bool,
}

/// Detect environment capabilities. Must run on Windows.
#[cfg(windows)]
pub fn detect_capabilities(test_root: &Path) -> Capabilities {
    let os_version = std::env::var("OS").unwrap_or_else(|_| "unknown".to_string());
    let arch = std::env::consts::ARCH.to_string();

    let fs_type = get_filesystem_type(test_root);
    let is_ntfs = fs_type.eq_ignore_ascii_case("NTFS");

    // Probe Developer Mode by attempting file symlink creation.
    let test_file = test_root.join("cap_probe_file.txt");
    let _ = fs::write(&test_file, "probe");
    let dm_link = test_root.join("cap_probe_dm_link");
    let developer_mode = std::os::windows::fs::symlink_file(&test_file, &dm_link).is_ok();
    if developer_mode {
        let _ = fs::remove_file(&dm_link);
    }

    // Probe directory symlink privilege.
    let dir_link = test_root.join("cap_probe_dir_link");
    let symlink_dir_privilege = std::os::windows::fs::symlink_dir(test_root, &dir_link).is_ok();
    if symlink_dir_privilege {
        let _ = fs::remove_dir(&dir_link);
    }

    // Probe junction creation via mklink /J.
    let junction_target = test_root.join("subdir");
    let junction_link = test_root.join("cap_probe_junction");
    let junction_creation = std::process::Command::new("cmd")
        .args([
            "/C",
            "mklink",
            "/J",
            junction_link.to_str().unwrap(),
            junction_target.to_str().unwrap(),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if junction_creation {
        let _ = fs::remove_dir(&junction_link);
    }

    // Probe local volume (not SMB/network/cloud).
    // Heuristic: if GetVolumeInformation succeeds and the path is on a
    // fixed drive, it's local. Full validation requires Win32 API.
    let is_local_volume = is_ntfs; // NTFS implies local for our purposes

    let _ = fs::remove_file(&test_file);

    Capabilities {
        os_version,
        arch,
        filesystem_type: fs_type,
        developer_mode,
        symlink_dir_privilege,
        junction_creation,
        is_ntfs,
        is_local_volume,
    }
}

/// Stub for non-Windows platforms.
#[cfg(not(windows))]
pub fn detect_capabilities(_test_root: &Path) -> Capabilities {
    Capabilities {
        os_version: "non-windows".to_string(),
        arch: std::env::consts::ARCH.to_string(),
        filesystem_type: "non-windows".to_string(),
        developer_mode: false,
        symlink_dir_privilege: false,
        junction_creation: false,
        is_ntfs: false,
        is_local_volume: false,
    }
}

/// Emit structured capability output for evidence collection.
pub fn emit_capabilities(cap: &Capabilities) {
    eprintln!("=== Windows Qualification Capabilities ===");
    eprintln!("os: {}", cap.os_version);
    eprintln!("arch: {}", cap.arch);
    eprintln!("filesystem: {}", cap.filesystem_type);
    eprintln!("is-ntfs: {}", cap.is_ntfs);
    eprintln!("is-local-volume: {}", cap.is_local_volume);
    eprintln!("developer-mode: {}", cap.developer_mode);
    eprintln!("symlink-dir-privilege: {}", cap.symlink_dir_privilege);
    eprintln!("junction-creation: {}", cap.junction_creation);
    eprintln!("qualification-mode: {}", is_qualification_mode());
    eprintln!("==========================================");
}

/// Terminate a test because a required fixture cannot be created.
///
/// In qualification mode, this panics (real test failure).
/// In standard CI, this panics with a `blocked-fixture:` prefix that
/// evidence collection interprets as a blocked result.
///
/// # Usage
///
/// ```ignore
/// if !capability_available {
///     blocked!("symlink creation requires Developer Mode");
/// }
/// ```
#[macro_export]
macro_rules! blocked {
    ($msg:expr $(, $arg:expr)*) => {
        if $crate::qualification::is_qualification_mode() {
            panic!("QUALIFICATION FAILURE: {} (qualification mode — fixture must succeed)", format_args!($msg $(, $arg)*));
        } else {
            panic!("blocked-fixture: {} (standard CI — expected block)", format_args!($msg $(, $arg)*));
        }
    };
}

/// Assert that a capability is available, blocking the test if not.
///
/// In qualification mode, failure is a real test failure.
/// In standard CI, failure produces a `blocked-fixture:` panic.
#[macro_export]
macro_rules! require_capability {
    ($cap:expr, $name:expr) => {
        if !$cap {
            $crate::blocked!("{} not available", $name);
        }
    };
}

// ============================================================================
// Filesystem type detection (Windows)
// ============================================================================

#[cfg(windows)]
fn get_filesystem_type(path: &Path) -> String {
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    extern "system" {
        fn GetVolumeInformationW(
            lp_root_path_name: *const u16,
            lp_volume_name_buffer: *mut u16,
            n_volume_name_size: u32,
            lp_volume_serial_number: *mut u32,
            lp_maximum_component_length: *mut u32,
            lp_file_system_flags: *mut u32,
            lp_file_system_name_buffer: *mut u16,
            n_file_system_name_size: u32,
        ) -> i32;
    }

    let wide: Vec<u16> = path
        .to_str()
        .map(|s| s.encode_utf16().chain(std::iter::once(0)).collect())
        .unwrap_or_default();

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
            fs_name.len() as u32,
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

#[cfg(not(windows))]
#[allow(dead_code)]
fn get_filesystem_type(_path: &Path) -> String {
    "non-windows".to_string()
}

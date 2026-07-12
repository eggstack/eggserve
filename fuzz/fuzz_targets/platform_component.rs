#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::path::platform::{check_component, has_windows_drive_prefix, is_windows_reserved_name};

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = check_component(s);

        let drive = has_windows_drive_prefix(s);
        let reserved = is_windows_reserved_name(s);

        // Drive prefix requires at least 2 bytes: alphabetic + ':'
        if s.len() < 2 {
            assert!(!drive);
        } else {
            let bytes = s.as_bytes();
            if drive {
                assert!(bytes[0].is_ascii_alphabetic());
                assert_eq!(bytes[1], b':');
            }
        }

        // Reserved name: if matched, the base (before first dot) uppercased must be one of the 20 names
        if reserved {
            let base = s.split('.').next().unwrap_or("");
            let name = base.trim_end_matches('.');
            assert!(!name.is_empty());
            let upper = name.to_ascii_uppercase();
            assert!(
                matches!(upper.as_str(),
                    "CON" | "PRN" | "AUX" | "NUL"
                    | "COM1" | "COM2" | "COM3" | "COM4" | "COM5"
                    | "COM6" | "COM7" | "COM8" | "COM9"
                    | "LPT1" | "LPT2" | "LPT3" | "LPT4" | "LPT5"
                    | "LPT6" | "LPT7" | "LPT8" | "LPT9"
                ),
                "reserved_name returned true for non-reserved: {}",
                upper
            );
        }
    }
});

#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::primitives::canonical::{StatusCode, ResponseConstructionError};

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }
    // Interpret first two bytes as a u16 (big-endian)
    let code = u16::from_be_bytes([data[0], data[1]]);

    let result = StatusCode::new(code);

    match result {
        Ok(status) => {
            // Valid codes are in 100..=999
            assert!(code >= 100 && code <= 999);
            // Round-trip
            assert_eq!(status.as_u16(), code);
            // Classification is mutually exclusive
            let classes = [
                status.is_informational(),
                status.is_success(),
                status.is_redirection(),
                status.is_client_error(),
                status.is_server_error(),
            ];
            let active = classes.iter().filter(|&&c| c).count();
            assert!(active <= 1, "multiple classes active for code {}", code);
            // Informational status codes should not permit payload body
            if status.is_informational() {
                assert!(!status.permits_payload_body());
            }
            // 204 and 304 should not permit payload body
            if code == 204 || code == 304 {
                assert!(!status.permits_payload_body());
            }
            // Display round-trip
            assert_eq!(format!("{}", status), format!("{}", code));
            // Into<u16> round-trip
            let back: u16 = status.into();
            assert_eq!(back, code);
        }
        Err(e) => {
            // Invalid codes (0, <100, or >999) produce InvalidStatus
            assert!(
                matches!(e, ResponseConstructionError::InvalidStatus(c) if c == code),
                "unexpected error: {:?}",
                e
            );
        }
    }
});

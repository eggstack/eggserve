#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::primitives::http::validate_request_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let result = validate_request_target(s);

        if let Ok(()) = result {
            // Valid targets must start with /
            assert!(s.starts_with('/'), "valid target does not start with /: {:?}", s);
            // Must not be empty
            assert!(!s.is_empty(), "empty target passed validation");
            // Must not contain whitespace
            assert!(!s.contains(char::is_whitespace), "whitespace in valid target: {:?}", s);
        }
    }
});

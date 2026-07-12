#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::primitives::http::{validate_method, validate_request_body};

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let method_result = validate_method(s);

        match method_result {
            Ok(method) => {
                // Only GET and HEAD are accepted
                assert!(
                    method == eggserve_core::primitives::http::ReadOnlyMethod::Get
                        || method == eggserve_core::primitives::http::ReadOnlyMethod::Head
                );
                assert!(method.as_str() == s);
            }
            Err(_) => {
                // Any other method is rejected
            }
        }

        // Also test body validation with arbitrary CL/TE values
        let _ = validate_request_body(Some(s), None, 1024);
        let _ = validate_request_body(None, Some(s), 1024);
        let _ = validate_request_body(Some(s), Some(s), 1024);
    }
});

#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::primitives::http::{validate_method, validate_request_body};
use eggserve_core::primitives::method::{Method, MethodError};

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Fuzz Method construction
        let method_result = Method::new(s);
        match method_result {
            Ok(method) => {
                assert_eq!(method.as_str(), s);
                assert!(!method.as_str().is_empty());
                if method.is_safe() {
                    assert!(matches!(method.as_str(), "GET" | "HEAD" | "OPTIONS" | "TRACE"));
                }
                if method.is_idempotent() {
                    assert!(matches!(
                        method.as_str(),
                        "GET" | "HEAD" | "PUT" | "DELETE" | "OPTIONS" | "TRACE"
                    ));
                }
                assert_eq!(format!("{}", method), s);
            }
            Err(e) => {
                assert!(
                    matches!(e, MethodError::Empty | MethodError::InvalidToken),
                    "unexpected error variant: {:?}",
                    e
                );
                if e == MethodError::Empty {
                    assert!(s.is_empty());
                }
            }
        }

        // Fuzz validate_method (read-only method validation)
        let validate_result = validate_method(s);
        match validate_result {
            Ok(method) => {
                assert!(
                    method == eggserve_core::primitives::http::ReadOnlyMethod::Get
                        || method == eggserve_core::primitives::http::ReadOnlyMethod::Head
                );
                assert!(method.as_str() == s);
            }
            Err(_) => {}
        }

        // Fuzz body validation with arbitrary CL/TE values
        let _ = validate_request_body(Some(s), None, 1024);
        let _ = validate_request_body(None, Some(s), 1024);
        let _ = validate_request_body(Some(s), Some(s), 1024);
    }
});

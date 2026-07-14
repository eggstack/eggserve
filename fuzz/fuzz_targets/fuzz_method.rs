#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::primitives::method::{Method, MethodError};

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let result = Method::new(s);

        match result {
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
    }
});

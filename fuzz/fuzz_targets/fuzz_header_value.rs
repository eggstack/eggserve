#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::primitives::header_block::{HeaderValue, HeaderError};

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let result = HeaderValue::new(s);

        match result {
            Ok(value) => {
                // Round-trip: the stored value matches the input
                assert_eq!(value.as_str(), s);
                // Display round-trip
                assert_eq!(format!("{}", value), s);
            }
            Err(e) => {
                // Must be InvalidValue (CR/LF/NUL in the string)
                assert_eq!(e, HeaderError::InvalidValue);
                // The input must actually contain one of the forbidden bytes
                assert!(
                    s.bytes()
                        .any(|b| b == b'\r' || b == b'\n' || b == 0),
                    "error reported but no forbidden byte found in: {:?}",
                    s
                );
            }
        }
    }
});

#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::primitives::header_block::{HeaderName, HeaderError};

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let result = HeaderName::new(s);

        match result {
            Ok(name) => {
                // Round-trip: the stored name matches the input
                assert_eq!(name.as_str(), s);
                // Name is non-empty
                assert!(!name.as_str().is_empty());
                // Name length is within bounds
                assert!(name.as_str().len() <= 256);
                // Display round-trip
                assert_eq!(format!("{}", name), s);
            }
            Err(e) => {
                // Must be one of the expected error variants
                assert!(
                    matches!(e, HeaderError::InvalidName | HeaderError::NameTooLong),
                    "unexpected error variant: {:?}",
                    e
                );
            }
        }
    }
});

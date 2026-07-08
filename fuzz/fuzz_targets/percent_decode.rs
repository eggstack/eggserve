#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::path::decode;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        if let Ok(decoded) = decode::percent_decode(s) {
            assert!(!decoded.contains('\0'));
        }
    }
});

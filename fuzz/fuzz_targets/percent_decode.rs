#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::path::decode;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        if let Ok(decoded) = decode::percent_decode(s) {
            // No NUL bytes in decoded output
            assert!(!decoded.contains('\0'), "NUL byte in decoded output");
            // Output is valid UTF-8 (guaranteed by return type, but verify)
            assert!(std::str::from_utf8(decoded.as_bytes()).is_ok());
            // Decoded output must not be longer than 4x input (each %XX is 3 chars -> 1 char)
            // Worst case: all bytes are %XX sequences, so decoded <= input
            assert!(decoded.len() <= s.len() + 1,
                "decoded length {} exceeds input length {}", decoded.len(), s.len());
        }
    }
});

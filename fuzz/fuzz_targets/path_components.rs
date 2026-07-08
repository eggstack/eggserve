#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::path::{ConfinedPath, PathPolicy};

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let policy = PathPolicy::default();
        if let Ok(confined) = ConfinedPath::parse(s, &policy) {
            for comp in confined.components() {
                assert!(comp != ".." && comp != ".");
            }
        }
    }
});

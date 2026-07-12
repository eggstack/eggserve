#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::path::{ConfinedPath, PathPolicy};

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let policy = PathPolicy::default();
        if let Ok(confined) = ConfinedPath::parse(s, &policy) {
            // as_str is valid UTF-8
            assert!(std::str::from_utf8(confined.as_str().as_bytes()).is_ok());

            for comp in confined.components() {
                // No parent/current components
                assert!(comp != "..", "parent component accepted: {:?}", comp);
                assert!(comp != ".", "current component accepted: {:?}", comp);
                // No NUL bytes
                assert!(!comp.contains('\0'), "NUL in component: {:?}", comp);
                // No slashes in component
                assert!(!comp.contains('/'), "slash in component: {:?}", comp);
            }

            // Path starts with / if non-empty
            let s = confined.as_str();
            if !s.is_empty() {
                assert!(s.starts_with('/'), "path does not start with /: {:?}", s);
            }
        }
    }
});

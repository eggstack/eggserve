#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::path::{ConfinedPath, PathPolicy};

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let policy = PathPolicy::default();
        if let Ok(confined) = ConfinedPath::parse(s, &policy) {
            for comp in confined.components() {
                // No NUL bytes
                assert!(!comp.contains('\0'), "NUL in component: {:?}", comp);
                // No parent/current components
                assert_ne!(comp, "..", "parent component accepted");
                assert_ne!(comp, ".", "current component accepted");
                // No slashes in component
                assert!(!comp.contains('/'), "slash in component: {:?}", comp);
                // No backslashes (default policy rejects them)
                assert!(!comp.contains('\\'), "backslash in component: {:?}", comp);
            }

            // Path starts with /
            let s = confined.as_str();
            if !s.is_empty() {
                assert!(s.starts_with('/'));
            }
        }
    }
});

#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::primitives::client::ParsedUrl;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        if let Ok(url) = ParsedUrl::parse(s) {
            // Supported scheme
            assert!(url.scheme == eggserve_core::primitives::client::Scheme::Http
                || url.scheme == eggserve_core::primitives::client::Scheme::Https);

            // Non-empty host
            assert!(!url.host.is_empty());

            // Valid port
            assert!(url.port > 0);

            // Path starts with /
            assert!(url.path.starts_with '/' );

            // No fragments in path
            assert!(!url.path.contains('#'));

            // authority() round-trips
            let authority = url.authority();
            if url.host.contains(':') {
                assert!(authority.starts_with('['));
                assert!(authority.ends_with(']'));
            }
            if url.port == url.scheme.default_port() {
                assert!(!authority.contains(':'));
            } else {
                assert!(authority.contains(':'));
            }

            // is_https consistency
            assert_eq!(url.is_https(), url.scheme == eggserve_core::primitives::client::Scheme::Https);
        }
    }
});

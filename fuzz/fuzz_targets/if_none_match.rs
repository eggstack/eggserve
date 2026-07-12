#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::primitives::planner::evaluate_if_none_match;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let current_etag = "W/\"100-1234\"";

        let matched = evaluate_if_none_match(s, current_etag);

        if matched {
            // Wildcard always matches
            if s.trim() == "*" {
                return;
            }

            // If matched, there must be a token in the input that matches the inner value
            let inner = "100-1234";
            let has_match = s.split(',')
                .any(|etag| {
                    let etag = etag.trim();
                    let etag_inner = etag.strip_prefix("W/").unwrap_or(etag);
                    etag_inner == inner
                });
            assert!(has_match, "evaluate_if_none_match returned true but no matching token found");
        }
    }
});

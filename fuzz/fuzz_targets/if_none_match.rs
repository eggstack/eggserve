#![no_main]
use eggserve_core::primitives::planner::evaluate_if_none_match;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let current_etag = "W/\"100-1234\"";

        let matched = evaluate_if_none_match(s, current_etag);

        if matched {
            // Wildcard always matches
            if s.trim() == "*" {
                return;
            }

            // If matched, there must be a token in the input that matches the
            // current entity-tag. `If-None-Match` uses *weak* comparison: the
            // `W/` prefix is insignificant, but the DQUOTEs are part of the
            // entity-tag (RFC 9110 §8.8.3). The oracle therefore compares
            // against the current tag with only the `W/` prefix removed.
            let current_inner = current_etag.strip_prefix("W/").unwrap_or(current_etag);
            let has_match = s.split(',').any(|etag| {
                let etag = etag.trim();
                let etag_inner = etag.strip_prefix("W/").unwrap_or(etag);
                etag_inner == current_inner
            });
            assert!(
                has_match,
                "evaluate_if_none_match returned true but no matching token found"
            );
        }
    }
});

#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::primitives::response::RangeRequestOutcome;
use eggserve_core::primitives::planner::evaluate_range_header;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Use first byte as file size (0..255) for bounded testing
        let file_size = if data.is_empty() { 100 } else { data[0] as u64 + 1 };

        let outcome = evaluate_range_header(s, file_size);

        match outcome {
            RangeRequestOutcome::Satisfiable(range) => {
                // Range must be within file bounds
                assert!(range.start < file_size, "start {} beyond file_size {}", range.start, file_size);
                assert!(range.end_inclusive < file_size, "end {} beyond file_size {}", range.end_inclusive, file_size);
                assert!(range.start <= range.end_inclusive, "start {} > end {}", range.start, range.end_inclusive);
                // Content-Length must be positive
                assert!(range.len() > 0);
                // Content-Length must not exceed file_size
                assert!(range.len() <= file_size);
            }
            RangeRequestOutcome::NotSatisfiable => {
                // Valid outcome for unsatisfiable ranges
            }
            RangeRequestOutcome::MalformedOrUnsupported => {
                // Valid outcome for malformed ranges
            }
            RangeRequestOutcome::MultipleRanges => {
                // Valid outcome for multi-range
            }
        }
    }
});

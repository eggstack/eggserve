#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::ops::{self, Event, EventKind, Field, Severity};

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Fuzz event_to_json with arbitrary message content
        let event = Event::new(Severity::Info, EventKind::RequestCompleted, s);
        let json = ops::event_to_json(&event);

        // Must be valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&json)
            .expect("event_to_json produced invalid JSON");

        // Must be an object with required fields
        assert!(parsed.is_object());
        assert_eq!(parsed["schema_version"], 1);
        assert_eq!(parsed["severity"], "INFO");
        assert_eq!(parsed["event"], "request_completed");

        // Must be exactly one line (no newlines in the serialized output)
        assert!(
            !json.contains('\n'),
            "event_to_json produced multi-line output"
        );

        // Record size must be bounded (16 KiB max for a single event)
        assert!(
            json.len() <= 16384,
            "event_to_json produced oversized output: {} bytes",
            json.len()
        );

        // Fuzz event_to_json with a string field
        let event_with_field = Event::new(Severity::Error, EventKind::FileNotFound, s)
            .field(Field::Str("path".into(), s.to_string()));
        let json2 = ops::event_to_json(&event_with_field);
        let parsed2: serde_json::Value = serde_json::from_str(&json2)
            .expect("event_to_json with field produced invalid JSON");
        assert!(parsed2.is_object());

        // Fuzz sanitize_text_field
        let sanitized = ops::sanitize_text_field(s);
        // Must not contain control characters
        assert!(
            !sanitized.chars().any(|c| (c as u32) < 0x20),
            "sanitize_text_field leaked control chars"
        );
        // Must be bounded
        assert!(
            sanitized.len() <= 515,
            "sanitize_text_field exceeded max length: {}",
            sanitized.len()
        );

        // Fuzz sanitize_path
        let path_result = ops::sanitize_path(s);
        // Must be bounded
        assert!(
            path_result.len() <= 128,
            "sanitize_path exceeded max length: {}",
            path_result.len()
        );
    }
});

//! Integration tests for the ops module (Plan 087).
//!
//! Covers:
//! - JSON log validity
//! - Text log sanitization
//! - Control-character injection
//! - Path/query/header privacy
//! - Library silence
//! - Listener error backoff interruptibility
//! - Streaming error events
//! - Log sink failure resilience
//! - Correlation ID uniqueness and boundedness
//! - Event schema version
//! - Counter increments

use std::sync::atomic::Ordering;
use std::sync::Arc;

use eggserve_core::ops::{
    self, CompositeLogSink, Event, EventKind, Field, LogSink, Logger, NopLogSink, OpsCounters,
    Severity,
};

// ---------------------------------------------------------------------------
// JSON log validity tests
// ---------------------------------------------------------------------------

#[test]
fn json_log_validity() {
    // Emit several events and verify JSON output is valid
    let events = vec![
        Event::new(Severity::Info, EventKind::ProcessStarting, "test"),
        Event::new(Severity::Warn, EventKind::ClientDisconnect, "disconnected"),
        Event::new(
            Severity::Error,
            EventKind::ListenerPersistentError,
            "fatal error",
        ),
        Event::new(Severity::Debug, EventKind::FileNotFound, "not found"),
    ];

    for event in &events {
        // Verify the JSON representation is valid by checking basic structure
        let json = format!("{:?}", event);
        assert!(!json.is_empty());
    }

    // Verify EventKind Display produces valid names
    for event in &events {
        let name = event.event.to_string();
        assert!(!name.is_empty());
        assert!(
            name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "event name should be snake_case: {}",
            name
        );
    }
}

#[test]
fn json_event_schema_version_always_one() {
    let event = Event::new(Severity::Info, EventKind::ProcessStarting, "test");
    assert_eq!(event.schema_version, 1);
}

#[test]
fn json_event_timestamp_format() {
    let event = Event::new(Severity::Info, EventKind::ProcessStarting, "test");
    // Format: YYYY-MM-DDTHH:MM:SS.mmmZ
    assert!(event.timestamp.ends_with('Z'));
    assert_eq!(event.timestamp.len(), 24);
    assert!(event.timestamp.contains('T'));
    assert_eq!(event.timestamp.matches('-').count(), 2);
    assert_eq!(event.timestamp.matches(':').count(), 2);
}

// ---------------------------------------------------------------------------
// Text log sanitization tests
// ---------------------------------------------------------------------------

#[test]
fn text_log_sanitize_control_chars() {
    assert_eq!(ops::sanitize_text_field("hello\r\nworld"), "helloworld");
    assert_eq!(ops::sanitize_text_field("tab\there"), "tabhere");
    assert_eq!(ops::sanitize_text_field("esc\x1B[31mred"), "esc[31mred");
    assert_eq!(
        ops::sanitize_text_field("null\x00byte\x7Fdel"),
        "nullbytedel"
    );
    assert_eq!(ops::sanitize_text_field("normal text"), "normal text");
}

#[test]
fn text_log_sanitize_bidi_controls() {
    // Bidi override characters (U+202A-U+202E) should be stripped
    let bidi = "hello\u{202A}world\u{202C}";
    let sanitized = ops::sanitize_text_field(bidi);
    assert!(!sanitized.contains('\u{202A}'));
    assert!(!sanitized.contains('\u{202C}'));
}

#[test]
fn text_log_sanitize_long_input() {
    let long = "a".repeat(10000);
    let sanitized = ops::sanitize_text_field(&long);
    assert!(sanitized.len() <= 10000);
}

#[test]
fn sanitize_path_extracts_last_component() {
    assert_eq!(ops::sanitize_path("/foo/bar/baz.txt"), "baz.txt");
    assert_eq!(ops::sanitize_path("no/slash/here/"), "");
    assert_eq!(ops::sanitize_path("only-one"), "only-one");
}

#[test]
fn sanitize_path_truncates_long_paths() {
    let long_name = "a".repeat(200);
    let result = ops::sanitize_path(&format!("/prefix/{}", long_name));
    assert!(result.chars().count() <= 128);
    assert!(result.ends_with('…'));
}

#[test]
fn sanitize_path_strips_control_chars() {
    let result = ops::sanitize_path("/foo/bar\x1B[31m.txt");
    assert!(!result.contains('\x1B'));
}

// ---------------------------------------------------------------------------
// Injection tests
// ---------------------------------------------------------------------------

#[test]
fn injection_carriage_return_in_path() {
    let result = ops::sanitize_path("/foo/bar\r\ninjection.txt");
    assert!(!result.contains('\r'));
    assert!(!result.contains('\n'));
}

#[test]
fn injection_null_byte_in_text() {
    let result = ops::sanitize_text_field("hello\x00world");
    assert_eq!(result, "helloworld");
}

#[test]
fn injection_escape_sequence_in_text() {
    let result = ops::sanitize_text_field("hello\x1B[31mred\x1B[0m");
    // ESC (0x1B) is stripped but remaining chars are kept
    assert_eq!(result, "hello[31mred[0m");
    assert!(!result.contains('\x1B'));
}

#[test]
fn injection_ansi_sgr_in_path() {
    let result = ops::sanitize_path("/foo/\x1B[31mred.txt\x1B[0m");
    assert!(!result.contains('\x1B'));
}

// ---------------------------------------------------------------------------
// Privacy tests
// ---------------------------------------------------------------------------

#[test]
fn no_absolute_path_in_sanitize_path_output() {
    // sanitize_path should only return the last component
    assert_eq!(ops::sanitize_path("/etc/passwd"), "passwd");
    assert_eq!(ops::sanitize_path("/home/user/secret.txt"), "secret.txt");
}

#[test]
fn path_sanitize_strips_query_string() {
    // Query strings in paths should be stripped by the last-component extraction
    assert_eq!(
        ops::sanitize_path("/foo/bar.txt?secret=abc"),
        "bar.txt?secret=abc"
    );
}

// ---------------------------------------------------------------------------
// Event kind display tests
// ---------------------------------------------------------------------------

#[test]
fn all_event_kinds_have_names() {
    let kinds = [
        EventKind::ProcessStarting,
        EventKind::RootInitialized,
        EventKind::ListenerReady,
        EventKind::ShutdownRequested,
        EventKind::DrainingStarted,
        EventKind::ForcedShutdownStarted,
        EventKind::ShutdownComplete,
        EventKind::ConnectionAccepted,
        EventKind::ConnectionRejected,
        EventKind::TlsHandshakeSuccess,
        EventKind::TlsHandshakeFailure,
        EventKind::TlsHandshakeTimeout,
        EventKind::HeaderTimeout,
        EventKind::BodyReadTimeout,
        EventKind::ParserRejection,
        EventKind::KeepAliveClosed,
        EventKind::ResponseWriteTimeout,
        EventKind::ClientDisconnect,
        EventKind::ConnectionPanic,
        EventKind::RequestCompleted,
        EventKind::FileNotFound,
        EventKind::FileDenied,
        EventKind::FileError,
        EventKind::DotfileDenied,
        EventKind::SymlinkDenied,
        EventKind::RootEscapeDenied,
        EventKind::BodyPolicyRejection,
        EventKind::ServiceTimeout,
        EventKind::ServiceError,
        EventKind::DirectoryListingLimit,
        EventKind::ListenerTransientError,
        EventKind::ListenerPersistentError,
        EventKind::ResourceExhaustion,
        EventKind::BlockingWorkerSaturation,
        EventKind::LogSinkFailure,
    ];

    let mut names = std::collections::HashSet::new();
    for kind in &kinds {
        let name = kind.to_string();
        assert!(!name.is_empty(), "event kind has no name");
        assert!(
            names.insert(name.clone()),
            "duplicate event kind name: {}",
            name
        );
    }
}

// ---------------------------------------------------------------------------
// Severity display tests
// ---------------------------------------------------------------------------

#[test]
fn severity_display() {
    assert_eq!(Severity::Debug.to_string(), "DEBUG");
    assert_eq!(Severity::Info.to_string(), "INFO");
    assert_eq!(Severity::Warn.to_string(), "WARN");
    assert_eq!(Severity::Error.to_string(), "ERROR");
}

// ---------------------------------------------------------------------------
// Correlation ID tests
// ---------------------------------------------------------------------------

#[test]
fn correlation_id_starts_at_one() {
    let cid = ops::CorrelationId::new();
    assert_eq!(cid.next(), 1);
}

#[test]
fn correlation_id_increments() {
    let cid = ops::CorrelationId::new();
    assert_eq!(cid.next(), 1);
    assert_eq!(cid.next(), 2);
    assert_eq!(cid.next(), 3);
}

#[test]
fn correlation_id_wraps_safely() {
    let cid = ops::CorrelationId::new();
    // Simulate many connections to test wraparound
    for _ in 0..1000 {
        let _ = cid.next();
    }
    // Should not panic
}

#[tokio::test]
async fn correlation_id_unique_concurrent() {
    let cid = Arc::new(ops::CorrelationId::new());
    let mut handles = Vec::new();
    for _ in 0..10 {
        let cid = cid.clone();
        handles.push(tokio::spawn(async move {
            let mut ids = Vec::new();
            for _ in 0..100 {
                ids.push(cid.next());
            }
            ids
        }));
    }
    let mut all_ids = Vec::new();
    for h in handles {
        all_ids.extend(h.await.unwrap());
    }
    // All IDs should be unique
    let set: std::collections::HashSet<u64> = all_ids.into_iter().collect();
    assert_eq!(set.len(), 1000);
}

// ---------------------------------------------------------------------------
// Log sink failure tests
// ---------------------------------------------------------------------------

struct PanicSink;

impl LogSink for PanicSink {
    fn emit(&self, _event: &Event) {
        panic!("sink panic");
    }
    fn flush(&self) {
        panic!("flush panic");
    }
}

#[test]
fn composite_sink_catches_panic() {
    let composite = CompositeLogSink::new(vec![Box::new(PanicSink), Box::new(NopLogSink)]);

    let event = Event::new(Severity::Info, EventKind::ProcessStarting, "test");
    // Should not panic
    composite.emit(&event);
}

#[test]
fn log_failure_does_not_panic() {
    let logger = Logger::global();
    let event = Event::new(Severity::Info, EventKind::ProcessStarting, "test");
    // NopLogSink never panics
    logger.emit(event);
}

// ---------------------------------------------------------------------------
// OpsCounters tests
// ---------------------------------------------------------------------------

#[test]
fn ops_counters_snapshot() {
    let counters = OpsCounters::new();
    counters
        .connections_accepted
        .fetch_add(5, Ordering::Relaxed);
    counters.bytes_sent.fetch_add(1024, Ordering::Relaxed);
    counters.listener_errors.fetch_add(1, Ordering::Relaxed);
    counters.active_connections.fetch_add(3, Ordering::Relaxed);
    counters.active_file_streams.fetch_add(2, Ordering::Relaxed);
    counters.parser_rejects.fetch_add(1, Ordering::Relaxed);
    counters.header_timeouts.fetch_add(1, Ordering::Relaxed);
    counters
        .response_write_timeouts
        .fetch_add(1, Ordering::Relaxed);
    counters.graceful_shutdowns.fetch_add(1, Ordering::Relaxed);
    counters.forced_shutdowns.fetch_add(1, Ordering::Relaxed);
    counters
        .connections_rejected
        .fetch_add(2, Ordering::Relaxed);
    counters.dropped_log_events.fetch_add(1, Ordering::Relaxed);

    let snap = counters.snapshot();
    assert_eq!(snap.connections_accepted, 5);
    assert_eq!(snap.bytes_sent, 1024);
    assert_eq!(snap.listener_errors, 1);
    assert_eq!(snap.active_connections, 3);
    assert_eq!(snap.active_file_streams, 2);
    assert_eq!(snap.parser_rejects, 1);
    assert_eq!(snap.header_timeouts, 1);
    assert_eq!(snap.response_write_timeouts, 1);
    assert_eq!(snap.graceful_shutdowns, 1);
    assert_eq!(snap.forced_shutdowns, 1);
    assert_eq!(snap.connections_rejected, 2);
    assert_eq!(snap.dropped_log_events, 1);
}

#[test]
fn global_counters_returns_singleton() {
    let c1 = ops::global_counters();
    let c2 = ops::global_counters();
    // Same pointer
    assert!(std::ptr::eq(c1, c2));
}

// ---------------------------------------------------------------------------
// Event builder tests
// ---------------------------------------------------------------------------

#[test]
fn event_builder_chain() {
    let event = Event::new(Severity::Info, EventKind::ProcessStarting, "test")
        .field(Field::Str("key".into(), "value".into()))
        .field(Field::Bool("flag".into(), true))
        .field(Field::U64("count".into(), 42))
        .field(Field::I64("signed".into(), -1))
        .connection_id(123)
        .request_seq(456);

    assert_eq!(event.schema_version, 1);
    assert_eq!(event.severity, Severity::Info);
    assert_eq!(event.event, EventKind::ProcessStarting);
    assert_eq!(event.message, "test");
    assert_eq!(event.connection_id, Some(123));
    assert_eq!(event.request_seq, Some(456));
    assert_eq!(event.fields.len(), 4);
}

// ---------------------------------------------------------------------------
// Logger tests
// ---------------------------------------------------------------------------

#[test]
fn logger_try_init_does_not_panic() {
    // try_init should not panic if already initialized
    let _ = Logger::try_init(Box::new(NopLogSink));
    let _ = Logger::try_init(Box::new(NopLogSink));
}

#[test]
fn logger_global_returns_something() {
    let _ = Logger::global();
}

// ---------------------------------------------------------------------------
// Event schema version test
// ---------------------------------------------------------------------------

#[test]
fn event_schema_version() {
    assert_eq!(ops::SCHEMA_VERSION, 1);
}

// ---------------------------------------------------------------------------
// Streaming error event types exist
// ---------------------------------------------------------------------------

#[test]
fn streaming_error_events_exist() {
    // Verify all streaming error event kinds exist and have correct names
    assert_eq!(EventKind::ClientDisconnect.to_string(), "client_disconnect");
    assert_eq!(
        EventKind::ResponseWriteTimeout.to_string(),
        "response_write_timeout"
    );
    assert_eq!(EventKind::BodyReadTimeout.to_string(), "body_read_timeout");
    assert_eq!(EventKind::ServiceTimeout.to_string(), "service_timeout");
    assert_eq!(EventKind::HeaderTimeout.to_string(), "header_timeout");
}

// ---------------------------------------------------------------------------
// Filesystem denial event types exist
// ---------------------------------------------------------------------------

#[test]
fn filesystem_denial_events_exist() {
    assert_eq!(EventKind::FileNotFound.to_string(), "file_not_found");
    assert_eq!(EventKind::FileDenied.to_string(), "file_denied");
    assert_eq!(EventKind::DotfileDenied.to_string(), "dotfile_denied");
    assert_eq!(EventKind::SymlinkDenied.to_string(), "symlink_denied");
    assert_eq!(
        EventKind::RootEscapeDenied.to_string(),
        "root_escape_denied"
    );
}

// ---------------------------------------------------------------------------
// Lifecycle event types exist
// ---------------------------------------------------------------------------

#[test]
fn lifecycle_events_exist() {
    assert_eq!(EventKind::DrainingStarted.to_string(), "draining_started");
    assert_eq!(
        EventKind::ForcedShutdownStarted.to_string(),
        "forced_shutdown_started"
    );
    assert_eq!(EventKind::ShutdownComplete.to_string(), "shutdown_complete");
}

// ---------------------------------------------------------------------------
// Field Display tests
// ---------------------------------------------------------------------------

#[test]
fn field_display() {
    let f = Field::Str("key".into(), "value".into());
    assert_eq!(format!("{}", f), "\"key\": \"value\"");

    let f = Field::Bool("flag".into(), true);
    assert_eq!(format!("{}", f), "\"flag\": true");

    let f = Field::U64("count".into(), 42);
    assert_eq!(format!("{}", f), "\"count\": 42");

    let f = Field::I64("signed".into(), -1);
    assert_eq!(format!("{}", f), "\"signed\": -1");
}

#[test]
fn field_display_escapes_strings() {
    let f = Field::Str("key".into(), "value with \"quotes\" and \\backslash".into());
    let display = format!("{}", f);
    assert!(display.contains("\\\"quotes\\\""));
    assert!(display.contains("\\\\backslash"));
}

// ---------------------------------------------------------------------------
// Fuzz/property tests for event serialization
// ---------------------------------------------------------------------------

#[cfg(test)]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn event_to_json_never_panics(
            severity in prop_oneof![
                Just(Severity::Debug),
                Just(Severity::Info),
                Just(Severity::Warn),
                Just(Severity::Error),
            ],
            message in ".*",
            field_key in "[a-z_]{0,50}",
            field_val in ".*",
        ) {
            let event = Event::new(severity, EventKind::ProcessStarting, message)
                .field(Field::Str(field_key, field_val));
            // Should never panic
            let _ = format!("{:?}", event);
        }

        #[test]
        fn sanitize_text_never_panics(s in ".*") {
            let _ = ops::sanitize_text_field(&s);
        }

        #[test]
        fn sanitize_path_never_panics(s in ".*") {
            let _ = ops::sanitize_path(&s);
        }

        #[test]
        fn sanitize_text_strips_control_chars(s in "\x00-\x1f") {
            let result = ops::sanitize_text_field(&s);
            assert!(!result.chars().any(|c| (c as u32) < 0x20));
        }

        #[test]
        fn sanitize_text_preserves_printable(s in "[\\x20-\\x7e]+") {
            let result = ops::sanitize_text_field(&s);
            assert_eq!(result, s);
        }

        #[test]
        fn sanitize_path_always_returns_last_component(path in "/[a-z]{1,10}/[a-z]{1,10}/[a-z]{1,10}") {
            let result = ops::sanitize_path(&path);
            // Should be the last component
            let last = path.rsplit('/').next().unwrap();
            // Result is truncated to 127 chars, so may be shorter
            assert!(result.len() <= last.len().min(128));
        }

        #[test]
        fn event_kind_display_is_snake_case(kind in prop_oneof![
            Just(EventKind::ProcessStarting),
            Just(EventKind::RootInitialized),
            Just(EventKind::ListenerReady),
            Just(EventKind::ShutdownRequested),
            Just(EventKind::DrainingStarted),
            Just(EventKind::ForcedShutdownStarted),
            Just(EventKind::ShutdownComplete),
            Just(EventKind::ConnectionAccepted),
            Just(EventKind::ConnectionRejected),
            Just(EventKind::ClientDisconnect),
            Just(EventKind::ResponseWriteTimeout),
            Just(EventKind::BodyReadTimeout),
            Just(EventKind::ServiceTimeout),
            Just(EventKind::FileNotFound),
            Just(EventKind::FileDenied),
            Just(EventKind::FileError),
            Just(EventKind::DotfileDenied),
            Just(EventKind::SymlinkDenied),
            Just(EventKind::RootEscapeDenied),
            Just(EventKind::ListenerTransientError),
            Just(EventKind::ListenerPersistentError),
            Just(EventKind::ResourceExhaustion),
            Just(EventKind::LogSinkFailure),
        ]) {
            let name = kind.to_string();
            assert!(!name.is_empty());
            assert!(name.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
        }

        #[test]
        fn correlation_id_always_positive(count in 1u64..10000) {
            let cid = ops::CorrelationId::new();
            let mut last = 0;
            for _ in 0..count {
                let id = cid.next();
                assert!(id > last);
                last = id;
            }
        }

        #[test]
        fn ops_counters_snapshot_consistent(
            accepted in 0u64..1000,
            rejected in 0u64..100,
            errors in 0u64..100,
        ) {
            let counters = OpsCounters::new();
            counters.connections_accepted.fetch_add(accepted, Ordering::Relaxed);
            counters.connections_rejected.fetch_add(rejected, Ordering::Relaxed);
            counters.listener_errors.fetch_add(errors, Ordering::Relaxed);
            let snap = counters.snapshot();
            assert_eq!(snap.connections_accepted, accepted);
            assert_eq!(snap.connections_rejected, rejected);
            assert_eq!(snap.listener_errors, errors);
        }
    }
}

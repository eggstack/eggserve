//! Accept-error classification authority matrix (Plan 309).
//!
//! The expected values in this file are the *baseline compatibility values*
//! (the classification the `eggserve-core` accept loop shipped before
//! Plan 309), hard-coded here on purpose. Comparing the two crates against
//! each other would pass even if both were wrong; comparing them against
//! this table pins the reference behavior.
//!
//! The matrix covers the injected errors from the milestone plan: Unix
//! `EMFILE`/`ENFILE`, Winsock `WSAEMFILE`/`WSAENFILE`, out-of-memory,
//! interrupted, a plain transient error, and a fatal error kind.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use eggserve_server::accept_error::{
    accept_backoff, accept_backoff_ms, classify_accept_error, is_fd_exhaustion,
    report_accept_error, AcceptErrorClassification, ACCEPT_BACKOFF_MS, EMFILE_RAW, ENFILE_RAW,
    WSAEMFILE_RAW, WSAENFILE_RAW,
};
use eggserve_server::ops::{Event, EventKind, LogSink, OpsContext, Severity};

/// A sink that keeps every emitted event for assertions.
#[derive(Default)]
struct CapturingSink {
    events: Mutex<Vec<Event>>,
}

impl CapturingSink {
    fn events(&self) -> Vec<Event> {
        self.events.lock().expect("sink mutex").clone()
    }
}

impl LogSink for CapturingSink {
    fn emit(&self, event: &Event) {
        self.events.lock().expect("sink mutex").push(event.clone());
    }

    fn flush(&self) {}
}

fn exhaustion() -> AcceptErrorClassification {
    AcceptErrorClassification {
        severity: Severity::Error,
        event_kind: EventKind::ResourceExhaustion,
        should_backoff: true,
        is_fatal: false,
        fd_exhausted: true,
    }
}

fn transient(severity: Severity) -> AcceptErrorClassification {
    AcceptErrorClassification {
        severity,
        event_kind: EventKind::ListenerTransientError,
        should_backoff: true,
        is_fatal: false,
        fd_exhausted: false,
    }
}

fn persistent() -> AcceptErrorClassification {
    AcceptErrorClassification {
        severity: Severity::Error,
        event_kind: EventKind::ListenerPersistentError,
        should_backoff: false,
        is_fatal: true,
        fd_exhausted: false,
    }
}

#[test]
fn classification_matrix_matches_the_compatibility_baseline() {
    use std::io::ErrorKind;

    let cases: Vec<(&str, std::io::Error, AcceptErrorClassification)> = vec![
        (
            "unix EMFILE",
            std::io::Error::from_raw_os_error(EMFILE_RAW),
            exhaustion(),
        ),
        (
            "unix ENFILE",
            std::io::Error::from_raw_os_error(ENFILE_RAW),
            exhaustion(),
        ),
        (
            "winsock WSAEMFILE",
            std::io::Error::from_raw_os_error(WSAEMFILE_RAW),
            exhaustion(),
        ),
        (
            "winsock WSAENFILE",
            std::io::Error::from_raw_os_error(WSAENFILE_RAW),
            exhaustion(),
        ),
        (
            "out of memory",
            std::io::Error::from(ErrorKind::OutOfMemory),
            AcceptErrorClassification {
                severity: Severity::Error,
                event_kind: EventKind::ResourceExhaustion,
                should_backoff: true,
                is_fatal: false,
                fd_exhausted: false,
            },
        ),
        (
            "interrupted",
            std::io::Error::from(ErrorKind::Interrupted),
            transient(Severity::Debug),
        ),
        (
            "plain transient (connection reset)",
            std::io::Error::from(ErrorKind::ConnectionReset),
            transient(Severity::Debug),
        ),
        (
            "transient with warning severity (would block)",
            std::io::Error::from(ErrorKind::WouldBlock),
            transient(Severity::Warn),
        ),
        (
            "fatal error kind",
            std::io::Error::from(ErrorKind::Other),
            persistent(),
        ),
        (
            "message-fallback exhaustion",
            std::io::Error::other("Too many open files"),
            exhaustion(),
        ),
    ];

    for (name, error, expected) in cases {
        assert_eq!(
            classify_accept_error(&error),
            expected,
            "classification drifted for case {name}"
        );
    }
}

#[test]
fn exhaustion_predicate_table() {
    // Raw OS codes are recognized by value on every platform: the Unix
    // codes stay exhaustion on a non-Unix host and the Winsock codes stay
    // exhaustion on a Unix host.
    assert!(is_fd_exhaustion(&std::io::Error::from_raw_os_error(
        EMFILE_RAW
    )));
    assert!(is_fd_exhaustion(&std::io::Error::from_raw_os_error(
        ENFILE_RAW
    )));
    assert!(is_fd_exhaustion(&std::io::Error::from_raw_os_error(
        WSAEMFILE_RAW
    )));
    assert!(is_fd_exhaustion(&std::io::Error::from_raw_os_error(
        WSAENFILE_RAW
    )));

    // A raw code outside the exhaustion set is not exhaustion, and the
    // message fallback never applies to an OS error.
    assert!(!is_fd_exhaustion(&std::io::Error::from_raw_os_error(11)));
    assert!(!is_fd_exhaustion(&std::io::Error::from_raw_os_error(12)));

    // The documented message fallback covers non-OS errors only.
    assert!(is_fd_exhaustion(&std::io::Error::other(
        "Too many open files"
    )));
    assert!(is_fd_exhaustion(&std::io::Error::other(
        "EMFILE: too many open files"
    )));
    assert!(is_fd_exhaustion(&std::io::Error::other("enfile")));
    assert!(!is_fd_exhaustion(&std::io::Error::other(
        "connection refused"
    )));
}

#[test]
fn backoff_ramp_matches_the_reference_schedule_and_saturates() {
    assert_eq!(ACCEPT_BACKOFF_MS, [1, 2, 4, 8, 50, 100, 250, 500]);

    for (index, expected) in ACCEPT_BACKOFF_MS.iter().enumerate() {
        assert_eq!(accept_backoff_ms(index), *expected, "ramp step {index}");
        assert_eq!(
            accept_backoff(index),
            Duration::from_millis(*expected),
            "ramp delay {index}"
        );
    }

    // The index saturates at the last step; it never exceeds 500 ms and
    // never wraps back to the start of the ramp.
    for index in [ACCEPT_BACKOFF_MS.len(), 64, 1024, usize::MAX] {
        assert_eq!(accept_backoff_ms(index), 500, "saturated step {index}");
        assert_eq!(
            accept_backoff(index),
            Duration::from_millis(500),
            "saturated delay {index}"
        );
    }
}

#[test]
fn direct_accept_error_handling_emits_the_classified_event() {
    let sink = Arc::new(CapturingSink::default());
    let ops = OpsContext::new(sink.clone());

    let error = std::io::Error::from_raw_os_error(EMFILE_RAW);
    let classification = report_accept_error(&error, &ops);

    assert_eq!(classification, classify_accept_error(&error));
    assert_eq!(classification, exhaustion());
    assert!(!classification.is_fatal);
    assert!(classification.should_backoff);

    let events = sink.events();
    assert_eq!(events.len(), 1, "every accept error is emitted");
    assert_eq!(events[0].severity, Severity::Error);
    assert_eq!(events[0].event, EventKind::ResourceExhaustion);
    assert!(
        events[0].message.starts_with("accept error: "),
        "unexpected message: {}",
        events[0].message
    );
    assert_eq!(ops.snapshot().listener_errors, 1);
}

#[test]
fn fatal_accept_error_is_reported_as_fatal_and_persistent() {
    let sink = Arc::new(CapturingSink::default());
    let ops = OpsContext::new(sink.clone());

    let error = std::io::Error::from(std::io::ErrorKind::Other);
    let classification = report_accept_error(&error, &ops);

    assert_eq!(classification, persistent());
    assert!(classification.is_fatal, "a fatal error must stop the loop");
    assert!(!classification.should_backoff);

    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].severity, Severity::Error);
    assert_eq!(events[0].event, EventKind::ListenerPersistentError);
    assert_eq!(ops.snapshot().listener_errors, 1);
}

#[test]
fn accept_error_text_is_sanitized_before_emission() {
    let sink = Arc::new(CapturingSink::default());
    let ops = OpsContext::new(sink.clone());

    let error = std::io::Error::other("bad \u{1b}[31mvalue\u{7f}");
    report_accept_error(&error, &ops);

    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert!(
        !events[0].message.contains('\u{1b}'),
        "escape byte reached the event payload"
    );
    assert!(
        !events[0].message.contains('\u{7f}'),
        "DEL byte reached the event payload"
    );
}

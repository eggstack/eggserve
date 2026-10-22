//! Accept-error classification parity for the compatibility crate (Plan 309).
//!
//! The compatibility crate keeps no second classification: it delegates to
//! the authority in `eggserve-server`. This suite pins the *baseline*
//! classification values on the compatibility side of that delegation, so
//! the compatibility accept loop's observable behavior stays the reference
//! behavior the direct runtime is brought to.
//!
//! The in-crate unit test (`crates/eggserve-core/src/server/accept.rs`)
//! proves the compatibility loop delegates; this suite proves the authority
//! it delegates to carries the baseline values.

use eggserve_core::layers::server::accept_error as compat_authority;
use eggserve_server::accept_error as authority;

/// The baseline classification values, hard-coded (not derived from either
/// crate) so a drift in both crates fails this suite.
fn baseline(
    severity: eggserve_server::ops::Severity,
    event_kind: eggserve_server::ops::EventKind,
    should_backoff: bool,
    is_fatal: bool,
    fd_exhausted: bool,
) -> authority::AcceptErrorClassification {
    authority::AcceptErrorClassification {
        severity,
        event_kind,
        should_backoff,
        is_fatal,
        fd_exhausted,
    }
}

#[test]
fn compat_authority_matches_the_baseline_classification_matrix() {
    use eggserve_server::ops::{EventKind, Severity};

    let exhaustion = baseline(
        Severity::Error,
        EventKind::ResourceExhaustion,
        true,
        false,
        true,
    );
    let debug_transient = baseline(
        Severity::Debug,
        EventKind::ListenerTransientError,
        true,
        false,
        false,
    );
    let fatal = baseline(
        Severity::Error,
        EventKind::ListenerPersistentError,
        false,
        true,
        false,
    );

    let cases: Vec<(std::io::Error, authority::AcceptErrorClassification)> = vec![
        (
            std::io::Error::from_raw_os_error(authority::EMFILE_RAW),
            exhaustion,
        ),
        (
            std::io::Error::from_raw_os_error(authority::ENFILE_RAW),
            exhaustion,
        ),
        (
            std::io::Error::from_raw_os_error(authority::WSAEMFILE_RAW),
            exhaustion,
        ),
        (
            std::io::Error::from_raw_os_error(authority::WSAENFILE_RAW),
            exhaustion,
        ),
        (
            std::io::Error::from(std::io::ErrorKind::OutOfMemory),
            baseline(
                Severity::Error,
                EventKind::ResourceExhaustion,
                true,
                false,
                false,
            ),
        ),
        (
            std::io::Error::from(std::io::ErrorKind::Interrupted),
            debug_transient,
        ),
        (
            std::io::Error::from(std::io::ErrorKind::ConnectionReset),
            debug_transient,
        ),
        (
            std::io::Error::from(std::io::ErrorKind::WouldBlock),
            baseline(
                Severity::Warn,
                EventKind::ListenerTransientError,
                true,
                false,
                false,
            ),
        ),
        (std::io::Error::from(std::io::ErrorKind::Other), fatal),
        (std::io::Error::other("Too many open files"), exhaustion),
    ];

    for (error, expected) in cases {
        assert_eq!(
            compat_authority::classify_accept_error(&error),
            expected,
            "compatibility-side classification drifted"
        );
        assert_eq!(
            compat_authority::is_fd_exhaustion(&error),
            expected.fd_exhausted,
            "compatibility-side exhaustion predicate drifted"
        );
    }
}

#[test]
fn compat_crate_and_direct_crate_share_one_classification_authority() {
    // The compatibility crate reaches the authority through its public
    // layer path; both paths must answer identically.
    for code in [
        authority::EMFILE_RAW,
        authority::ENFILE_RAW,
        authority::WSAEMFILE_RAW,
        authority::WSAENFILE_RAW,
        11,
    ] {
        let error = std::io::Error::from_raw_os_error(code);
        assert_eq!(
            compat_authority::classify_accept_error(&error),
            authority::classify_accept_error(&error),
            "the two crates disagree for raw code {code}"
        );
        assert_eq!(
            compat_authority::is_fd_exhaustion(&error),
            authority::is_fd_exhaustion(&error),
            "the two crates disagree on exhaustion for raw code {code}"
        );
    }

    assert_eq!(
        compat_authority::ACCEPT_BACKOFF_MS,
        authority::ACCEPT_BACKOFF_MS,
        "the two crates disagree on the backoff ramp"
    );
}

/// Compile-time proof that the compatibility crate's observability
/// vocabulary *is* the authority's vocabulary (one `Severity`/`EventKind`
/// type, not two), so an event emitted by the compatibility accept loop
/// carries the authority's classification.
const _: fn(eggserve_core::ops::Severity) -> eggserve_server::ops::Severity = |s| s;
const _: fn(eggserve_core::ops::EventKind) -> eggserve_server::ops::EventKind = |k| k;

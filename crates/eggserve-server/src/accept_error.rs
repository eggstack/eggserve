//! Accept-error classification authority (Plan 309).
//!
//! One classification serves both accept loops: the direct
//! `eggserve-server` runtime and the compatibility
//! `eggserve-core::server::accept` loop. The reference behavior is the
//! compatibility loop's classification (severity, event kind, fatal
//! decision, and the shutdown-aware backoff ramp); the direct loop is
//! brought *to* it, never the other way around.
//!
//! The module is deliberately dependency-free. The descriptor-exhaustion
//! predicate is expressed with [`std::io::Error::raw_os_error`] plus the
//! platforms' raw OS error numbers, so `eggserve-server` keeps its current
//! direct dependency graph (no `rustix`, no `windows-sys`).
//!
//! Classification is a pure function of the error. Accounting and emission
//! stay with the caller's runtime context
//! ([`crate::accept_error::report_accept_error`]), so the
//! `OpsContext` event taxonomy and the `listener_errors` counter keep their
//! meaning in both loops.

use std::time::Duration;

use crate::ops::{Event, EventKind, OpsContext, Severity};

/// Unix `EMFILE` ("too many open files") raw OS error number.
///
/// POSIX assigns `EMFILE` 24 and `ENFILE` 23 on every supported Unix
/// platform (Linux, macOS, the BSDs, illumos). These are the numbers
/// `rustix::io::Errno::MFILE.raw_os_error()` / `NFILE.raw_os_error()`
/// resolve to; they are spelled out here so `eggserve-server` needs no
/// `rustix` dependency.
pub const EMFILE_RAW: i32 = 24;

/// Unix `ENFILE` ("too many open files in system") raw OS error number.
pub const ENFILE_RAW: i32 = 23;

/// Winsock `WSAEMFILE` ("too many open files") raw OS error number.
pub const WSAEMFILE_RAW: i32 = 10024;

/// Winsock `WSAENFILE` ("too many open files in system") raw OS error number.
pub const WSAENFILE_RAW: i32 = 10023;

/// The accept-loop backoff ramp in milliseconds.
///
/// The ramp is the compatibility runtime's reference schedule: it starts at
/// 1 ms and saturates at 500 ms, so a persistent accept failure backs off
/// instead of hot-spinning the accept loop.
pub const ACCEPT_BACKOFF_MS: [u64; 8] = [1, 2, 4, 8, 50, 100, 250, 500];

/// The classification of one accept-loop error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcceptErrorClassification {
    /// Severity for the emitted event.
    pub severity: Severity,
    /// Event kind for the emitted event.
    pub event_kind: EventKind,
    /// Whether the accept loop must back off before retrying.
    pub should_backoff: bool,
    /// Whether the error is fatal for the accept loop.
    ///
    /// A fatal error ends the accept loop through the runtime's normal
    /// shutdown path instead of being retried forever.
    pub is_fatal: bool,
    /// Whether the error was recognized as descriptor exhaustion.
    pub fd_exhausted: bool,
}

/// Classify one accept-loop error.
///
/// The mapping is the compatibility runtime's reference mapping, arm for
/// arm:
///
/// | `io::ErrorKind` | Severity | Event kind | Backoff | Fatal |
/// |---|---|---|---|---|
/// | `Interrupted` | `Debug` | `ListenerTransientError` | yes | no |
/// | `ConnectionRefused`, `ConnectionReset`, `ConnectionAborted`, `BrokenPipe` | `Debug` | `ListenerTransientError` | yes | no |
/// | `WouldBlock`, `TimedOut` | `Warn` | `ListenerTransientError` | yes | no |
/// | `OutOfMemory` | `Error` | `ResourceExhaustion` | yes | no |
/// | `Other` with descriptor exhaustion | `Error` | `ResourceExhaustion` | yes | no |
/// | any other kind with descriptor exhaustion | `Error` | `ResourceExhaustion` | yes | no |
/// | any other kind | `Error` | `ListenerPersistentError` | no | yes |
#[allow(clippy::collapsible_match)]
pub fn classify_accept_error(error: &std::io::Error) -> AcceptErrorClassification {
    let kind = error.kind();
    let fd_exhausted = is_fd_exhaustion(error);

    let (severity, event_kind, should_backoff, is_fatal) = match kind {
        std::io::ErrorKind::Interrupted => (
            Severity::Debug,
            EventKind::ListenerTransientError,
            true,
            false,
        ),
        std::io::ErrorKind::ConnectionRefused
        | std::io::ErrorKind::ConnectionReset
        | std::io::ErrorKind::ConnectionAborted
        | std::io::ErrorKind::BrokenPipe => (
            Severity::Debug,
            EventKind::ListenerTransientError,
            true,
            false,
        ),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => (
            Severity::Warn,
            EventKind::ListenerTransientError,
            true,
            false,
        ),
        // Kernel memory pressure (ENOMEM) surfaces as OutOfMemory without
        // matching is_fd_exhaustion; like fd exhaustion it can be transient,
        // so back off instead of terminating the server.
        std::io::ErrorKind::OutOfMemory => {
            (Severity::Error, EventKind::ResourceExhaustion, true, false)
        }
        std::io::ErrorKind::Other if fd_exhausted => {
            (Severity::Error, EventKind::ResourceExhaustion, true, false)
        }
        std::io::ErrorKind::Other => (
            Severity::Error,
            EventKind::ListenerPersistentError,
            false,
            true,
        ),
        _ if fd_exhausted => (Severity::Error, EventKind::ResourceExhaustion, true, false),
        _ => (
            Severity::Error,
            EventKind::ListenerPersistentError,
            false,
            true,
        ),
    };

    AcceptErrorClassification {
        severity,
        event_kind,
        should_backoff,
        is_fatal,
        fd_exhausted,
    }
}

/// Recognize a descriptor-exhaustion accept error.
///
/// Raw OS error numbers are matched by value rather than by host platform:
/// a Unix `EMFILE`/`ENFILE`-coded error and a Winsock
/// `WSAEMFILE`/`WSAENFILE`-coded error are both exhaustion everywhere. A
/// kernel only produces its own platform's codes, so the union only affects
/// synthetic or cross-platform errors; it keeps the predicate code-based
/// instead of host-dependent.
///
/// Errors without a raw OS number (non-OS errors, e.g. mocked accept
/// failures) fall back to a case-insensitive message match. That fallback is
/// the documented last resort: `accept()` errors originate from the kernel
/// and are not user-controlled, so the misclassification risk of matching
/// the error string is negligible.
pub fn is_fd_exhaustion(error: &std::io::Error) -> bool {
    if let Some(raw) = error.raw_os_error() {
        return raw == EMFILE_RAW
            || raw == ENFILE_RAW
            || raw == WSAEMFILE_RAW
            || raw == WSAENFILE_RAW;
    }

    let message = error.to_string().to_ascii_lowercase();
    message.contains("too many open files")
        || message.contains("emfile")
        || message.contains("enfile")
}

/// The backoff step for `index`, in milliseconds.
///
/// The index is clamped to the last ramp entry, so the delay never exceeds
/// [`ACCEPT_BACKOFF_MS`]'s final 500 ms step.
pub fn accept_backoff_ms(index: usize) -> u64 {
    ACCEPT_BACKOFF_MS[index.min(ACCEPT_BACKOFF_MS.len() - 1)]
}

/// The backoff delay for `index`.
///
/// The index is clamped to the last ramp entry, so the delay never exceeds
/// [`ACCEPT_BACKOFF_MS`]'s final 500 ms step.
pub fn accept_backoff(index: usize) -> Duration {
    Duration::from_millis(accept_backoff_ms(index))
}

/// Classify, account, and emit one accept-loop error.
///
/// Accept errors are never silently dropped: the `listener_errors` counter
/// is incremented and exactly one structured event is emitted through
/// `ops`, carrying the classified severity and event kind. The message is
/// sanitized before it reaches the sink.
///
/// Returns the classification so the caller can apply the loop policy
/// (bounded backoff, or termination for a fatal error).
pub fn report_accept_error(error: &std::io::Error, ops: &OpsContext) -> AcceptErrorClassification {
    let classification = classify_accept_error(error);
    ops.counters()
        .listener_errors
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    ops.emit(Event::new(
        classification.severity,
        classification.event_kind,
        crate::ops::sanitize_text_field(&format!("accept error: {error}")),
    ));
    classification
}

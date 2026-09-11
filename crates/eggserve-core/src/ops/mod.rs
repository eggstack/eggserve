//! Operational observability root (Plan 206 Track H).
//!
//! Ownership:
//! - `events` owns the single event/field schema vocabulary (`Severity`,
//!   `EventKind`, `Field`, `Event`, sanitization, JSON rendering).
//! - `sinks` owns `LogSink` implementations and containment semantics.
//! - `counters` owns `OpsCounters`/`OpsSnapshot`.
//! - This module owns the single runtime authority [`OpsContext`] plus the
//!   process-global compatibility [`Logger`]/[`global_counters`] path.
//!
//! Public import paths are preserved: `crate::ops::Event`,
//! `crate::ops::OpsContext`, etc. continue to resolve through re-exports
//! below. New code may import from the owning submodule directly.

pub mod counters;
pub mod events;
pub mod sinks;

pub use counters::{OpsCounters, OpsSnapshot};
pub use events::{
    event_to_json, sanitize_path, sanitize_text_field, truncate, Event, EventKind, Field, Severity,
    SCHEMA_VERSION,
};
pub use sinks::{CompositeLogSink, FilteredLogSink, LogFormat, LogSink, NopLogSink, StderrLogSink};

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

#[allow(dead_code)]
pub struct Logger {
    sink: Arc<dyn LogSink>,
}

#[allow(dead_code)]
static GLOBAL_LOGGER: OnceLock<Logger> = OnceLock::new();

static GLOBAL_OPS: OnceLock<OpsContext> = OnceLock::new();

/// Process-global compatibility counters.
///
/// This is the counter set owned by [`OpsContext::global`]. Live
/// server/connection execution prefers the explicit context carried by
/// [`crate::server::RuntimeState`]; the default runtime context clones the
/// global one, so CLI/default construction keeps historical behavior while
/// embedders with explicit contexts stay isolated.
pub fn global_counters() -> &'static OpsCounters {
    OpsContext::global().counters()
}

#[allow(dead_code)]
impl Logger {
    /// Install the global logger. Returns `Err(())` if a logger has already
    /// been installed; embedders should prefer [`Logger::try_init`].
    ///
    /// On success the sink is also adopted as the process-global runtime
    /// default (see [`OpsContext::global`]) when no global context exists
    /// yet, so default-constructed runtimes keep emitting to the CLI sink.
    /// Explicit per-runtime contexts created before this call are unaffected.
    #[allow(clippy::result_unit_err)]
    pub fn init(sink: Box<dyn LogSink>) -> Result<(), ()> {
        Self::install(sink)
    }

    #[allow(clippy::result_unit_err)]
    pub fn try_init(sink: Box<dyn LogSink>) -> Result<(), ()> {
        Self::install(sink)
    }

    fn install(sink: Box<dyn LogSink>) -> Result<(), ()> {
        let shared: Arc<dyn LogSink> = sink.into();
        GLOBAL_LOGGER
            .set(Logger {
                sink: shared.clone(),
            })
            .map_err(|_| ())?;
        // Best-effort: a global runtime context created later (the default
        // for `RuntimeState::try_new`) must observe the same sink the CLI
        // installed. If a global context already exists it keeps its sink;
        // explicit contexts never consult this path.
        let _ = GLOBAL_OPS.set(OpsContext::new(shared));
        Ok(())
    }

    pub fn global() -> &'static Logger {
        GLOBAL_LOGGER.get_or_init(|| Logger {
            sink: Arc::new(NopLogSink),
        })
    }

    pub fn emit(&self, event: Event) {
        self.sink.emit(&event);
    }

    pub fn emit_if(&self, condition: bool, event: Event) {
        if condition {
            self.sink.emit(&event);
        }
    }
}

/// Per-runtime observability context (Plan 181).
///
/// A small cloneable handle owning the three observability capabilities a
/// running server needs: the active [`LogSink`], the [`OpsCounters`], and
/// the connection correlation-ID source ([`CorrelationId`]). Clones share
/// one inner allocation, so handing the context to connection tasks is cheap
/// and counter/ID state stays coherent per runtime.
///
/// - Embedders that run several servers in one process build one context per
///   server (for example [`OpsContext::with_sinks`]) and attach it via
///   [`crate::server::RuntimeState::with_ops`] or
///   [`crate::server::ServerBuilder::ops_context`]. Events, counters, sink
///   failures, and correlation IDs are then isolated per runtime.
/// - [`OpsContext::global`] preserves the historical process-global default
///   for the CLI and for compatibility constructors. It is the same counter
///   set exposed by [`global_counters`].
/// - A no-op/default context ([`OpsContext::default`]) remains valid when
///   observability is not required.
///
/// Canonical request/response types never name this type; it travels with
/// runtime ownership ([`crate::server::RuntimeState`], connection activity)
/// rather than with messages.
#[derive(Clone)]
pub struct OpsContext {
    inner: Arc<OpsContextInner>,
}

struct OpsContextInner {
    sink: Arc<dyn LogSink>,
    counters: Arc<OpsCounters>,
    ids: CorrelationId,
}

impl std::fmt::Debug for OpsContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpsContext")
            .field("counters", &self.inner.counters)
            .finish_non_exhaustive()
    }
}

impl Default for OpsContext {
    /// No-op sink with fresh counters and a fresh correlation-ID source.
    fn default() -> Self {
        Self::new(Arc::new(NopLogSink))
    }
}

impl OpsContext {
    /// Build a context emitting to `sink` with fresh counters and IDs.
    pub fn new(sink: Arc<dyn LogSink>) -> Self {
        Self {
            inner: Arc::new(OpsContextInner {
                sink,
                counters: Arc::new(OpsCounters::new()),
                ids: CorrelationId::new(),
            }),
        }
    }

    /// Build a context from a boxed sink with fresh counters and IDs.
    pub fn from_boxed(sink: Box<dyn LogSink>) -> Self {
        Self::new(sink.into())
    }

    /// Build a context fanning out to `sinks` through a [`CompositeLogSink`]
    /// whose contained child-sink failures are counted in this context's own
    /// `dropped_log_events` rather than the process-global counter.
    pub fn with_sinks(sinks: Vec<Box<dyn LogSink>>) -> Self {
        let counters = Arc::new(OpsCounters::new());
        let composite = CompositeLogSink::with_failure_counters(sinks, counters.clone());
        Self {
            inner: Arc::new(OpsContextInner {
                sink: Arc::new(composite),
                counters,
                ids: CorrelationId::new(),
            }),
        }
    }

    /// Process-global default context.
    ///
    /// Initialized once with a no-op sink unless [`Logger::init`] adopted a
    /// real sink first. Default-constructed runtimes clone this context, so
    /// CLI initialization order (logger first, server second) keeps working
    /// without new mandatory configuration.
    pub fn global() -> &'static Self {
        GLOBAL_OPS.get_or_init(Self::default)
    }

    /// Emit one event to this context's sink.
    ///
    /// A panicking sink is contained here (Plan 178 semantics carried into
    /// the per-runtime path): the failure increments this context's
    /// `dropped_log_events` and no synthetic event is emitted, so a failing
    /// sink can never recursively re-enter itself through another context.
    pub fn emit(&self, event: Event) {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.inner.sink.emit(&event);
        }));
        if result.is_err() {
            self.inner
                .counters
                .dropped_log_events
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Emit one event when `condition` holds.
    pub fn emit_if(&self, condition: bool, event: Event) {
        if condition {
            self.emit(event);
        }
    }

    /// This context's counters.
    pub fn counters(&self) -> &OpsCounters {
        &self.inner.counters
    }

    /// Shared ownership of this context's counters, for wiring a
    /// prebuilt [`CompositeLogSink::with_failure_counters`].
    pub fn counters_arc(&self) -> Arc<OpsCounters> {
        self.inner.counters.clone()
    }

    /// Non-blocking, bounded snapshot of this context's counters.
    ///
    /// Reads never reset. No exporter or endpoint is involved.
    pub fn snapshot(&self) -> OpsSnapshot {
        self.inner.counters.snapshot()
    }

    /// Allocate the next connection correlation ID for this runtime.
    ///
    /// IDs start at 1 per context and are coherent within the runtime that
    /// owns the context. An explicit caller-supplied connection ID (see
    /// `serve_http1_connection_with_id`) still takes precedence where the
    /// driver contract provides one.
    pub fn next_connection_id(&self) -> u64 {
        self.inner.ids.next()
    }
}

pub struct CorrelationId {
    connection_id: AtomicU64,
}

impl Default for CorrelationId {
    fn default() -> Self {
        Self::new()
    }
}

impl CorrelationId {
    pub fn new() -> Self {
        Self {
            connection_id: AtomicU64::new(1),
        }
    }

    pub fn next(&self) -> u64 {
        self.connection_id.fetch_add(1, Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correlation_id_increments() {
        let cid = CorrelationId::new();
        assert_eq!(cid.next(), 1);
        assert_eq!(cid.next(), 2);
        assert_eq!(cid.next(), 3);
    }
}

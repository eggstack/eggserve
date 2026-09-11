//! Ops sink implementations (Plan 206 Track H).
//!
//! Owns the [`LogSink`] trait and its implementations. Failure accounting
//! stays context-local and non-recursive: contained child-sink panics
//! increment `dropped_log_events` in the owning context; no synthetic event
//! is re-emitted through a logger.

use std::sync::Arc;

use super::counters::OpsCounters;
use super::events::{event_to_json, Event};

pub trait LogSink: Send + Sync {
    fn emit(&self, event: &Event);
    fn flush(&self);
}

pub struct NopLogSink;

impl LogSink for NopLogSink {
    fn emit(&self, _event: &Event) {}
    fn flush(&self) {}
}

/// A log sink that wraps another sink and only forwards events at or above
/// a minimum severity level. Used for `--quiet` mode.
pub struct FilteredLogSink {
    inner: Box<dyn LogSink>,
    min_severity: super::events::Severity,
}

impl FilteredLogSink {
    pub fn new(inner: Box<dyn LogSink>, min_severity: super::events::Severity) -> Self {
        Self {
            inner,
            min_severity,
        }
    }
}

impl LogSink for FilteredLogSink {
    fn emit(&self, event: &Event) {
        if event.severity >= self.min_severity {
            self.inner.emit(event);
        }
    }

    fn flush(&self) {
        self.inner.flush();
    }
}

pub struct CompositeLogSink {
    sinks: Vec<Box<dyn LogSink>>,
    /// Failure accounting for contained child-sink panics. `None` preserves
    /// the historical process-global accounting via
    /// [`super::global_counters`]; per-runtime contexts should prefer
    /// [`CompositeLogSink::with_failure_counters`] or
    /// [`super::OpsContext::with_sinks`] so a failing sink is counted in the
    /// same context that owns it (Plan 181 Track B2).
    failure_counters: Option<Arc<OpsCounters>>,
}

impl CompositeLogSink {
    pub fn new(sinks: Vec<Box<dyn LogSink>>) -> Self {
        Self {
            sinks,
            failure_counters: None,
        }
    }

    /// Build a composite whose contained child-sink failures are counted in
    /// `counters` instead of the process-global counters.
    ///
    /// accounting is still non-recursive: only the counter is incremented,
    /// never a synthetic event through a logger that could re-enter this
    /// same composite.
    pub fn with_failure_counters(sinks: Vec<Box<dyn LogSink>>, counters: Arc<OpsCounters>) -> Self {
        Self {
            sinks,
            failure_counters: Some(counters),
        }
    }

    fn failure_counters(&self) -> &OpsCounters {
        match &self.failure_counters {
            Some(counters) => counters,
            None => super::global_counters(),
        }
    }
}

impl LogSink for CompositeLogSink {
    fn emit(&self, event: &Event) {
        for sink in &self.sinks {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                sink.emit(event);
            }));
            if result.is_err() {
                self.failure_counters()
                    .dropped_log_events
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                // Intentionally no synthetic `LogSinkFailure` emission here.
                // Reporting through `Logger::global()` would re-enter this
                // same composite when it is installed globally, invoking the
                // same failing sink again. Failure accounting stays in the
                // `dropped_log_events` counter; iteration continues so healthy
                // siblings still receive the original event.
            }
        }
    }
    fn flush(&self) {
        for sink in &self.sinks {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                sink.flush();
            }));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Text,
    Json,
}

pub struct StderrLogSink {
    pub log_format: LogFormat,
}

impl LogSink for StderrLogSink {
    fn emit(&self, event: &Event) {
        match self.log_format {
            LogFormat::Text => {
                use std::fmt::Write;

                let mut line = format!("[{}] {}: {}", event.severity, event.event, event.message);
                if let Some(cid) = event.connection_id {
                    write!(&mut line, " conn={}", cid).expect("writing to String cannot fail");
                }
                if let Some(seq) = event.request_seq {
                    write!(&mut line, " seq={}", seq).expect("writing to String cannot fail");
                }
                for f in &event.fields {
                    write!(&mut line, " {}", f).expect("writing to String cannot fail");
                }
                eprintln!("{}", line);
            }
            LogFormat::Json => {
                let json = event_to_json(event);
                eprintln!("{}", json);
            }
        }
    }

    fn flush(&self) {}
}

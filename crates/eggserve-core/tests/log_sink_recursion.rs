//! Plan 178 Track B: composite installed via the global path must not recurse.
//!
//! This binary exists so `Logger::try_init` succeeds deterministically: each
//! integration test binary has a fresh `OnceLock` global, so installing the
//! failing composite here exercises the same global/default path that made
//! recursion possible (standalone composites with a default `Nop` global
//! would never recurse).

use eggserve_core::ops::{CompositeLogSink, Event, EventKind, LogSink, Logger, Severity};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};

struct CountingPanicSink {
    calls: Arc<AtomicU64>,
}

impl LogSink for CountingPanicSink {
    fn emit(&self, _event: &Event) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        panic!("boom");
    }
    fn flush(&self) {}
}

struct RecordingSink {
    events: Arc<Mutex<Vec<String>>>,
}

impl LogSink for RecordingSink {
    fn emit(&self, event: &Event) {
        if let Ok(mut guard) = self.events.lock() {
            guard.push(event.event.to_string());
        }
    }
    fn flush(&self) {}
}

struct FlushPanicSink;

impl LogSink for FlushPanicSink {
    fn emit(&self, _event: &Event) {}
    fn flush(&self) {
        panic!("flush boom");
    }
}

#[test]
fn global_composite_with_panicking_child_does_not_recurse() {
    let calls = Arc::new(AtomicU64::new(0));
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let composite = CompositeLogSink::new(vec![
        Box::new(CountingPanicSink {
            calls: calls.clone(),
        }),
        Box::new(RecordingSink {
            events: recorded.clone(),
        }),
    ]);

    Logger::try_init(Box::new(composite))
        .expect("fresh test binary must install the global composite");

    let counters = eggserve_core::ops::global_counters();
    let before = counters.dropped_log_events.load(Ordering::Relaxed);

    // Must return normally despite the panicking child, even though the
    // global logger IS the failing composite (the old recursive path).
    Logger::global().emit(Event::new(
        Severity::Info,
        EventKind::ProcessStarting,
        "recursion probe",
    ));

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "panicking sink must run exactly once per emit (no recursive re-entry)"
    );
    let events = recorded.lock().unwrap();
    assert_eq!(
        events.len(),
        1,
        "healthy sibling must receive exactly the original event, got: {events:?}"
    );
    assert_eq!(events[0], "process_starting");
    drop(events);

    let after = counters.dropped_log_events.load(Ordering::Relaxed);
    assert_eq!(
        after,
        before + 1,
        "dropped counter must increment deterministically by one"
    );

    // Second emit proves determinism, not a one-shot synthetic loop.
    Logger::global().emit(Event::new(
        Severity::Info,
        EventKind::ProcessStarting,
        "second probe",
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(recorded.lock().unwrap().len(), 2);
    let after2 = counters.dropped_log_events.load(Ordering::Relaxed);
    assert_eq!(after2, after + 1);
}

#[test]
fn composite_flush_panic_is_contained_standalone() {
    // Flush never went through the global path; standalone containment is
    // sufficient and must not panic out of the caller.
    let composite = CompositeLogSink::new(vec![
        Box::new(FlushPanicSink),
        Box::new(RecordingSink {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
    ]);
    composite.flush();
}

use std::borrow::Cow;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::SystemTime;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Debug,
    Info,
    Warn,
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Debug => write!(f, "DEBUG"),
            Severity::Info => write!(f, "INFO"),
            Severity::Warn => write!(f, "WARN"),
            Severity::Error => write!(f, "ERROR"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    // Process/Config
    ProcessStarting,
    RootInitialized,
    ListenerReady,
    ShutdownRequested,
    DrainingStarted,
    ForcedShutdownStarted,
    ShutdownComplete,

    // Connection
    ConnectionAccepted,
    ConnectionRejected,
    TlsHandshakeSuccess,
    TlsHandshakeFailure,
    TlsHandshakeTimeout,
    HeaderTimeout,
    BodyReadTimeout,
    ParserRejection,
    KeepAliveClosed,
    ResponseWriteTimeout,
    ClientDisconnect,
    ConnectionPanic,

    // Request/Service
    RequestCompleted,
    FileNotFound,
    FileDenied,
    FileError,
    DotfileDenied,
    SymlinkDenied,
    RootEscapeDenied,
    BodyPolicyRejection,
    ServiceTimeout,
    ServiceError,
    DirectoryListingLimit,

    // Operational
    ListenerTransientError,
    ListenerPersistentError,
    ResourceExhaustion,
    BlockingWorkerSaturation,
    LogSinkFailure,
}

impl fmt::Display for EventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            EventKind::ProcessStarting => "process_starting",
            EventKind::RootInitialized => "root_initialized",
            EventKind::ListenerReady => "listener_ready",
            EventKind::ShutdownRequested => "shutdown_requested",
            EventKind::DrainingStarted => "draining_started",
            EventKind::ForcedShutdownStarted => "forced_shutdown_started",
            EventKind::ShutdownComplete => "shutdown_complete",

            EventKind::ConnectionAccepted => "connection_accepted",
            EventKind::ConnectionRejected => "connection_rejected",
            EventKind::TlsHandshakeSuccess => "tls_handshake_success",
            EventKind::TlsHandshakeFailure => "tls_handshake_failure",
            EventKind::TlsHandshakeTimeout => "tls_handshake_timeout",
            EventKind::HeaderTimeout => "header_timeout",
            EventKind::BodyReadTimeout => "body_read_timeout",
            EventKind::ParserRejection => "parser_rejection",
            EventKind::KeepAliveClosed => "keep_alive_closed",
            EventKind::ResponseWriteTimeout => "response_write_timeout",
            EventKind::ClientDisconnect => "client_disconnect",
            EventKind::ConnectionPanic => "connection_panic",

            EventKind::RequestCompleted => "request_completed",
            EventKind::FileNotFound => "file_not_found",
            EventKind::FileDenied => "file_denied",
            EventKind::FileError => "file_error",
            EventKind::DotfileDenied => "dotfile_denied",
            EventKind::SymlinkDenied => "symlink_denied",
            EventKind::RootEscapeDenied => "root_escape_denied",
            EventKind::BodyPolicyRejection => "body_policy_rejection",
            EventKind::ServiceTimeout => "service_timeout",
            EventKind::ServiceError => "service_error",
            EventKind::DirectoryListingLimit => "directory_listing_limit",

            EventKind::ListenerTransientError => "listener_transient_error",
            EventKind::ListenerPersistentError => "listener_persistent_error",
            EventKind::ResourceExhaustion => "resource_exhaustion",
            EventKind::BlockingWorkerSaturation => "blocking_worker_saturation",
            EventKind::LogSinkFailure => "log_sink_failure",
        };
        write!(f, "{}", name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Field {
    Bool(String, bool),
    I64(String, i64),
    U64(String, u64),
    Str(String, String),
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Field::Bool(k, v) => write!(f, "\"{}\": {}", k, v),
            Field::I64(k, v) => write!(f, "\"{}\": {}", k, v),
            Field::U64(k, v) => write!(f, "\"{}\": {}", k, v),
            Field::Str(k, v) => write!(f, "\"{}\": \"{}\"", k, escape_json_string(v)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Event {
    pub schema_version: u32,
    pub severity: Severity,
    pub event: EventKind,
    pub timestamp: String,
    pub message: String,
    pub connection_id: Option<u64>,
    pub request_seq: Option<u32>,
    pub fields: Vec<Field>,
}

impl Event {
    pub fn new(severity: Severity, event: EventKind, message: impl Into<String>) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            severity,
            event,
            timestamp: rfc3339_now(),
            message: message.into(),
            connection_id: None,
            request_seq: None,
            fields: Vec::new(),
        }
    }

    pub fn field(mut self, field: Field) -> Self {
        self.fields.push(field);
        self
    }

    pub fn connection_id(mut self, id: u64) -> Self {
        self.connection_id = Some(id);
        self
    }

    pub fn request_seq(mut self, seq: u32) -> Self {
        self.request_seq = Some(seq);
        self
    }
}

fn rfc3339_now() -> String {
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();

    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;
    let millis = dur.subsec_millis();

    // Civil date from days since 1970-01-01
    let (year, month, day) = days_to_civil(days_since_epoch);

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        year, month, day, hours, minutes, seconds, millis
    )
}

fn days_to_civil(days: u64) -> (u64, u64, u64) {
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

pub fn sanitize_text_field(text: &str) -> String {
    let filtered: String = text
        .chars()
        .filter(|c| {
            let code = *c as u32;
            if code < 0x20 {
                return false;
            }
            if code == 0x7F {
                return false;
            }
            if code == 0x1B {
                return false;
            }
            if code > 0x7E {
                return false;
            }
            true
        })
        .collect();
    truncate_str(&filtered, 512).into_owned()
}

pub fn sanitize_path(path: &str) -> String {
    let last_component = path.rsplit('/').next().unwrap_or(path);
    let without_query = last_component.split('?').next().unwrap_or(last_component);
    let sanitized: String = without_query
        .chars()
        .filter(|c| {
            let code = *c as u32;
            (0x20..0x7F).contains(&code) && code != 0x1B
        })
        .collect();
    truncate_str(&sanitized, 127).into_owned()
}

pub fn truncate(text: &str, max_len: usize) -> Cow<'_, str> {
    truncate_str(text, max_len)
}

fn truncate_str(text: &str, max_len: usize) -> Cow<'_, str> {
    if text.len() <= max_len {
        return Cow::Borrowed(text);
    }
    // Find a char boundary at or before max_len
    let mut end = max_len;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    Cow::Owned(format!("{}…", &text[..end]))
}

pub trait LogSink: Send + Sync {
    fn emit(&self, event: &Event);
    fn flush(&self);
}

pub struct NopLogSink;

impl LogSink for NopLogSink {
    fn emit(&self, _event: &Event) {}
    fn flush(&self) {}
}

pub struct CompositeLogSink {
    sinks: Vec<Box<dyn LogSink>>,
}

impl CompositeLogSink {
    pub fn new(sinks: Vec<Box<dyn LogSink>>) -> Self {
        Self { sinks }
    }
}

impl LogSink for CompositeLogSink {
    fn emit(&self, event: &Event) {
        for sink in &self.sinks {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                sink.emit(event);
            }));
            if result.is_err() {
                global_counters()
                    .dropped_log_events
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                // Emit a LogSinkFailure event to surface sink panics.
                // Use catch_unwind to prevent recursive failure.
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    Logger::global().emit(Event::new(
                        Severity::Error,
                        EventKind::LogSinkFailure,
                        "log sink panicked",
                    ));
                }));
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
                let mut line = format!("[{}] {}: {}", event.severity, event.event, event.message);
                if let Some(cid) = event.connection_id {
                    line.push_str(&format!(" conn={}", cid));
                }
                if let Some(seq) = event.request_seq {
                    line.push_str(&format!(" seq={}", seq));
                }
                for f in &event.fields {
                    line.push_str(&format!(" {}", f));
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

pub fn event_to_json(event: &Event) -> String {
    let mut out = String::with_capacity(256);
    out.push('{');

    out.push_str("\"schema_version\":");
    out.push_str(&event.schema_version.to_string());

    out.push_str(",\"severity\":\"");
    out.push_str(&format!("{}", event.severity));
    out.push('"');

    out.push_str(",\"event\":\"");
    out.push_str(&format!("{}", event.event));
    out.push('"');

    out.push_str(",\"timestamp\":\"");
    out.push_str(&escape_json_string(&event.timestamp));
    out.push('"');

    out.push_str(",\"message\":\"");
    out.push_str(&escape_json_string(&event.message));
    out.push('"');

    if let Some(cid) = event.connection_id {
        out.push_str(",\"connection_id\":");
        out.push_str(&cid.to_string());
    }

    if let Some(seq) = event.request_seq {
        out.push_str(",\"request_seq\":");
        out.push_str(&seq.to_string());
    }

    if !event.fields.is_empty() {
        out.push_str(",\"fields\":[");
        for (i, f) in event.fields.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push('{');
            match f {
                Field::Bool(k, v) => {
                    out.push('"');
                    out.push_str(&escape_json_string(k));
                    out.push_str("\":");
                    out.push_str(if *v { "true" } else { "false" });
                }
                Field::I64(k, v) => {
                    out.push('"');
                    out.push_str(&escape_json_string(k));
                    out.push_str("\":");
                    out.push_str(&v.to_string());
                }
                Field::U64(k, v) => {
                    out.push('"');
                    out.push_str(&escape_json_string(k));
                    out.push_str("\":");
                    out.push_str(&v.to_string());
                }
                Field::Str(k, v) => {
                    out.push('"');
                    out.push_str(&escape_json_string(k));
                    out.push_str("\":\"");
                    out.push_str(&escape_json_string(v));
                    out.push('"');
                }
            }
            out.push('}');
        }
        out.push(']');
    }

    out.push('}');
    out
}

pub(crate) fn escape_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            _ => out.push(c),
        }
    }
    out
}

#[allow(dead_code)]
pub struct Logger {
    sink: Box<dyn LogSink>,
}

#[allow(dead_code)]
static GLOBAL_LOGGER: OnceLock<Logger> = OnceLock::new();

static GLOBAL_COUNTERS: OnceLock<OpsCounters> = OnceLock::new();

pub fn global_counters() -> &'static OpsCounters {
    GLOBAL_COUNTERS.get_or_init(OpsCounters::new)
}

#[allow(dead_code)]
impl Logger {
    pub fn init(sink: Box<dyn LogSink>) {
        GLOBAL_LOGGER
            .set(Logger { sink })
            .ok()
            .expect("Logger::init called more than once");
    }

    #[allow(clippy::result_unit_err)]
    pub fn try_init(sink: Box<dyn LogSink>) -> Result<(), ()> {
        GLOBAL_LOGGER.set(Logger { sink }).map_err(|_| ())
    }

    pub fn global() -> &'static Logger {
        GLOBAL_LOGGER.get_or_init(|| Logger {
            sink: Box::new(NopLogSink),
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

#[derive(Debug)]
pub struct OpsCounters {
    pub connections_accepted: AtomicU64,
    pub connections_rejected: AtomicU64,
    pub active_connections: AtomicU64,
    pub active_file_streams: AtomicU64,
    pub parser_rejects: AtomicU64,
    pub header_timeouts: AtomicU64,
    pub body_read_timeouts: AtomicU64,
    pub response_write_timeouts: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub graceful_shutdowns: AtomicU64,
    pub forced_shutdowns: AtomicU64,
    pub listener_errors: AtomicU64,
    pub dropped_log_events: AtomicU64,
}

impl Default for OpsCounters {
    fn default() -> Self {
        Self::new()
    }
}

impl OpsCounters {
    pub fn new() -> Self {
        Self {
            connections_accepted: AtomicU64::new(0),
            connections_rejected: AtomicU64::new(0),
            active_connections: AtomicU64::new(0),
            active_file_streams: AtomicU64::new(0),
            parser_rejects: AtomicU64::new(0),
            header_timeouts: AtomicU64::new(0),
            body_read_timeouts: AtomicU64::new(0),
            response_write_timeouts: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            graceful_shutdowns: AtomicU64::new(0),
            forced_shutdowns: AtomicU64::new(0),
            listener_errors: AtomicU64::new(0),
            dropped_log_events: AtomicU64::new(0),
        }
    }

    pub fn snapshot(&self) -> OpsSnapshot {
        OpsSnapshot {
            connections_accepted: self.connections_accepted.load(Ordering::Relaxed),
            connections_rejected: self.connections_rejected.load(Ordering::Relaxed),
            active_connections: self.active_connections.load(Ordering::Relaxed),
            active_file_streams: self.active_file_streams.load(Ordering::Relaxed),
            parser_rejects: self.parser_rejects.load(Ordering::Relaxed),
            header_timeouts: self.header_timeouts.load(Ordering::Relaxed),
            body_read_timeouts: self.body_read_timeouts.load(Ordering::Relaxed),
            response_write_timeouts: self.response_write_timeouts.load(Ordering::Relaxed),
            bytes_sent: self.bytes_sent.load(Ordering::Relaxed),
            graceful_shutdowns: self.graceful_shutdowns.load(Ordering::Relaxed),
            forced_shutdowns: self.forced_shutdowns.load(Ordering::Relaxed),
            listener_errors: self.listener_errors.load(Ordering::Relaxed),
            dropped_log_events: self.dropped_log_events.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpsSnapshot {
    pub connections_accepted: u64,
    pub connections_rejected: u64,
    pub active_connections: u64,
    pub active_file_streams: u64,
    pub parser_rejects: u64,
    pub header_timeouts: u64,
    pub body_read_timeouts: u64,
    pub response_write_timeouts: u64,
    pub bytes_sent: u64,
    pub graceful_shutdowns: u64,
    pub forced_shutdowns: u64,
    pub listener_errors: u64,
    pub dropped_log_events: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_text_removes_control_chars() {
        assert_eq!(sanitize_text_field("hello\r\nworld"), "helloworld");
        assert_eq!(sanitize_text_field("tab\there"), "tabhere");
        assert_eq!(sanitize_text_field("esc\x1B[31mred"), "esc[31mred");
        assert_eq!(sanitize_text_field("null\x00byte\x7Fdel"), "nullbytedel");
        assert_eq!(sanitize_text_field("normal text"), "normal text");
    }

    #[test]
    fn sanitize_path_extracts_last_component() {
        assert_eq!(sanitize_path("/foo/bar/baz.txt"), "baz.txt");
        assert_eq!(sanitize_path("no/slash/here/"), "");
        assert_eq!(sanitize_path("only-one"), "only-one");
        assert_eq!(sanitize_path("/a/b/c/d/e/f.txt"), "f.txt");
    }

    #[test]
    fn sanitize_path_truncates_long_paths() {
        let long_name: String = "a".repeat(200);
        let result = sanitize_path(&format!("/prefix/{}", long_name));
        assert!(result.chars().count() <= 128);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn event_timestamp_is_valid() {
        let ev = Event::new(Severity::Info, EventKind::ProcessStarting, "test");
        // Format: YYYY-MM-DDTHH:MM:SS.mmmZ
        assert!(ev.timestamp.ends_with('Z'));
        assert_eq!(ev.timestamp.len(), 24);
        assert!(ev.timestamp.contains('T'));
        // Dashes in date part
        assert_eq!(ev.timestamp.matches('-').count(), 2);
        // Colons in time part
        assert_eq!(ev.timestamp.matches(':').count(), 2);
    }

    #[test]
    fn correlation_id_increments() {
        let cid = CorrelationId::new();
        assert_eq!(cid.next(), 1);
        assert_eq!(cid.next(), 2);
        assert_eq!(cid.next(), 3);
    }

    #[test]
    fn ops_counters_snapshot() {
        let counters = OpsCounters::new();
        counters
            .connections_accepted
            .fetch_add(5, Ordering::Relaxed);
        counters.bytes_sent.fetch_add(1024, Ordering::Relaxed);
        counters.listener_errors.fetch_add(1, Ordering::Relaxed);

        let snap = counters.snapshot();
        assert_eq!(snap.connections_accepted, 5);
        assert_eq!(snap.bytes_sent, 1024);
        assert_eq!(snap.listener_errors, 1);
        assert_eq!(snap.connections_rejected, 0);
        assert_eq!(snap.active_connections, 0);
    }
}

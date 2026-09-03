use std::borrow::Cow;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::SystemTime;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd)]
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
    HeaderBytesRejected,
    RequestTargetTooLong,
    ServiceAdmissionRejected,
    KeepAliveClosed,
    KeepAliveIdleTimeout,
    MaxRequestsClose,
    WriteStallTimeout,
    ConnectionTotalTimeout,
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
    IncompleteBodyClose,
    ServiceInvocationSuppressed,
    ServiceTimeout,
    ServiceError,
    DirectoryListingLimit,
    // Streaming responses (Plan 162)
    ResponseStreamStarted,
    ResponseStreamCompleted,
    ResponseStreamLengthMismatch,
    ResponseStreamProducerError,
    ResponseStreamProducerPanic,
    ResponseStreamCancelled,

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
            EventKind::HeaderBytesRejected => "header_bytes_rejected",
            EventKind::RequestTargetTooLong => "request_target_too_long",
            EventKind::ServiceAdmissionRejected => "service_admission_rejected",
            EventKind::KeepAliveClosed => "keep_alive_closed",
            EventKind::KeepAliveIdleTimeout => "keep_alive_idle_timeout",
            EventKind::MaxRequestsClose => "max_requests_close",
            EventKind::WriteStallTimeout => "write_stall_timeout",
            EventKind::ConnectionTotalTimeout => "connection_total_timeout",
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
            EventKind::IncompleteBodyClose => "incomplete_body_close",
            EventKind::ServiceInvocationSuppressed => "service_invocation_suppressed",
            EventKind::ServiceTimeout => "service_timeout",
            EventKind::ServiceError => "service_error",
            EventKind::DirectoryListingLimit => "directory_listing_limit",
            EventKind::ResponseStreamStarted => "response_stream_started",
            EventKind::ResponseStreamCompleted => "response_stream_completed",
            EventKind::ResponseStreamLengthMismatch => "response_stream_length_mismatch",
            EventKind::ResponseStreamProducerError => "response_stream_producer_error",
            EventKind::ResponseStreamProducerPanic => "response_stream_producer_panic",
            EventKind::ResponseStreamCancelled => "response_stream_cancelled",

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
            // Printable ASCII only; this also excludes control characters
            // (0x00-0x1F, including ESC) and DEL (0x7F). The directory-listing
            // `server::static_service::html_escape` intentionally renders DEL
            // distinctly as an entity; logs instead remove it to keep fields
            // printable and bounded.
            (0x20..=0x7E).contains(&code)
        })
        .collect();
    truncate_str(&filtered, 512).into_owned()
}

pub fn sanitize_path(path: &str) -> String {
    let without_query = path.split('?').next().unwrap_or(path);
    let last_component = without_query
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(without_query);
    let last_component = if last_component.is_empty() && without_query == "/" {
        "/"
    } else {
        last_component
    };
    let sanitized: String = last_component
        .chars()
        .filter(|c| (0x20..=0x7E).contains(&(*c as u32)))
        .collect();
    truncate_str(&sanitized, 127).into_owned()
}

pub fn truncate(text: &str, max_len: usize) -> Cow<'_, str> {
    truncate_str(text, max_len)
}

fn truncate_str(text: &str, max_len: usize) -> Cow<'_, str> {
    // `max_len` counts characters, while the sentinel is appended after the
    // retained prefix. Finding the next boundary also proves whether the
    // string needs truncation in the same pass.
    match text.char_indices().nth(max_len) {
        Some((end, _)) => Cow::Owned(format!("{}…", &text[..end])),
        None => Cow::Borrowed(text),
    }
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

/// A log sink that wraps another sink and only forwards events at or above
/// a minimum severity level. Used for `--quiet` mode.
pub struct FilteredLogSink {
    inner: Box<dyn LogSink>,
    min_severity: Severity,
}

impl FilteredLogSink {
    pub fn new(inner: Box<dyn LogSink>, min_severity: Severity) -> Self {
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

pub fn event_to_json(event: &Event) -> String {
    use std::fmt::Write;

    let mut out = String::with_capacity(256);
    out.push('{');

    out.push_str("\"schema_version\":");
    write!(&mut out, "{}", event.schema_version).unwrap();

    out.push_str(",\"severity\":\"");
    write!(&mut out, "{}", event.severity).unwrap();
    out.push('"');

    out.push_str(",\"event\":\"");
    write!(&mut out, "{}", event.event).unwrap();
    out.push('"');

    out.push_str(",\"timestamp\":\"");
    escape_json_string_into(&mut out, &event.timestamp);
    out.push('"');

    out.push_str(",\"message\":\"");
    escape_json_string_into(&mut out, &event.message);
    out.push('"');

    if let Some(cid) = event.connection_id {
        out.push_str(",\"connection_id\":");
        write!(&mut out, "{}", cid).unwrap();
    }

    if let Some(seq) = event.request_seq {
        out.push_str(",\"request_seq\":");
        write!(&mut out, "{}", seq).unwrap();
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
                    escape_json_string_into(&mut out, k);
                    out.push_str("\":");
                    out.push_str(if *v { "true" } else { "false" });
                }
                Field::I64(k, v) => {
                    out.push('"');
                    escape_json_string_into(&mut out, k);
                    out.push_str("\":");
                    write!(&mut out, "{}", v).unwrap();
                }
                Field::U64(k, v) => {
                    out.push('"');
                    escape_json_string_into(&mut out, k);
                    out.push_str("\":");
                    write!(&mut out, "{}", v).unwrap();
                }
                Field::Str(k, v) => {
                    out.push('"');
                    escape_json_string_into(&mut out, k);
                    out.push_str("\":\"");
                    escape_json_string_into(&mut out, v);
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
    escape_json_string_into(&mut out, s);
    out
}

fn escape_json_string_into(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str("\\u");
                let value = c as u32;
                for shift in [12, 8, 4, 0] {
                    let digit = ((value >> shift) & 0xf) as u8;
                    out.push(char::from(b"0123456789abcdef"[digit as usize]));
                }
            }
            _ => out.push(c),
        }
    }
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
    /// Install the global logger. Returns `Err(())` if a logger has already
    /// been installed; embedders should prefer [`Logger::try_init`].
    #[allow(clippy::result_unit_err)]
    pub fn init(sink: Box<dyn LogSink>) -> Result<(), ()> {
        GLOBAL_LOGGER.set(Logger { sink }).map_err(|_| ())
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
    pub active_service_requests: AtomicU64,
    pub connection_panics: AtomicU64,
    pub parser_rejects: AtomicU64,
    pub header_bytes_rejected: AtomicU64,
    pub request_target_rejected: AtomicU64,
    pub service_admission_rejected: AtomicU64,
    pub body_rejections: AtomicU64,
    pub header_timeouts: AtomicU64,
    pub body_read_timeouts: AtomicU64,
    pub keepalive_idle_timeouts: AtomicU64,
    pub max_requests_closes: AtomicU64,
    pub write_stall_timeouts: AtomicU64,
    pub connection_total_timeouts: AtomicU64,
    pub graceful_shutdowns: AtomicU64,
    pub forced_shutdowns: AtomicU64,
    pub listener_errors: AtomicU64,
    pub dropped_log_events: AtomicU64,
    pub streaming_started: AtomicU64,
    pub streaming_completed: AtomicU64,
    pub stream_length_mismatches: AtomicU64,
    pub stream_producer_errors: AtomicU64,
    pub stream_producer_panics: AtomicU64,
    pub stream_cancelled: AtomicU64,
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
            active_service_requests: AtomicU64::new(0),
            connection_panics: AtomicU64::new(0),
            parser_rejects: AtomicU64::new(0),
            header_bytes_rejected: AtomicU64::new(0),
            request_target_rejected: AtomicU64::new(0),
            service_admission_rejected: AtomicU64::new(0),
            body_rejections: AtomicU64::new(0),
            header_timeouts: AtomicU64::new(0),
            body_read_timeouts: AtomicU64::new(0),
            keepalive_idle_timeouts: AtomicU64::new(0),
            max_requests_closes: AtomicU64::new(0),
            write_stall_timeouts: AtomicU64::new(0),
            connection_total_timeouts: AtomicU64::new(0),
            graceful_shutdowns: AtomicU64::new(0),
            forced_shutdowns: AtomicU64::new(0),
            listener_errors: AtomicU64::new(0),
            dropped_log_events: AtomicU64::new(0),
            streaming_started: AtomicU64::new(0),
            streaming_completed: AtomicU64::new(0),
            stream_length_mismatches: AtomicU64::new(0),
            stream_producer_errors: AtomicU64::new(0),
            stream_producer_panics: AtomicU64::new(0),
            stream_cancelled: AtomicU64::new(0),
        }
    }

    pub fn snapshot(&self) -> OpsSnapshot {
        OpsSnapshot {
            connections_accepted: self.connections_accepted.load(Ordering::Relaxed),
            connections_rejected: self.connections_rejected.load(Ordering::Relaxed),
            active_connections: self.active_connections.load(Ordering::Relaxed),
            active_file_streams: self.active_file_streams.load(Ordering::Relaxed),
            active_service_requests: self.active_service_requests.load(Ordering::Relaxed),
            connection_panics: self.connection_panics.load(Ordering::Relaxed),
            parser_rejects: self.parser_rejects.load(Ordering::Relaxed),
            header_bytes_rejected: self.header_bytes_rejected.load(Ordering::Relaxed),
            request_target_rejected: self.request_target_rejected.load(Ordering::Relaxed),
            service_admission_rejected: self.service_admission_rejected.load(Ordering::Relaxed),
            body_rejections: self.body_rejections.load(Ordering::Relaxed),
            header_timeouts: self.header_timeouts.load(Ordering::Relaxed),
            body_read_timeouts: self.body_read_timeouts.load(Ordering::Relaxed),
            keepalive_idle_timeouts: self.keepalive_idle_timeouts.load(Ordering::Relaxed),
            max_requests_closes: self.max_requests_closes.load(Ordering::Relaxed),
            write_stall_timeouts: self.write_stall_timeouts.load(Ordering::Relaxed),
            connection_total_timeouts: self.connection_total_timeouts.load(Ordering::Relaxed),
            graceful_shutdowns: self.graceful_shutdowns.load(Ordering::Relaxed),
            forced_shutdowns: self.forced_shutdowns.load(Ordering::Relaxed),
            listener_errors: self.listener_errors.load(Ordering::Relaxed),
            dropped_log_events: self.dropped_log_events.load(Ordering::Relaxed),
            streaming_started: self.streaming_started.load(Ordering::Relaxed),
            streaming_completed: self.streaming_completed.load(Ordering::Relaxed),
            stream_length_mismatches: self.stream_length_mismatches.load(Ordering::Relaxed),
            stream_producer_errors: self.stream_producer_errors.load(Ordering::Relaxed),
            stream_producer_panics: self.stream_producer_panics.load(Ordering::Relaxed),
            stream_cancelled: self.stream_cancelled.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpsSnapshot {
    pub connections_accepted: u64,
    pub connections_rejected: u64,
    pub active_connections: u64,
    pub active_file_streams: u64,
    pub active_service_requests: u64,
    pub connection_panics: u64,
    pub parser_rejects: u64,
    pub header_bytes_rejected: u64,
    pub request_target_rejected: u64,
    pub service_admission_rejected: u64,
    pub body_rejections: u64,
    pub header_timeouts: u64,
    pub body_read_timeouts: u64,
    pub keepalive_idle_timeouts: u64,
    pub max_requests_closes: u64,
    pub write_stall_timeouts: u64,
    pub connection_total_timeouts: u64,
    pub graceful_shutdowns: u64,
    pub forced_shutdowns: u64,
    pub listener_errors: u64,
    pub dropped_log_events: u64,
    pub streaming_started: u64,
    pub streaming_completed: u64,
    pub stream_length_mismatches: u64,
    pub stream_producer_errors: u64,
    pub stream_producer_panics: u64,
    pub stream_cancelled: u64,
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
        assert_eq!(sanitize_path("no/slash/here/"), "here");
        assert_eq!(sanitize_path("only-one"), "only-one");
        assert_eq!(sanitize_path("/a/b/c/d/e/f.txt"), "f.txt");
        assert_eq!(sanitize_path("/"), "/");
    }

    #[test]
    fn sanitize_path_truncates_long_paths() {
        let long_name: String = "a".repeat(200);
        let result = sanitize_path(&format!("/prefix/{}", long_name));
        assert!(result.chars().count() <= 128);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn sanitize_path_excludes_del() {
        assert_eq!(sanitize_path("/foo/a\x7Fb"), "ab");
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
        counters.header_timeouts.fetch_add(1, Ordering::Relaxed);
        counters.listener_errors.fetch_add(1, Ordering::Relaxed);

        let snap = counters.snapshot();
        assert_eq!(snap.connections_accepted, 5);
        assert_eq!(snap.header_timeouts, 1);
        assert_eq!(snap.listener_errors, 1);
        assert_eq!(snap.connections_rejected, 0);
        assert_eq!(snap.active_connections, 0);
    }
}

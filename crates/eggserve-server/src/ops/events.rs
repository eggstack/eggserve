//! Ops event schema vocabulary (Plan 215: moved from compatibility core).
//!
//! Owns the single event/field schema authority: [`Severity`],
//! [`EventKind`], [`Field`], [`Event`], sanitization/truncation helpers,
//! JSON rendering, and the schema version. Sink implementations live in
//! [`super::sinks`]; counters live in [`super::counters`]; the runtime
//! authority ([`super::OpsContext`]) lives in the parent module.

use std::borrow::Cow;
use std::fmt;
use std::time::SystemTime;

/// Version of the event schema stamped into every [`Event`].
///
/// Bump this only when the shape or the semantics of an event changes in a
/// way a consumer cannot already handle.
pub const SCHEMA_VERSION: u32 = 1;

/// Severity of an event.
///
/// The ordering is by increasing severity, so a filter can compare with `>=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd)]
pub enum Severity {
    /// Diagnostic detail that is normally filtered out.
    Debug,
    /// Normal operational progress.
    Info,
    /// A recoverable problem that an operator should notice.
    Warn,
    /// A failure that ended an operation.
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

/// The single event-kind vocabulary.
///
/// Every runtime event is labelled with one of these kinds; the kind is a
/// stable, machine-readable classification that sinks and dashboards may
/// switch on. The variants are grouped by the stage of the request lifecycle
/// that produced them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    // Process/Config
    /// The runtime process is starting, before any listener is bound.
    ProcessStarting,
    /// The serving root was resolved and initialized.
    RootInitialized,
    /// A listener is bound and ready to accept connections.
    ListenerReady,
    /// A shutdown was requested by the operator or supervisor.
    ShutdownRequested,
    /// Draining of in-flight connections has begun.
    DrainingStarted,
    /// The graceful drain budget expired; the remaining connections are being
    /// aborted.
    ForcedShutdownStarted,
    /// Shutdown finished; the message carries the terminal result.
    ShutdownComplete,

    // Connection
    /// A connection was accepted and dispatched to a driver.
    ConnectionAccepted,
    /// A connection was refused before dispatch, typically because the
    /// connection limit was saturated.
    ConnectionRejected,
    /// A TLS handshake completed successfully.
    TlsHandshakeSuccess,
    /// A TLS handshake failed.
    TlsHandshakeFailure,
    /// A TLS handshake exceeded its deadline.
    TlsHandshakeTimeout,
    /// The application protocol for a connection was negotiated.
    ProtocolNegotiated,
    /// A connection was closed because the header-read deadline expired.
    HeaderTimeout,
    /// A request body was not consumed before the body-read deadline.
    BodyReadTimeout,
    /// A request was refused by the HTTP parser.
    ParserRejection,
    /// A request was refused because the aggregate post-parse request-header
    /// byte ceiling was exceeded.
    HeaderBytesRejected,
    /// A request was refused because the request target exceeded its ceiling.
    RequestTargetTooLong,
    /// A service invocation was refused because the in-flight request limit
    /// was saturated.
    ServiceAdmissionRejected,
    /// A keep-alive connection was closed.
    KeepAliveClosed,
    /// A keep-alive connection was closed after the idle timeout elapsed.
    KeepAliveIdleTimeout,
    /// A connection was closed after reaching the per-connection request
    /// limit.
    MaxRequestsClose,
    /// A response was closed because no bytes were written within the
    /// write-progress deadline.
    WriteStallTimeout,
    /// A connection was closed because the total connection lifetime expired.
    ConnectionTotalTimeout,
    /// The peer closed the connection.
    ClientDisconnect,
    /// A connection task panicked.
    ConnectionPanic,

    // Request/Service
    /// A request finished and its response was produced.
    RequestCompleted,
    /// The requested path does not exist.
    FileNotFound,
    /// The requested path was denied by the static serving policy.
    FileDenied,
    /// The requested path could not be served because of an I/O error.
    FileError,
    /// The requested path was denied because it names a dotfile.
    DotfileDenied,
    /// The requested path was denied because it traverses or names a symlink.
    SymlinkDenied,
    /// The requested path was denied because it resolved outside the serving
    /// root.
    RootEscapeDenied,
    /// The request body was refused by the body policy.
    BodyPolicyRejection,
    /// The connection was closed because the request body was not fully read.
    IncompleteBodyClose,
    /// The service was not invoked because the request was already answered
    /// or its body was refused.
    ServiceInvocationSuppressed,
    /// The service invocation exceeded its handler deadline.
    ServiceTimeout,
    /// The service returned an error.
    ServiceError,
    /// A directory listing was truncated at the configured entry or byte
    /// budget.
    DirectoryListingLimit,
    // Streaming responses (Plan 162)
    /// A streaming response started producing body chunks.
    ResponseStreamStarted,
    /// A streaming response produced its full body.
    ResponseStreamCompleted,
    /// A streaming response emitted a different number of bytes than it
    /// declared.
    ResponseStreamLengthMismatch,
    /// The streaming producer failed with an error.
    ResponseStreamProducerError,
    /// The streaming producer panicked; the panic was contained at the stream
    /// boundary.
    ResponseStreamProducerPanic,
    /// A streaming response was dropped before completion.
    ResponseStreamCancelled,
    // Deferred request-body ownership + lifecycle (Plan 174)
    /// The request body was handed to the service after the response had
    /// already started.
    DeferredBodyDelegated,
    /// A deferred body reached a complete terminal state.
    DeferredBodyCompleted,
    /// A deferred body was abandoned or failed after the response started.
    DeferredBodyAbandoned,
    /// A deferred body hit the body-read deadline.
    DeferredBodyTimeout,
    /// An in-flight request was cancelled because the peer disconnected.
    RequestLifecyclePeerDisconnect,
    /// An in-flight request was cancelled by the runtime, for example by a
    /// shutdown or a connection timeout.
    RequestLifecycleRuntimeCancel,
    // Trailers + interim responses (Plan 198)
    /// The request's trailer fields were refused.
    RequestTrailerRejected,
    /// The response's trailer fields were suppressed.
    ResponseTrailerSuppressed,
    /// An interim (1xx) response was sent to the client.
    InterimSent,
    /// An interim (1xx) response was refused.
    InterimRejected,
    /// The request's `Expect` header could not be satisfied.
    ExpectationFailed,
    // Generic tunnel / upgrade (Plan 199)
    /// A tunnel handshake was admitted and upgraded.
    TunnelAccepted,
    /// A tunnel handshake was refused, typically because the active-tunnel
    /// limit was saturated.
    TunnelRejected,
    /// A tunnel session ended.
    TunnelClosed,
    /// The tunnel transport upgrade was unavailable or failed.
    TunnelUpgradeFailed,
    // Trusted proxy metadata (Plan 202)
    /// A PROXY protocol preamble was accepted from a trusted peer.
    ProxyProtocolAccepted,
    /// A PROXY protocol preamble was rejected.
    ProxyProtocolRejected,
    /// Forwarded metadata was accepted from a trusted peer.
    ForwardedMetadataAccepted,
    /// Forwarded metadata was rejected.
    ForwardedMetadataRejected,

    // Operational
    /// The listener hit a recoverable accept error.
    ListenerTransientError,
    /// The listener hit an accept error that repeated or could not be
    /// recovered from.
    ListenerPersistentError,
    /// The runtime could not obtain the resources a connection needs.
    ResourceExhaustion,
    /// The blocking-worker pool is saturated.
    BlockingWorkerSaturation,
    /// A log sink failed while handling an event.
    ///
    /// Reserved kind: it is deliberately not emitted today, because
    /// reporting a sink failure through a logger would re-enter the same
    /// failing sink. Sink failures are counted in the `dropped_log_events`
    /// counter instead.
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
            EventKind::ProtocolNegotiated => "protocol_negotiated",
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
            EventKind::DeferredBodyDelegated => "deferred_body_delegated",
            EventKind::DeferredBodyCompleted => "deferred_body_completed",
            EventKind::DeferredBodyAbandoned => "deferred_body_abandoned",
            EventKind::DeferredBodyTimeout => "deferred_body_timeout",
            EventKind::RequestLifecyclePeerDisconnect => "request_lifecycle_peer_disconnect",
            EventKind::RequestLifecycleRuntimeCancel => "request_lifecycle_runtime_cancel",
            EventKind::RequestTrailerRejected => "request_trailer_rejected",
            EventKind::ResponseTrailerSuppressed => "response_trailer_suppressed",
            EventKind::InterimSent => "interim_sent",
            EventKind::InterimRejected => "interim_rejected",
            EventKind::ExpectationFailed => "expectation_failed",
            EventKind::TunnelAccepted => "tunnel_accepted",
            EventKind::TunnelRejected => "tunnel_rejected",
            EventKind::TunnelClosed => "tunnel_closed",
            EventKind::TunnelUpgradeFailed => "tunnel_upgrade_failed",
            EventKind::ProxyProtocolAccepted => "proxy_protocol_accepted",
            EventKind::ProxyProtocolRejected => "proxy_protocol_rejected",
            EventKind::ForwardedMetadataAccepted => "forwarded_metadata_accepted",
            EventKind::ForwardedMetadataRejected => "forwarded_metadata_rejected",

            EventKind::ListenerTransientError => "listener_transient_error",
            EventKind::ListenerPersistentError => "listener_persistent_error",
            EventKind::ResourceExhaustion => "resource_exhaustion",
            EventKind::BlockingWorkerSaturation => "blocking_worker_saturation",
            EventKind::LogSinkFailure => "log_sink_failure",
        };
        write!(f, "{name}")
    }
}

/// One structured key/value field carried by an [`Event`].
///
/// The first member is the field name; the second is its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Field {
    /// A boolean field.
    Bool(String, bool),
    /// A signed integer field.
    I64(String, i64),
    /// An unsigned integer field.
    U64(String, u64),
    /// A string field. Its value is escaped when rendered as JSON.
    Str(String, String),
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Field::Bool(k, v) => write!(f, "\"{k}\": {v}"),
            Field::I64(k, v) => write!(f, "\"{k}\": {v}"),
            Field::U64(k, v) => write!(f, "\"{k}\": {v}"),
            Field::Str(k, v) => write!(f, "\"{}\": \"{}\"", k, escape_json_string(v)),
        }
    }
}

/// One runtime event.
///
/// Build an event with [`Event::new`] and attach correlation data with
/// [`Event::connection_id`], [`Event::request_seq`], and [`Event::field`].
#[derive(Debug, Clone)]
pub struct Event {
    /// Schema version this event was produced under; always
    /// [`SCHEMA_VERSION`] for events built by this crate.
    pub schema_version: u32,
    /// Severity of the event.
    pub severity: Severity,
    /// The machine-readable classification of what happened.
    pub event: EventKind,
    /// UTC timestamp in RFC 3339 form with millisecond precision.
    pub timestamp: String,
    /// Human-readable description of what happened.
    pub message: String,
    /// Correlation ID of the connection this event belongs to, when the
    /// event is connection-scoped.
    pub connection_id: Option<u64>,
    /// Per-connection request sequence number, when the event is
    /// request-scoped.
    pub request_seq: Option<u32>,
    /// Structured key/value fields carrying additional structured detail.
    pub fields: Vec<Field>,
}

impl Event {
    /// Create an event with the current timestamp and no correlation data.
    ///
    /// The schema version is stamped with [`SCHEMA_VERSION`] and the
    /// timestamp is the current UTC time in RFC 3339 form.
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

    /// Append one structured field and return the event.
    pub fn field(mut self, field: Field) -> Self {
        self.fields.push(field);
        self
    }

    /// Set the connection correlation ID and return the event.
    pub fn connection_id(mut self, id: u64) -> Self {
        self.connection_id = Some(id);
        self
    }

    /// Set the per-connection request sequence number and return the event.
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

    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}.{millis:03}Z")
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

/// Reduce free-form text to a bounded, printable field value.
///
/// Every character outside printable ASCII (`0x20..=0x7E`) is dropped, which
/// removes control characters and DEL. The result is truncated to 512
/// characters.
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

/// Reduce a request path to a bounded, printable last-component field value.
///
/// The query string is discarded, only the final path component is kept, and
/// every character outside printable ASCII is dropped. The result is truncated
/// to 127 characters.
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

/// Truncate `text` to at most `max_len` characters.
///
/// The limit counts characters, not bytes. When the text is longer than the
/// limit, the retained prefix is returned with a `…` sentinel appended, so
/// the result is one character longer than `max_len`. Shorter text is
/// borrowed unchanged.
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

/// Render one event as a JSON object.
///
/// The output matches the documented event schema: the schema version,
/// severity, event kind, timestamp, and message are always present, the
/// correlation fields appear only when they are set, and the structured
/// fields are rendered as a `fields` array. String values are escaped.
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
        write!(&mut out, "{cid}").unwrap();
    }

    if let Some(seq) = event.request_seq {
        out.push_str(",\"request_seq\":");
        write!(&mut out, "{seq}").unwrap();
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
                    write!(&mut out, "{v}").unwrap();
                }
                Field::U64(k, v) => {
                    out.push('"');
                    escape_json_string_into(&mut out, k);
                    out.push_str("\":");
                    write!(&mut out, "{v}").unwrap();
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
        let result = sanitize_path(&format!("/prefix/{long_name}"));
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
}

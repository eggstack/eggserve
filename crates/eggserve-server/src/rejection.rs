//! Typed, non-sensitive descriptions and bounded presentation for runtime rejections.

use eggserve_primitives::{HeaderBlock, StatusCode};

/// Category of a rejection selected by the EggServe runtime.
///
/// The kind is a non-sensitive label: it names *why* the runtime refused the
/// request so a presenter can vary the bounded presentation, without exposing
/// request content, transport detail, or error text. The wire status is
/// selected separately by the runtime and passed alongside the kind.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeRejectionKind {
    /// The request target exceeded the configured request-target ceiling
    /// (HTTP 414).
    RequestTargetTooLong,
    /// The request headers exceeded the configured aggregate header-byte
    /// ceiling (HTTP 431).
    RequestHeadersTooLarge,
    /// The request body was refused by the runtime or by the service's body
    /// policy. The status is the one carried by the underlying body error
    /// (commonly `400`).
    RequestBodyRejected,
    /// The request body exceeded the configured body ceiling (HTTP 413).
    RequestBodyTooLarge,
    /// The request body was not consumed before the body-read deadline
    /// (HTTP 408).
    RequestBodyTimeout,
    /// The in-flight service admission gate was saturated (HTTP 503).
    ServiceAdmissionSaturated,
    /// The active-tunnel admission gate was saturated (HTTP 503).
    TunnelAdmissionSaturated,
    /// A service invocation did not settle before the handler deadline
    /// (HTTP 504).
    HandlerTimeout,
    /// A service invocation panicked; the panic was contained at the
    /// runtime boundary (HTTP 500).
    ServicePanic,
    /// A service returned a deliberate rejection with its own status code.
    ServiceRejected,
    /// Reserved for runtime-internal failures with no more specific
    /// category. No producer constructs this variant today.
    Internal,
}

/// Safe runtime rejection facts offered to a presenter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeRejection {
    kind: RuntimeRejectionKind,
    status: StatusCode,
}

impl RuntimeRejection {
    /// Create a rejection. It contains no request, transport, or error text.
    pub const fn new(kind: RuntimeRejectionKind, status: StatusCode) -> Self {
        Self { kind, status }
    }
    /// Rejection category.
    pub const fn kind(&self) -> RuntimeRejectionKind {
        self.kind
    }
    /// Runtime-selected response status.
    pub const fn status(&self) -> StatusCode {
        self.status
    }
}

/// Bounded application-facing response presentation. EggServe retains status,
/// framing, privacy, and connection-lifecycle authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeErrorPresentation {
    /// Application headers; framing, hop-by-hop, `Date`, and `Server` fields are ignored.
    pub headers: HeaderBlock,
    /// Response payload, capped at 64 KiB by the runtime.
    pub body: Vec<u8>,
}

/// Synchronous presenter for EggServe-selected runtime rejections.
pub trait RuntimeRejectionPresenter: std::fmt::Debug + Send + Sync + 'static {
    /// Return safe body/application-header presentation, or `None` to use the default.
    fn present(&self, rejection: &RuntimeRejection) -> Option<RuntimeErrorPresentation>;
}

/// Maximum custom rejection body accepted from a presenter.
pub const MAX_RUNTIME_REJECTION_BODY_BYTES: usize = 64 * 1024;

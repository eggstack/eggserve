//! Typed, non-sensitive descriptions and bounded presentation for runtime rejections.

use eggserve_primitives::{HeaderBlock, StatusCode};

/// Category of a rejection selected by the EggServe runtime.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeRejectionKind {
    RequestTargetTooLong,
    RequestHeadersTooLarge,
    RequestBodyRejected,
    RequestBodyTooLarge,
    RequestBodyTimeout,
    ServiceAdmissionSaturated,
    TunnelAdmissionSaturated,
    HandlerTimeout,
    ServicePanic,
    ServiceRejected,
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

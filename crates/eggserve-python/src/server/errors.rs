//! Python exception mapping (Plan 206 Track B).
//!
//! Centralizes `ServerRequestError`/`RawBodyError` to `PyErr` conversion.
//! Single owner for all Python exception mapping; sync/async bridges share
//! helpers here with no duplication.

#![allow(unused_imports)]
use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyIterator};
use tokio::sync::mpsc;
use tokio::sync::Semaphore;

use bytes::Bytes;
use eggserve_core::policy;
use eggserve_core::primitives::body::BodySource;
use eggserve_core::primitives::canonical::{
    normalize_response, NormalizeRequest, Response as CanonicalResponse, ResponseBody,
    ResponseStream, ResponseStreamError, StatusCode as CanonicalStatusCode,
};
use eggserve_core::primitives::header_block::{HeaderName, HeaderValue};
use eggserve_core::primitives::http::ReadOnlyMethod;
use eggserve_core::primitives::request_body::RequestBody;
use eggserve_core::primitives::request_body_error::RequestBodyError as RustBodyError;
use eggserve_core::primitives::request_body_policy::RequestBodyPolicy;
use eggserve_core::primitives::request_context::RequestContext;
use eggserve_core::primitives::request_head::RequestHead;
use eggserve_core::primitives::{
    resolve_and_plan, ConfinedPath, PathDotfilePolicy, PathPolicy, PathRejection,
    ResolveAndPlanError, SecureRoot, StaticPolicy,
};
use eggserve_core::server::config::RuntimeConfig;
use eggserve_core::server::errors::ShutdownResult;
use eggserve_core::server::lifecycle::LifecycleState;
use eggserve_core::server::service::{Service, ServiceError};
use eggserve_core::server::{Server, ServerHandle};

use super::*;

// ---------------------------------------------------------------------------
#[pyclass(frozen, name = "ServerRequestError")]
#[derive(Debug)]
pub enum ServerRequestError {
    MethodNotAllowed { allowed: String },
    TargetInvalid { reason: String },
    PathRejected { reason: String },
    BodyNotAllowed(),
}

impl std::fmt::Display for ServerRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MethodNotAllowed { allowed } => write!(f, "Method not allowed; use {allowed}"),
            Self::TargetInvalid { reason } => write!(f, "Invalid request target: {reason}"),
            Self::PathRejected { reason } => write!(f, "Path rejected: {reason}"),
            Self::BodyNotAllowed() => write!(f, "Request body not allowed"),
        }
    }
}

impl std::error::Error for ServerRequestError {}

impl ServerRequestError {
    pub(super) fn into_py_err(self) -> PyErr {
        pyo3::exceptions::PyValueError::new_err(self.to_string())
    }
}

// ---------------------------------------------------------------------------
// Raw body error for channel communication (no Python objects)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub(super) enum RawBodyError {
    RejectedByPolicy,
    DeclaredLengthTooLarge {
        declared: u64,
        limit: u64,
    },
    LimitExceeded {
        limit: u64,
        received: u64,
    },
    ReadTimeout,
    PrematureEof {
        received: u64,
        expected: Option<u64>,
    },
    LengthMismatch {
        declared: u64,
        actual: u64,
    },
    InvalidChunkFraming(String),
    Cancelled,
    Disconnected,
    AlreadyConsumed,
    MixedConsumptionMode,
    Transport(String),
}

impl From<RustBodyError> for RawBodyError {
    fn from(err: RustBodyError) -> Self {
        match err {
            RustBodyError::RejectedByPolicy => Self::RejectedByPolicy,
            RustBodyError::DeclaredLengthTooLarge { declared, limit } => {
                Self::DeclaredLengthTooLarge { declared, limit }
            }
            RustBodyError::LimitExceeded { limit, received } => {
                Self::LimitExceeded { limit, received }
            }
            RustBodyError::ReadTimeout => Self::ReadTimeout,
            RustBodyError::PrematureEof { received, expected } => {
                Self::PrematureEof { received, expected }
            }
            RustBodyError::LengthMismatch { declared, actual } => {
                Self::LengthMismatch { declared, actual }
            }
            RustBodyError::InvalidChunkFraming(msg) => Self::InvalidChunkFraming(msg),
            RustBodyError::Cancelled => Self::Cancelled,
            RustBodyError::Disconnected => Self::Disconnected,
            RustBodyError::AlreadyConsumed => Self::AlreadyConsumed,
            RustBodyError::MixedConsumptionMode => Self::MixedConsumptionMode,
            RustBodyError::Transport(msg) => Self::Transport(msg),
            // Plan 198: trailer failures never reach the synchronous facade as
            // trailers (facade unchanged); map to sanitized transport failure.
            RustBodyError::InvalidTrailers(msg) => Self::Transport(msg),
            RustBodyError::TrailersNotReady => Self::AlreadyConsumed,
            // Plan 197: `RequestBodyError` is `#[non_exhaustive]`; future
            // categories map to a sanitized transport failure (500) without
            // leaking variant detail.
            _ => Self::Transport("request body failed".to_owned()),
        }
    }
}

pub(super) fn raw_body_error_to_pyerr(err: RawBodyError) -> PyErr {
    match err {
        RawBodyError::RejectedByPolicy => {
            crate::RequestBodyRejectedError::new_err("request body rejected by policy")
        }
        RawBodyError::DeclaredLengthTooLarge { declared, limit } => {
            crate::RequestBodyTooLargeError::new_err(format!(
                "declared content-length {declared} exceeds limit {limit}"
            ))
        }
        RawBodyError::LimitExceeded { limit, received } => {
            crate::RequestBodyTooLargeError::new_err(format!(
                "body exceeded limit: received {received} bytes, limit is {limit}"
            ))
        }
        RawBodyError::ReadTimeout => crate::RequestBodyTimeoutError::new_err("body read timed out"),
        RawBodyError::PrematureEof { received, expected } => {
            let msg = match expected {
                Some(exp) => {
                    format!("premature EOF: received {received} of {exp} expected bytes")
                }
                None => format!("premature EOF after {received} bytes"),
            };
            crate::RequestBodyDisconnectedError::new_err(msg)
        }
        RawBodyError::Disconnected => {
            crate::RequestBodyDisconnectedError::new_err("client disconnected")
        }
        RawBodyError::AlreadyConsumed => {
            crate::RequestBodyConsumedError::new_err("body already consumed")
        }
        RawBodyError::MixedConsumptionMode => crate::RequestBodyConsumedError::new_err(
            "mixed consumption mode: cannot switch between read_all and streaming",
        ),
        RawBodyError::Cancelled => {
            crate::RequestBodyCancelledError::new_err("body consumption cancelled")
        }
        RawBodyError::LengthMismatch { declared, actual } => crate::RequestBodyError::new_err(
            format!("body length mismatch: declared {declared}, actual {actual}"),
        ),
        RawBodyError::InvalidChunkFraming(msg) => {
            crate::RequestBodyError::new_err(format!("invalid chunk framing: {msg}"))
        }
        RawBodyError::Transport(msg) => {
            crate::RequestBodyDisconnectedError::new_err(format!("transport error: {msg}"))
        }
    }
}

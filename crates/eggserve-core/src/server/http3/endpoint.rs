//! H3 endpoint/listener lifecycle (Plan 206 Track F).
//!
//! Owns connection-guard (`ActiveConnectionGuard`) and close-reason
//! mapping (`h3_connection_close_reason`, peer/shutdown cancellation via
//! the shared lifecycle registry). Listener startup stays in the parent.

#![allow(unused_imports)]
use std::sync::Arc;

use bytes::{Buf, Bytes};
use futures_util::{stream, StreamExt};
use tokio::sync::{broadcast, OwnedSemaphorePermit, Semaphore};

use crate::primitives::canonical::{normalize_response, NormalizeRequest, Response, ResponseBody};
use crate::primitives::connection_info::TlsInfo;
use crate::primitives::header_block::{HeaderBlock, HeaderName, HeaderValue};
use crate::primitives::method::Method;
use crate::primitives::request::Request;
use crate::primitives::request_body::IncomingError;
use crate::primitives::request_head::RequestHead;
use crate::primitives::request_lifecycle::{RequestCancellationReason, RequestShared};
use crate::primitives::request_target::RequestTarget;
use crate::primitives::version::HttpVersion;
use crate::server::config::RuntimeConfig;
use crate::server::connection::lifecycle::{cancel_shared_with_observability, ConnectionRequests};
use crate::server::connection::ConnectionContext;
use crate::server::errors::ShutdownResult;
use crate::server::service::{Service, ServiceError};
use crate::server::RuntimeState;

pub(super) struct ActiveConnectionGuard {
    pub(super) ops: crate::ops::OpsContext,
}

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        self.ops
            .counters()
            .active_connections
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

pub(super) fn h3_connection_close_reason(
    error: &quinn::ConnectionError,
) -> RequestCancellationReason {
    match error {
        quinn::ConnectionError::ApplicationClosed(_)
        | quinn::ConnectionError::ConnectionClosed(_)
        | quinn::ConnectionError::Reset => RequestCancellationReason::PeerDisconnected,
        quinn::ConnectionError::TimedOut => RequestCancellationReason::ConnectionTimeout,
        quinn::ConnectionError::LocallyClosed => RequestCancellationReason::ServerShutdown,
        _ => RequestCancellationReason::TransportFailure,
    }
}

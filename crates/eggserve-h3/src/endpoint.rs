//! H3 endpoint/listener lifecycle (Plan 206 Track F).
//!
//! Owns connection-guard (`ActiveConnectionGuard`) and close-reason
//! mapping (`h3_connection_close_reason`, peer/shutdown cancellation via
//! the shared lifecycle registry). Listener startup stays in the parent.

#![allow(unused_imports)]
use std::sync::Arc;

use crate::quinn;

use bytes::{Buf, Bytes};
use futures_util::{stream, StreamExt};
use tokio::sync::{broadcast, OwnedSemaphorePermit, Semaphore};

use eggserve_primitives::canonical::{
    normalize_response, NormalizeRequest, Response, ResponseBody,
};
use eggserve_primitives::connection_info::TlsInfo;
use eggserve_primitives::header_block::{HeaderBlock, HeaderName, HeaderValue};
use eggserve_primitives::method::Method;
use eggserve_primitives::request::Request;
use eggserve_primitives::request_body::IncomingError;
use eggserve_primitives::request_head::RequestHead;
use eggserve_primitives::request_lifecycle::{RequestCancellationReason, RequestShared};
use eggserve_primitives::request_target::RequestTarget;
use eggserve_primitives::version::HttpVersion;
use eggserve_server::config::RuntimeConfig;
use eggserve_server::connection::ConnectionContext;
use eggserve_server::connection::{cancel_shared_with_observability, ConnectionRequests};
use eggserve_server::errors::ShutdownResult;
use eggserve_server::runtime::RuntimeState;
use eggserve_server::service::{Service, ServiceError};

pub(crate) struct ActiveConnectionGuard {
    pub(crate) ops: eggserve_server::ops::OpsContext,
}

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        self.ops
            .counters()
            .active_connections
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

pub(crate) fn h3_connection_close_reason(
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

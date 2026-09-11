//! H3 tunnel/Extended CONNECT adapter (Plan 206 Track F).
//!
//! Owns tunnel kind naming (`kind_string`) and the active-tunnel guard
//! (`H3ActiveTunnelGuard`). Handshake validation stays canonical via
//! `TunnelCapability`; runtime owns framing.

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

pub(super) type H3Bytes = Bytes;

pub(super) fn kind_string(kind: crate::primitives::tunnel::TunnelKind) -> &'static str {
    match kind {
        crate::primitives::tunnel::TunnelKind::Http1Upgrade => "http1-upgrade",
        crate::primitives::tunnel::TunnelKind::Connect => "connect",
        crate::primitives::tunnel::TunnelKind::ExtendedConnect => "extended-connect",
    }
}

pub(super) struct H3ActiveTunnelGuard {
    pub(super) ops: crate::ops::OpsContext,
}

impl Drop for H3ActiveTunnelGuard {
    fn drop(&mut self) {
        self.ops
            .counters()
            .active_tunnels
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Send an H3 tunnel `200` handshake (headers only, no body, no FIN).
///
/// Applies canonical privacy (Server/Date/denylist) like ordinary responses
/// but never invents `Content-Length`/`Transfer-Encoding` and never finishes
/// the stream (duplex continues). Hop-by-hop already stripped in `accept`.
pub(super) async fn send_h3_tunnel_handshake<S>(
    stream: &mut h3::server::RequestStream<S, H3Bytes>,
    response: Response,
    config: &RuntimeConfig,
) -> Result<(), String>
where
    S: h3::quic::SendStream<H3Bytes>,
{
    // Start from accepted handshake headers (validated/bounded, no framing,
    // no hop-by-hop), then apply canonical privacy (Server/Date/denylist).
    let mut builder_block = HeaderBlock::new();
    for field in response.headers().iter() {
        // Defense in depth: strip framing/hop-by-hop even though `accept`
        // already did (`head_mut` clears tunnel, so this is unchanged, but
        // re-validate before wire).
        if field.name.as_str().eq_ignore_ascii_case("content-length")
            || field
                .name
                .as_str()
                .eq_ignore_ascii_case("transfer-encoding")
            || crate::primitives::canonical::is_hop_by_hop_header(field.name.as_str())
        {
            continue;
        }
        builder_block.push(field.name.clone(), field.value.clone());
    }
    let mut tmp = Response::builder()
        .status(response.status())
        .body(ResponseBody::Empty)
        .map_err(|e| e.to_string())?;
    for field in builder_block.iter() {
        // `head_mut` clears tunnel acceptance, but `tmp` has none (fresh),
        // so safe: we are building a wire head, not mutating the handshake.
        tmp.head_mut()
            .headers_mut()
            .push(field.name.clone(), field.value.clone());
    }
    let tmp = crate::server::connection::response::finalize_canonical_response(tmp, config);
    let status = hyper::StatusCode::from_u16(tmp.status().as_u16()).map_err(|e| e.to_string())?;
    // Ensure 200 (not 101) for H3 Extended/CONNECT; reject 101 defensively.
    if status == hyper::StatusCode::SWITCHING_PROTOCOLS {
        return Err("H3 tunnel must not synthesize 101".to_string());
    }
    let mut headers = hyper::HeaderMap::new();
    for field in tmp.headers().iter() {
        let name = hyper::header::HeaderName::from_bytes(field.name.as_str().as_bytes())
            .map_err(|e| e.to_string())?;
        let value = hyper::header::HeaderValue::from_bytes(field.value.as_bytes())
            .map_err(|e| e.to_string())?;
        headers.append(name, value);
    }
    let mut head = hyper::Response::builder()
        .status(status)
        .body(())
        .map_err(|e| e.to_string())?;
    *head.headers_mut() = headers;
    tokio::time::timeout(config.response_write_timeout, stream.send_response(head))
        .await
        .map_err(|_| "response write timeout".to_string())?
        .map_err(|e| e.to_string())
}

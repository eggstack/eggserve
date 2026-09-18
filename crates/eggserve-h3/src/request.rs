//! H3 request conversion (Plan 206 Track F).
//!
//! Owns request metadata conversion (`convert_request_head`), declared
//! length probing (`declared_content_length`), and field receive
//! (`h3_trailers_to_block`). Single service invocation kernel and
//! canonical normalization stay shared (no H3-specific semantics).

#![allow(unused_imports)]
use std::sync::Arc;

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

pub(crate) fn h3_trailers_to_block(map: &hyper::HeaderMap) -> Result<HeaderBlock, String> {
    // Canonical header validation only; trailer denylist/limits enforced once
    // in `RequestBody` via `validate_trailers` (no second H3 policy).
    let mut block = HeaderBlock::new();
    for (name, value) in map.iter() {
        let name =
            HeaderName::new(name.as_str()).map_err(|_| "invalid trailer name".to_string())?;
        let value = HeaderValue::from_bytes(value.as_bytes())
            .map_err(|_| "invalid trailer value".to_string())?;
        block.push(name, value);
    }
    Ok(block)
}

pub(crate) fn declared_content_length(
    request: &hyper::Request<()>,
) -> Result<Option<u64>, ServiceError> {
    let values = request
        .headers()
        .get_all(hyper::header::CONTENT_LENGTH)
        .iter()
        .collect::<Vec<_>>();
    if values.len() > 1 {
        return Err(ServiceError::rejected(
            400,
            "duplicate Content-Length headers",
        ));
    }
    values
        .first()
        .map(|value| {
            value
                .to_str()
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or_else(|| ServiceError::rejected(400, "invalid Content-Length header"))
        })
        .transpose()
}

pub(crate) fn convert_request_head(
    request: &hyper::Request<()>,
    config: &RuntimeConfig,
    _conn_id: u64,
    ops: &eggserve_server::ops::OpsContext,
) -> Result<RequestHead, ServiceError> {
    let method = Method::new(request.method().as_str())
        .map_err(|_| ServiceError::rejected(400, "invalid method"))?;
    let uri = request.uri();
    if !uri
        .scheme_str()
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https"))
    {
        return Err(ServiceError::rejected(400, "HTTP/3 requires https scheme"));
    }
    let raw_target = uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    if raw_target.len() > config.max_request_target_bytes {
        ops.counters()
            .request_target_rejected
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        return Err(ServiceError::rejected(414, "request target too long"));
    }
    let target = RequestTarget::parse(raw_target)
        .map_err(|_| ServiceError::rejected(400, "invalid request target"))?;
    let mut headers = HeaderBlock::with_capacity(request.headers().len());
    let mut header_bytes = 0usize;
    if request.headers().len() > config.max_headers {
        return Err(ServiceError::rejected(431, "too many request headers"));
    }
    for (name, value) in request.headers() {
        if matches!(
            name.as_str().to_ascii_lowercase().as_str(),
            "connection" | "keep-alive" | "proxy-connection" | "transfer-encoding" | "upgrade"
        ) {
            return Err(ServiceError::rejected(
                400,
                "forbidden HTTP/3 connection header",
            ));
        }
        if name.as_str().eq_ignore_ascii_case("te")
            && !value.as_bytes().eq_ignore_ascii_case(b"trailers")
        {
            return Err(ServiceError::rejected(400, "invalid HTTP/3 TE header"));
        }
        header_bytes = header_bytes
            .saturating_add(name.as_str().len())
            .saturating_add(value.len());
        if header_bytes > config.max_header_bytes {
            return Err(ServiceError::rejected(431, "request headers too large"));
        }
        let name = HeaderName::new(name.as_str())
            .map_err(|_| ServiceError::rejected(400, "invalid header name"))?;
        let value = HeaderValue::from_bytes(value.as_bytes())
            .map_err(|_| ServiceError::rejected(400, "invalid header value"))?;
        headers.push(name, value);
    }
    let authority = uri
        .authority()
        .map(|value| eggserve_primitives::authority::Authority::parse(value.as_str()))
        .transpose()
        .map_err(|_| ServiceError::rejected(400, "invalid authority"))?;
    Ok(RequestHead::new_with_authority(
        method,
        target,
        HttpVersion::Http3,
        headers,
        authority,
    ))
}

pub(crate) async fn invoke_service<S: Service>(
    service: Arc<S>,
    request: Request,
    _permit: OwnedSemaphorePermit,
    config: &RuntimeConfig,
    ops: &eggserve_server::ops::OpsContext,
) -> Result<Response, ServiceError> {
    eggserve_server::connection::invoke_canonical_service(
        service.as_ref(),
        request,
        config.handler_timeout,
        ops,
    )
    .await
}

//! Deprecated compatibility adapter for the pre-runtime static API.
//!
//! Static resolution, directory listing, response planning, and canonical
//! normalization live exclusively in [`crate::server::StaticService`]. This
//! module remains only so alpha consumers of `handle_request` can migrate
//! without retaining a second implementation.

use std::convert::Infallible;
use std::sync::Arc;

use http_body_util::BodyExt;
use hyper::{Request as HyperRequest, Response, StatusCode};

use crate::config::ServeState;
use crate::primitives::connection_info::{ConnectionInfo, Scheme, TlsInfo};
use crate::primitives::header_block::HeaderBlock;
use crate::primitives::method::Method;
use crate::primitives::request::Request;
use crate::primitives::request_body::RequestBody;
use crate::primitives::request_target::RequestTarget;
use crate::primitives::version::HttpVersion;
use crate::response::BoxBodyInner;
use crate::server::service::Service;
use crate::server::StaticService;

/// Handle a request through the authoritative [`StaticService`] planner.
///
/// This compatibility entry point is deprecated in favor of
/// `Server::start()` and does not define a separate static-serving path.
pub async fn handle_request<B>(req: HyperRequest<B>, state: &ServeState) -> Response<BoxBodyInner> {
    handle_request_with_metadata(
        req,
        state,
        "127.0.0.1:0".parse().expect("valid loopback address"),
        "127.0.0.1:0".parse().expect("valid loopback address"),
        None,
    )
    .await
}

/// Metadata-aware form of [`handle_request`].
pub async fn handle_request_with_metadata<B>(
    req: HyperRequest<B>,
    state: &ServeState,
    local_addr: std::net::SocketAddr,
    remote_addr: std::net::SocketAddr,
    tls_info: Option<TlsInfo>,
) -> Response<BoxBodyInner> {
    let is_head = req.method() == hyper::Method::HEAD;
    if req.method() == hyper::Method::GET || req.method() == hyper::Method::HEAD {
        if let Err(response) = validate_body_headers(&req, state) {
            return response;
        }
    }

    let method = match Method::new(req.method().as_str()) {
        Ok(method) => method,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, is_head),
    };
    let target = if method.is_get() || method.is_head() {
        if req.uri().scheme_str().is_some() || req.uri().authority().is_some() {
            return error_response(StatusCode::BAD_REQUEST, is_head);
        }
        match req.uri().path_and_query() {
            Some(value) => match RequestTarget::parse(value.as_str()) {
                Ok(target) => target,
                Err(_) => return error_response(StatusCode::BAD_REQUEST, is_head),
            },
            None => return error_response(StatusCode::BAD_REQUEST, is_head),
        }
    } else {
        // StaticService rejects unsupported methods before it resolves the
        // target. Preserve that behavior for authority and asterisk forms.
        RequestTarget::parse("/").expect("root target is valid")
    };
    let version = match req.version() {
        hyper::Version::HTTP_10 => HttpVersion::Http10,
        hyper::Version::HTTP_11 => HttpVersion::Http11,
        _ => return error_response(StatusCode::HTTP_VERSION_NOT_SUPPORTED, is_head),
    };
    let mut headers = HeaderBlock::new();
    for (name, value) in req.headers() {
        let value = match value.to_str() {
            Ok(value) => value,
            Err(_) => return error_response(StatusCode::BAD_REQUEST, is_head),
        };
        if headers.push_str(name.as_str(), value).is_err() {
            return error_response(StatusCode::BAD_REQUEST, is_head);
        }
    }

    let request = Request::new(
        crate::primitives::request_head::RequestHead::new(method, target, version, headers),
        RequestBody::empty(),
        ConnectionInfo {
            local_addr,
            remote_addr,
            scheme: if tls_info.is_some() {
                Scheme::Https
            } else {
                Scheme::Http
            },
            tls: tls_info,
        },
    );
    let service = StaticService::from_state(Arc::new(state.clone()));
    let response = match service.call(request).await {
        Ok(response) => response,
        Err(_) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, is_head),
    };
    match crate::primitives::canonical::to_hyper_response_with_file_stream_semaphore(
        response,
        state.file_stream_semaphore(),
    ) {
        Ok(response) => response,
        Err(crate::primitives::canonical::ResponseConstructionError::FileStreamLimit) => {
            error_response(StatusCode::SERVICE_UNAVAILABLE, false)
        }
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, is_head),
    }
}

#[allow(clippy::result_large_err)]
fn validate_body_headers<B>(
    req: &HyperRequest<B>,
    state: &ServeState,
) -> Result<(), Response<BoxBodyInner>> {
    let is_head = req.method() == hyper::Method::HEAD;
    let content_length = match req.headers().get(hyper::header::CONTENT_LENGTH) {
        Some(value) => match value
            .to_str()
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
        {
            Some(length) => Some(length),
            None => return Err(error_response(StatusCode::BAD_REQUEST, is_head)),
        },
        None => None,
    };
    if req.headers().contains_key(hyper::header::TRANSFER_ENCODING) {
        return Err(error_response(StatusCode::BAD_REQUEST, is_head));
    }
    if content_length.is_some_and(|length| length > state.config.limits.max_request_body_bytes) {
        return Err(error_response(StatusCode::PAYLOAD_TOO_LARGE, is_head));
    }
    Ok(())
}

fn error_response(status: StatusCode, is_head: bool) -> Response<BoxBodyInner> {
    let body = if is_head || status == StatusCode::NOT_MODIFIED {
        http_body_util::Empty::new()
            .map_err(|never: Infallible| match never {})
            .boxed()
    } else {
        let bytes = format!(
            "{} {}\n",
            status.as_u16(),
            status.canonical_reason().unwrap_or("Error")
        );
        http_body_util::Full::new(bytes::Bytes::from(bytes))
            .map_err(|never: Infallible| match never {})
            .boxed()
    };
    let mut builder = Response::builder()
        .status(status)
        .header(hyper::header::CONTENT_TYPE, "text/plain; charset=utf-8");
    if status == StatusCode::METHOD_NOT_ALLOWED {
        builder = builder.header(hyper::header::ALLOW, "GET, HEAD");
    }
    builder
        .body(body)
        .expect("static compatibility response has valid headers")
}

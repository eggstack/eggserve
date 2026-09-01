//! Final Hyper response helpers owned by the runtime boundary.

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::{Response, StatusCode};
use std::time::SystemTime;

pub type BoxBodyInner = BoxBody<Bytes, std::io::Error>;

/// Add the origin server's single authoritative Date header.
pub(crate) fn finalize_origin_headers(response: &mut Response<BoxBodyInner>, now: SystemTime) {
    response.headers_mut().remove(hyper::header::DATE);
    if let Ok(value) = hyper::header::HeaderValue::from_str(&httpdate::fmt_http_date(now)) {
        response.headers_mut().insert(hyper::header::DATE, value);
    }
}

fn finalize(mut response: Response<BoxBodyInner>) -> Response<BoxBodyInner> {
    finalize_origin_headers(&mut response, SystemTime::now());
    response
}

pub(crate) fn canonical_error(
    status: StatusCode,
    body: &'static str,
    is_head: bool,
) -> Response<BoxBodyInner> {
    let code = crate::primitives::canonical::StatusCode::new(status.as_u16())
        .unwrap_or(crate::primitives::canonical::StatusCode::INTERNAL_SERVER_ERROR);
    let mut headers = crate::primitives::header_block::HeaderBlock::new();
    // These headers are static, valid HTTP metadata; failure would indicate
    // an implementation change rather than a runtime input problem.
    headers
        .push_str("content-type", "text/plain; charset=utf-8")
        .expect("canonical error content type is valid");
    if status == StatusCode::METHOD_NOT_ALLOWED {
        headers
            .push_str("allow", "GET, HEAD")
            .expect("canonical error Allow value is valid");
    }
    crate::primitives::canonical::normalize_metadata(code, &mut headers, body.len() as u64)
        // All current canonical error statuses permit a payload and therefore
        // cannot trigger the normalizer's body-forbidden metadata error.
        .expect("canonical error metadata is valid");
    let mut builder = Response::builder().status(status);
    for field in headers.iter() {
        builder = builder.header(field.name.as_str(), field.value.as_str());
    }
    let body = if is_head { "" } else { body };
    finalize(
        builder
            .body(full_body(body))
            .expect("canonical error response headers and body are valid"),
    )
}

pub fn bad_request(is_head: bool) -> Response<BoxBodyInner> {
    canonical_error(StatusCode::BAD_REQUEST, "400 Bad Request\n", is_head)
}

pub fn payload_too_large(is_head: bool) -> Response<BoxBodyInner> {
    canonical_error(
        StatusCode::PAYLOAD_TOO_LARGE,
        "413 Payload Too Large\n",
        is_head,
    )
}

pub fn internal_error() -> Response<BoxBodyInner> {
    canonical_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "500 Internal Server Error\n",
        false,
    )
}

pub fn service_unavailable() -> Response<BoxBodyInner> {
    canonical_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "503 Service Unavailable\n",
        false,
    )
}

#[cfg(test)]
pub fn not_found(is_head: bool) -> Response<BoxBodyInner> {
    canonical_error(StatusCode::NOT_FOUND, "404 Not Found\n", is_head)
}

fn full_body(s: &str) -> BoxBodyInner {
    Full::new(Bytes::copy_from_slice(s.as_bytes()))
        .map_err(|never| match never {})
        .boxed()
}

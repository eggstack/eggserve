//! Final Hyper response helpers owned by the runtime boundary.

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::{BodyExt, Full};
use hyper::{Response, StatusCode};
use std::time::SystemTime;

pub type BoxBodyInner = BoxBody<Bytes, std::io::Error>;

/// Add the origin server's single authoritative Date header.
///
/// This is the direct-primitives default (system clock). The server
/// connection pipeline overrides per [`crate::server::response_policy::DatePolicy`]
/// in `finalize_runtime_response`, which is the sole Date authority when a
/// `RuntimeConfig` is present (Hyper automatic Date is disabled).
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

#[allow(dead_code)]
pub(crate) fn canonical_error(
    status: StatusCode,
    body: &'static str,
    is_head: bool,
) -> Response<BoxBodyInner> {
    canonical_error_with_policy(
        status,
        body,
        is_head,
        crate::policy::ErrorRepresentationPolicy::Minimal,
    )
}

/// Canonical error with an explicit representation policy.
///
/// `Minimal` emits the fixed generic plain-text body; `Empty` emits no body
/// bytes (`Content-Length: 0`, no `Content-Type`) for runtime-generated
/// errors. `Allow` for 405 is retained under both variants. `HEAD`
/// suppression remains correct.
pub(crate) fn canonical_error_with_policy(
    status: StatusCode,
    body: &'static str,
    is_head: bool,
    policy: crate::policy::ErrorRepresentationPolicy,
) -> Response<BoxBodyInner> {
    let code = crate::primitives::canonical::StatusCode::new(status.as_u16())
        .unwrap_or(crate::primitives::canonical::StatusCode::INTERNAL_SERVER_ERROR);
    let effective_body: &str = match policy {
        crate::policy::ErrorRepresentationPolicy::Minimal => {
            if is_head {
                ""
            } else {
                body
            }
        }
        crate::policy::ErrorRepresentationPolicy::Empty => "",
    };
    let mut headers = crate::primitives::header_block::HeaderBlock::new();
    // These headers are static, valid HTTP metadata; failure would indicate
    // an implementation change rather than a runtime input problem.
    // `Empty` omits Content-Type (no body emitted); `Allow` is retained.
    if policy == crate::policy::ErrorRepresentationPolicy::Minimal {
        headers
            .push_str("content-type", "text/plain; charset=utf-8")
            .expect("canonical error content type is valid");
    }
    if status == StatusCode::METHOD_NOT_ALLOWED {
        headers
            .push_str("allow", "GET, HEAD")
            .expect("canonical error Allow value is valid");
    }
    crate::primitives::canonical::normalize_metadata(
        code,
        &mut headers,
        effective_body.len() as u64,
    )
    // All current canonical error statuses permit a payload and therefore
    // cannot trigger the normalizer's body-forbidden metadata error.
    .expect("canonical error metadata is valid");
    let mut builder = Response::builder().status(status);
    for field in headers.iter() {
        builder = builder.header(field.name.as_str(), field.value.as_str());
    }
    finalize(
        builder
            .body(full_body(effective_body))
            .expect("canonical error response headers and body are valid"),
    )
}

#[allow(dead_code)]
pub fn bad_request(is_head: bool) -> Response<BoxBodyInner> {
    canonical_error(StatusCode::BAD_REQUEST, "400 Bad Request\n", is_head)
}

pub fn bad_request_with_policy(
    is_head: bool,
    policy: crate::policy::ErrorRepresentationPolicy,
) -> Response<BoxBodyInner> {
    canonical_error_with_policy(
        StatusCode::BAD_REQUEST,
        "400 Bad Request\n",
        is_head,
        policy,
    )
}

#[allow(dead_code)]
pub fn payload_too_large(is_head: bool) -> Response<BoxBodyInner> {
    canonical_error(
        StatusCode::PAYLOAD_TOO_LARGE,
        "413 Payload Too Large\n",
        is_head,
    )
}

pub fn payload_too_large_with_policy(
    is_head: bool,
    policy: crate::policy::ErrorRepresentationPolicy,
) -> Response<BoxBodyInner> {
    canonical_error_with_policy(
        StatusCode::PAYLOAD_TOO_LARGE,
        "413 Payload Too Large\n",
        is_head,
        policy,
    )
}

#[allow(dead_code)]
pub fn internal_error() -> Response<BoxBodyInner> {
    canonical_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "500 Internal Server Error\n",
        false,
    )
}

pub fn internal_error_with_policy(
    policy: crate::policy::ErrorRepresentationPolicy,
) -> Response<BoxBodyInner> {
    canonical_error_with_policy(
        StatusCode::INTERNAL_SERVER_ERROR,
        "500 Internal Server Error\n",
        false,
        policy,
    )
}

#[allow(dead_code)]
pub fn service_unavailable() -> Response<BoxBodyInner> {
    canonical_error(
        StatusCode::SERVICE_UNAVAILABLE,
        "503 Service Unavailable\n",
        false,
    )
}

pub fn service_unavailable_with_policy(
    policy: crate::policy::ErrorRepresentationPolicy,
) -> Response<BoxBodyInner> {
    canonical_error_with_policy(
        StatusCode::SERVICE_UNAVAILABLE,
        "503 Service Unavailable\n",
        false,
        policy,
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

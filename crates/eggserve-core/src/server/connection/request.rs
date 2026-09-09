//! Canonical request validation: target/header ceilings, framing checks,
//! body-policy selection, and Hyper body bridging.
//!
//! Single request-validation source of truth used by the canonical pipeline.
//! Hyper enforces `max_buf_size`/`max_headers` during parsing (431 from Hyper
//! itself); aggregate header bytes (431) and request-target length (414) are
//! bounded here before any service work. No Hyper types escape: the bridge
//! converts into canonical `RequestHead` / `RequestBody`.

use std::sync::atomic::Ordering;

use hyper::body::Incoming;
use hyper::Request;

use crate::primitives::request_body_policy::RequestBodyPolicy;
use crate::server::service::ServiceError;

/// Select the effective body policy from service preference and runtime ceiling.
pub(crate) fn select_body_policy(
    service_policy: RequestBodyPolicy,
    max_body_bytes: u64,
) -> RequestBodyPolicy {
    match service_policy {
        RequestBodyPolicy::Reject => RequestBodyPolicy::Reject,
        RequestBodyPolicy::Buffer { max_bytes } => {
            let effective = max_bytes.min(max_body_bytes);
            if effective == 0 {
                RequestBodyPolicy::Reject
            } else {
                RequestBodyPolicy::Buffer {
                    max_bytes: effective,
                }
            }
        }
        RequestBodyPolicy::Stream { max_bytes } => {
            let effective = max_bytes.min(max_body_bytes);
            if effective == 0 {
                RequestBodyPolicy::Reject
            } else {
                RequestBodyPolicy::Stream {
                    max_bytes: effective,
                }
            }
        }
    }
}

/// Validate body framing for ALL methods.
///
/// Rejects requests with duplicate Content-Length fields and TE+CL
/// conflicts where both headers are visible. Duplicate Content-Length values
/// that disagree are the request-smuggling vector (RFC 9110 §6.3.3) and are
/// rejected as conflicting; agreeing duplicates are still rejected as
/// duplicates (safe default). This is a hardened framing policy applied
/// before body construction.
///
/// Note: Hyper 1.x strips a lone Content-Length header when
/// Transfer-Encoding is present (since 1.11, regardless of header order;
/// TE wins per RFC 9112 §6.1) and rejects duplicate Content-Length fields
/// while decoding the request. Consequently, the TE+CL branch below is
/// defense-in-depth for a future or alternate parser and is unreachable
/// behind Hyper 1.11 — the lone-CL+TE corpus cases now exercise Hyper's
/// TE-wins normalization (200 with chunked framing) rather than this
/// rejection. Keeping the branch makes the framing policy explicit at this
/// boundary.
pub(crate) fn validate_body_framing(headers: &hyper::HeaderMap) -> Result<(), ServiceError> {
    let has_te = headers.contains_key(hyper::header::TRANSFER_ENCODING);
    let cl_values: Vec<_> = headers
        .get_all(hyper::header::CONTENT_LENGTH)
        .iter()
        .collect();
    let has_cl = !cl_values.is_empty();

    if has_te && has_cl {
        return Err(ServiceError::rejected(
            400,
            "conflicting Transfer-Encoding and Content-Length",
        ));
    }

    if cl_values.len() > 1 {
        let first = cl_values[0].as_bytes();
        if cl_values[1..].iter().any(|v| v.as_bytes() != first) {
            return Err(ServiceError::rejected(
                400,
                "conflicting Content-Length headers",
            ));
        }
        return Err(ServiceError::rejected(
            400,
            "duplicate Content-Length headers",
        ));
    }

    Ok(())
}

/// Wrap a Hyper `Incoming` body into a `Stream<Item = Result<Bytes, IncomingError>>`.
///
/// This bridges the Hyper body type to the canonical `RequestBody` type
/// without leaking Hyper into the public API.
pub(crate) fn wrap_incoming_body(
    body: Incoming,
) -> impl futures_util::Stream<
    Item = Result<bytes::Bytes, crate::primitives::request_body::IncomingError>,
> + Send
       + 'static {
    use futures_util::StreamExt;
    http_body_util::BodyStream::new(body).filter_map(|result| async {
        match result {
            Ok(frame) => frame.into_data().ok().map(Ok),
            Err(e) => Some(Err(crate::primitives::request_body::IncomingError(
                e.to_string(),
            ))),
        }
    })
}

/// Convert a Hyper request to a canonical [`RequestHead`], enforcing the
/// EggServe-owned request-target and aggregate header ceilings before any
/// service work.
///
/// Hyper enforces `max_buf_size` (parse buffer) and `max_headers` (field
/// count, answered with 431 by Hyper itself) during parsing; those rejections
/// never reach this function and surface as parse-class connection errors.
/// What Hyper cannot bound independently — aggregate header bytes and
/// request-target length — is bounded here:
///
/// - request targets longer than `max_target_bytes` fail with 414;
/// - aggregate post-parse header name+value bytes above `max_header_bytes`
///   fail with 431.
///
/// There is no separate request-line knob: the request line is bounded
/// jointly by the parser buffer (raw bytes) and this target-length ceiling
/// (application semantics). Neither hostile targets nor header contents are
/// logged; only lengths are recorded as fields.
pub(crate) fn convert_request_head(
    req: &Request<Incoming>,
    max_target_bytes: usize,
    max_header_bytes: usize,
    expected_scheme: crate::primitives::connection_info::Scheme,
    conn_id: u64,
    ops: &crate::ops::OpsContext,
) -> Result<crate::primitives::request_head::RequestHead, ServiceError> {
    use crate::primitives::authority::Authority;
    use crate::primitives::header_block::HeaderBlock;
    use crate::primitives::method::Method;
    use crate::primitives::request_target::RequestTarget;
    use crate::primitives::version::HttpVersion;

    let method = match req.method().as_str() {
        "GET" => Method::get(),
        "HEAD" => Method::head(),
        "POST" => Method::post(),
        "PUT" => Method::put(),
        "DELETE" => Method::delete(),
        "PATCH" => Method::patch(),
        "OPTIONS" => Method::options(),
        "TRACE" => Method::trace(),
        other => Method::new(other)
            .map_err(|_| ServiceError::rejected(400, format!("invalid method: {}", other)))?,
    };

    let version = match req.version() {
        hyper::Version::HTTP_10 => HttpVersion::Http10,
        hyper::Version::HTTP_11 => HttpVersion::Http11,
        hyper::Version::HTTP_2 => HttpVersion::Http2,
        other => {
            return Err(ServiceError::rejected(
                505,
                format!("unsupported HTTP version: {:?}", other),
            ))
        }
    };

    let raw_target = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");

    if raw_target.len() > max_target_bytes {
        ops.counters()
            .request_target_rejected
            .fetch_add(1, Ordering::Relaxed);
        ops.emit(
            crate::ops::Event::new(
                crate::ops::Severity::Debug,
                crate::ops::EventKind::RequestTargetTooLong,
                "request target too long",
            )
            .connection_id(conn_id)
            .field(crate::ops::Field::U64(
                "target_bytes".into(),
                raw_target.len() as u64,
            ))
            .field(crate::ops::Field::U64(
                "limit_bytes".into(),
                max_target_bytes as u64,
            )),
        );
        return Err(ServiceError::rejected(414, "request target too long"));
    }

    let is_h2 = version == HttpVersion::Http2;

    // HTTP/1 absolute-form is intentionally not accepted. HTTP/2 carries
    // scheme and authority as pseudo-fields, which Hyper represents on the
    // URI; validate the scheme against the transport context instead.
    if !is_h2 && req.uri().scheme_str().is_some() {
        return Err(ServiceError::rejected(
            400,
            "absolute-form request target not allowed",
        ));
    }
    if is_h2 {
        if let Some(scheme) = req.uri().scheme_str() {
            if !scheme.eq_ignore_ascii_case(expected_scheme.as_str()) {
                return Err(ServiceError::rejected(400, "conflicting request scheme"));
            }
        }
    }

    // Asterisk-form (`*`) is rejected as method-not-allowed (405) rather
    // than bad-request (400) because the method check must fire before the
    // target-form check per the release contract.
    if raw_target == "*" {
        return Err(ServiceError::rejected(
            405,
            format!("method not allowed: {}", method.as_str()),
        ));
    }

    // Authority-form is reserved for CONNECT in HTTP/1, which the static
    // service does not implement. Preserve the established method-level 405
    // response before validating Host/authority metadata. HTTP/2 carries
    // authority as pseudo-field metadata and follows the branch below.
    if !is_h2 && req.uri().authority().is_some() {
        return Err(ServiceError::rejected(
            405,
            format!("method not allowed: {}", method.as_str()),
        ));
    }

    let target = RequestTarget::parse(raw_target)
        .map_err(|e| ServiceError::rejected(400, format!("invalid request target: {}", e)))?;

    let mut headers = HeaderBlock::new();
    let mut header_bytes: usize = 0;
    for (name, value) in req.headers().iter() {
        header_bytes = header_bytes
            .saturating_add(name.as_str().len())
            .saturating_add(value.len());
        if header_bytes > max_header_bytes {
            ops.counters()
                .header_bytes_rejected
                .fetch_add(1, Ordering::Relaxed);
            ops.emit(
                crate::ops::Event::new(
                    crate::ops::Severity::Debug,
                    crate::ops::EventKind::HeaderBytesRejected,
                    "request headers too large",
                )
                .connection_id(conn_id)
                .field(crate::ops::Field::U64(
                    "limit_bytes".into(),
                    max_header_bytes as u64,
                )),
            );
            return Err(ServiceError::rejected(431, "request headers too large"));
        }
        let header_name = crate::primitives::header_block::HeaderName::new(name.as_str())
            .map_err(|_| ServiceError::rejected(400, format!("invalid header name: {}", name)))?;
        // Byte-preserving inbound conversion (Plan 173 Track C1): legal opaque
        // bytes reach the service unchanged. Aggregate limits above already
        // count bytes, not Unicode scalars.
        let header_value = crate::primitives::header_block::HeaderValue::from_bytes(
            value.as_bytes(),
        )
        .map_err(|_| ServiceError::rejected(400, format!("invalid header value for {}", name)))?;
        headers.push(header_name, header_value);
    }

    let host_authorities = req
        .headers()
        .get_all(hyper::header::HOST)
        .iter()
        .map(|value| {
            Authority::parse(
                value
                    .to_str()
                    .map_err(|_| ServiceError::rejected(400, "invalid Host header"))?,
            )
            .map_err(|_| ServiceError::rejected(400, "invalid Host header"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let uri_authority = req
        .uri()
        .authority()
        .map(|authority| {
            Authority::parse(authority.as_str())
                .map_err(|_| ServiceError::rejected(400, "invalid :authority"))
        })
        .transpose()?;
    let host_authority = match host_authorities.as_slice() {
        [] => None,
        [first, rest @ ..] if rest.iter().all(|value| value == first) => Some(first.clone()),
        _ => return Err(ServiceError::rejected(400, "conflicting Host headers")),
    };
    let authority = match (uri_authority, host_authority) {
        (Some(uri), Some(host)) if uri != host => {
            return Err(ServiceError::rejected(
                400,
                "conflicting authority metadata",
            ));
        }
        (Some(uri), _) => Some(uri),
        (None, host) => host,
    };

    Ok(
        crate::primitives::request_head::RequestHead::new_with_authority(
            method, target, version, headers, authority,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framing_rejects_disagreeing_duplicate_content_length() {
        let mut headers = hyper::HeaderMap::new();
        headers.append(
            hyper::header::CONTENT_LENGTH,
            hyper::header::HeaderValue::from_static("5"),
        );
        headers.append(
            hyper::header::CONTENT_LENGTH,
            hyper::header::HeaderValue::from_static("10"),
        );
        let err = validate_body_framing(&headers).unwrap_err();
        assert_eq!(err.message(), "conflicting Content-Length headers");
    }

    #[test]
    fn framing_rejects_agreeing_duplicate_content_length() {
        let mut headers = hyper::HeaderMap::new();
        headers.append(
            hyper::header::CONTENT_LENGTH,
            hyper::header::HeaderValue::from_static("5"),
        );
        headers.append(
            hyper::header::CONTENT_LENGTH,
            hyper::header::HeaderValue::from_static("5"),
        );
        let err = validate_body_framing(&headers).unwrap_err();
        assert_eq!(err.message(), "duplicate Content-Length headers");
    }

    #[test]
    fn framing_accepts_single_content_length() {
        let mut headers = hyper::HeaderMap::new();
        headers.insert(
            hyper::header::CONTENT_LENGTH,
            hyper::header::HeaderValue::from_static("5"),
        );
        assert!(validate_body_framing(&headers).is_ok());
    }
}

//! Final Hyper response helpers owned by the runtime boundary.

#[cfg(feature = "http2")]
use bytes::Bytes;
#[cfg(feature = "http2")]
use http_body_util::combinators::UnsyncBoxBody;
#[cfg(feature = "http2")]
use http_body_util::BodyExt;
#[cfg(feature = "http2")]
use hyper::{Response, StatusCode};

/// The Hyper body type used by the runtime-owned error responses.
///
/// H2-only (Plan 249): the compatibility crate builds Hyper error responses
/// only for the H2-specific pipeline, so the alias exists exactly when H2
/// compiles.
#[cfg(feature = "http2")]
pub type BoxBodyInner = UnsyncBoxBody<Bytes, std::io::Error>;

/// Canonical error with an explicit representation policy.
///
/// The runtime-error representation is owned by the single authority
/// `eggserve_primitives::canonical::runtime_error_with_policy`; this crate
/// holds no second error-body table. `Minimal` emits the fixed generic
/// plain-text body; `Empty` emits no body bytes (`Content-Length: 0`, no
/// `Content-Type`) for runtime-generated errors. `Allow` for 405 is retained
/// under both variants. `HEAD` suppression remains correct (no body bytes).
/// Body-forbidden statuses (1xx/204/205/304) never emit body bytes regardless
/// of policy.
///
/// `body` is redundant: it is retained only so existing call sites keep
/// compiling, and the authority decides what is emitted. See
/// [`canonical_error_owned_with_policy`] for the drift check.
///
/// H2-only (Plan 249): H1 execution lives in `eggserve-server`; core builds
/// Hyper error responses only for the H2-specific pipeline.
#[cfg(feature = "http2")]
pub(crate) fn canonical_error_with_policy(
    status: StatusCode,
    body: &'static str,
    is_head: bool,
    policy: crate::policy::ErrorRepresentationPolicy,
) -> Response<BoxBodyInner> {
    canonical_error_owned_with_policy(status, body, is_head, policy)
}

/// Owned-body variant of [`canonical_error_with_policy`] for dynamically
/// derived error bodies (Plan 178 Track C).
///
/// The representation is produced by the single authority
/// `eggserve_primitives::canonical::runtime_error_with_policy`; this crate
/// holds no independent error-body construction.
///
/// `body` is redundant and is kept only so existing call sites keep
/// compiling: the authority is authoritative, and a caller whose body drifts
/// from the canonical representation is caught by a debug assertion instead of
/// being silently ignored. The assertion compares the supplied body with the
/// canonical representation whenever the response actually carries a payload
/// (`Minimal`, non-`HEAD`, payload-permitting status).
///
/// The status is canonicalized to the 100-599 range the authority requires.
/// A status outside that range is a caller bug and is caught by a debug
/// assertion.
#[cfg(feature = "http2")]
pub(crate) fn canonical_error_owned_with_policy(
    status: StatusCode,
    body: &str,
    is_head: bool,
    policy: crate::policy::ErrorRepresentationPolicy,
) -> Response<BoxBodyInner> {
    let canonical_status = crate::primitives::canonical::StatusCode::new(status.as_u16())
        .unwrap_or(crate::primitives::canonical::StatusCode::INTERNAL_SERVER_ERROR);
    debug_assert_eq!(
        canonical_status.as_u16(),
        status.as_u16(),
        "runtime error status must be inside the canonical 100-599 range"
    );
    debug_assert!(
        body_matches_authority(canonical_status, body, is_head, policy),
        "runtime error body must match the primitives authority representation"
    );
    runtime_error_with_policy(status, is_head, policy)
}

/// Debug-only drift check: the caller-supplied body must equal the canonical
/// representation whenever the response actually carries a payload.
///
/// A representation that emits no payload (`Empty` policy, `HEAD`, or a
/// body-forbidden status) cannot drift on the wire, so the check is vacuous
/// there.
#[cfg(feature = "http2")]
fn body_matches_authority(
    status: crate::primitives::canonical::StatusCode,
    body: &str,
    is_head: bool,
    policy: crate::policy::ErrorRepresentationPolicy,
) -> bool {
    if policy != crate::policy::ErrorRepresentationPolicy::Minimal
        || is_head
        || !status.permits_payload_body()
    {
        return true;
    }
    body.as_bytes() == canonical_error_body(status)
}

/// The canonical runtime-error body the authority renders for `status`.
///
/// This is a debug-only helper: it reuses the authority itself, so it can
/// never become a second representation table.
#[cfg(feature = "http2")]
fn canonical_error_body(status: crate::primitives::canonical::StatusCode) -> Vec<u8> {
    let canonical = crate::primitives::canonical::runtime_error_with_policy(
        status,
        false,
        crate::policy::ErrorRepresentationPolicy::Minimal,
    );
    match canonical.body() {
        Some(crate::primitives::canonical::ResponseBody::Bytes(bytes)) => bytes.clone(),
        _ => Vec::new(),
    }
}

/// Hyper conversion wrapper for the transport-neutral runtime-error builder
/// (Plan 178 Track C, corrected by Plan 189).
///
/// The canonical module owns the status/reason/body representation. This
/// wrapper applies the existing Hyper body type and leaves status selection to
/// its callers; no application detail is reflected.
///
/// H2-only (Plan 249): H1 execution lives in `eggserve-server`; core builds
/// Hyper error responses only for the H2-specific pipeline.
///
/// This is the single delegation target of the crate's runtime-error helpers:
/// every runtime error in this crate is rendered here, through
/// `eggserve_primitives::canonical::runtime_error_with_policy`.
#[cfg(feature = "http2")]
pub(crate) fn runtime_error_with_policy(
    status: StatusCode,
    is_head: bool,
    policy: crate::policy::ErrorRepresentationPolicy,
) -> Response<BoxBodyInner> {
    let canonical_status = crate::primitives::canonical::StatusCode::new(status.as_u16())
        .unwrap_or(crate::primitives::canonical::StatusCode::INTERNAL_SERVER_ERROR);
    let canonical =
        crate::primitives::canonical::runtime_error_with_policy(canonical_status, is_head, policy);
    let normalized = crate::primitives::canonical::normalize_response(
        canonical,
        &crate::primitives::canonical::NormalizeRequest::new(is_head),
    )
    .expect("canonical runtime error normalizes");
    crate::primitives::canonical::adapters::to_hyper_response_without_origin_date(normalized)
        .expect("canonical runtime error converts to Hyper")
        .map(|body| body.boxed_unsync())
}

#[cfg(feature = "http2")]
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

#[cfg(feature = "http2")]
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

#[cfg(feature = "http2")]
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

#[cfg(feature = "http2")]
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

#[cfg(feature = "http2")]
pub fn expectation_failed_with_policy(
    is_head: bool,
    policy: crate::policy::ErrorRepresentationPolicy,
) -> Response<BoxBodyInner> {
    canonical_error_with_policy(
        StatusCode::EXPECTATION_FAILED,
        "417 Expectation Failed\n",
        is_head,
        policy,
    )
}

#[cfg(all(test, feature = "http2"))]
pub fn not_found(is_head: bool) -> Response<BoxBodyInner> {
    canonical_error_with_policy(
        StatusCode::NOT_FOUND,
        "404 Not Found\n",
        is_head,
        crate::policy::ErrorRepresentationPolicy::Minimal,
    )
}

#[cfg(all(test, feature = "http2"))]
mod single_authority_tests {
    use super::*;
    use crate::policy::ErrorRepresentationPolicy;
    use http_body_util::BodyExt;

    /// Frozen baseline capture (Milestone 306).
    ///
    /// One row per matrix cell: `(status, policy, is_head, headers, body)`.
    /// The header list is the exact wire header set in wire order, and the
    /// body is the exact payload the pre-consolidation parallel builder
    /// produced. Captured at baseline `465b80cdabf90801e6ef929563b12886c509c201`
    /// from `canonical_error_owned_with_policy` before it was replaced by the
    /// delegation to `eggserve_primitives::canonical::runtime_error_with_policy`.
    ///
    /// The captured bytes are identical in `eggserve-server`
    /// (`crates/eggserve-server/src/response.rs`), so one table describes both
    /// crates.
    /// One frozen baseline cell: the status, the policy, whether the request
    /// was a HEAD, the exact wire header set, and the exact body bytes.
    type BaselineCell = (
        u16,
        ErrorRepresentationPolicy,
        bool,
        &'static [(&'static str, &'static str)],
        &'static str,
    );

    const BASELINE: &[BaselineCell] = &[
        (
            100,
            ErrorRepresentationPolicy::Minimal,
            false,
            &[("content-type", "text/plain; charset=utf-8")],
            "",
        ),
        (
            100,
            ErrorRepresentationPolicy::Minimal,
            true,
            &[("content-type", "text/plain; charset=utf-8")],
            "",
        ),
        (100, ErrorRepresentationPolicy::Empty, false, &[], ""),
        (100, ErrorRepresentationPolicy::Empty, true, &[], ""),
        (
            204,
            ErrorRepresentationPolicy::Minimal,
            false,
            &[("content-type", "text/plain; charset=utf-8")],
            "",
        ),
        (
            204,
            ErrorRepresentationPolicy::Minimal,
            true,
            &[("content-type", "text/plain; charset=utf-8")],
            "",
        ),
        (204, ErrorRepresentationPolicy::Empty, false, &[], ""),
        (204, ErrorRepresentationPolicy::Empty, true, &[], ""),
        (
            205,
            ErrorRepresentationPolicy::Minimal,
            false,
            &[("content-type", "text/plain; charset=utf-8")],
            "",
        ),
        (
            205,
            ErrorRepresentationPolicy::Minimal,
            true,
            &[("content-type", "text/plain; charset=utf-8")],
            "",
        ),
        (205, ErrorRepresentationPolicy::Empty, false, &[], ""),
        (205, ErrorRepresentationPolicy::Empty, true, &[], ""),
        (
            304,
            ErrorRepresentationPolicy::Minimal,
            false,
            &[("content-type", "text/plain; charset=utf-8")],
            "",
        ),
        (
            304,
            ErrorRepresentationPolicy::Minimal,
            true,
            &[("content-type", "text/plain; charset=utf-8")],
            "",
        ),
        (304, ErrorRepresentationPolicy::Empty, false, &[], ""),
        (304, ErrorRepresentationPolicy::Empty, true, &[], ""),
        (
            400,
            ErrorRepresentationPolicy::Minimal,
            false,
            &[
                ("content-type", "text/plain; charset=utf-8"),
                ("content-length", "16"),
            ],
            "400 Bad Request\n",
        ),
        (
            400,
            ErrorRepresentationPolicy::Minimal,
            true,
            &[
                ("content-type", "text/plain; charset=utf-8"),
                ("content-length", "0"),
            ],
            "",
        ),
        (
            400,
            ErrorRepresentationPolicy::Empty,
            false,
            &[("content-length", "0")],
            "",
        ),
        (
            400,
            ErrorRepresentationPolicy::Empty,
            true,
            &[("content-length", "0")],
            "",
        ),
        (
            404,
            ErrorRepresentationPolicy::Minimal,
            false,
            &[
                ("content-type", "text/plain; charset=utf-8"),
                ("content-length", "14"),
            ],
            "404 Not Found\n",
        ),
        (
            404,
            ErrorRepresentationPolicy::Minimal,
            true,
            &[
                ("content-type", "text/plain; charset=utf-8"),
                ("content-length", "0"),
            ],
            "",
        ),
        (
            404,
            ErrorRepresentationPolicy::Empty,
            false,
            &[("content-length", "0")],
            "",
        ),
        (
            404,
            ErrorRepresentationPolicy::Empty,
            true,
            &[("content-length", "0")],
            "",
        ),
        (
            405,
            ErrorRepresentationPolicy::Minimal,
            false,
            &[
                ("content-type", "text/plain; charset=utf-8"),
                ("allow", "GET, HEAD"),
                ("content-length", "23"),
            ],
            "405 Method Not Allowed\n",
        ),
        (
            405,
            ErrorRepresentationPolicy::Minimal,
            true,
            &[
                ("content-type", "text/plain; charset=utf-8"),
                ("allow", "GET, HEAD"),
                ("content-length", "0"),
            ],
            "",
        ),
        (
            405,
            ErrorRepresentationPolicy::Empty,
            false,
            &[("allow", "GET, HEAD"), ("content-length", "0")],
            "",
        ),
        (
            405,
            ErrorRepresentationPolicy::Empty,
            true,
            &[("allow", "GET, HEAD"), ("content-length", "0")],
            "",
        ),
        (
            413,
            ErrorRepresentationPolicy::Minimal,
            false,
            &[
                ("content-type", "text/plain; charset=utf-8"),
                ("content-length", "22"),
            ],
            "413 Payload Too Large\n",
        ),
        (
            413,
            ErrorRepresentationPolicy::Minimal,
            true,
            &[
                ("content-type", "text/plain; charset=utf-8"),
                ("content-length", "0"),
            ],
            "",
        ),
        (
            413,
            ErrorRepresentationPolicy::Empty,
            false,
            &[("content-length", "0")],
            "",
        ),
        (
            413,
            ErrorRepresentationPolicy::Empty,
            true,
            &[("content-length", "0")],
            "",
        ),
        (
            417,
            ErrorRepresentationPolicy::Minimal,
            false,
            &[
                ("content-type", "text/plain; charset=utf-8"),
                ("content-length", "23"),
            ],
            "417 Expectation Failed\n",
        ),
        (
            417,
            ErrorRepresentationPolicy::Minimal,
            true,
            &[
                ("content-type", "text/plain; charset=utf-8"),
                ("content-length", "0"),
            ],
            "",
        ),
        (
            417,
            ErrorRepresentationPolicy::Empty,
            false,
            &[("content-length", "0")],
            "",
        ),
        (
            417,
            ErrorRepresentationPolicy::Empty,
            true,
            &[("content-length", "0")],
            "",
        ),
        (
            500,
            ErrorRepresentationPolicy::Minimal,
            false,
            &[
                ("content-type", "text/plain; charset=utf-8"),
                ("content-length", "26"),
            ],
            "500 Internal Server Error\n",
        ),
        (
            500,
            ErrorRepresentationPolicy::Minimal,
            true,
            &[
                ("content-type", "text/plain; charset=utf-8"),
                ("content-length", "0"),
            ],
            "",
        ),
        (
            500,
            ErrorRepresentationPolicy::Empty,
            false,
            &[("content-length", "0")],
            "",
        ),
        (
            500,
            ErrorRepresentationPolicy::Empty,
            true,
            &[("content-length", "0")],
            "",
        ),
        (
            503,
            ErrorRepresentationPolicy::Minimal,
            false,
            &[
                ("content-type", "text/plain; charset=utf-8"),
                ("content-length", "24"),
            ],
            "503 Service Unavailable\n",
        ),
        (
            503,
            ErrorRepresentationPolicy::Minimal,
            true,
            &[
                ("content-type", "text/plain; charset=utf-8"),
                ("content-length", "0"),
            ],
            "",
        ),
        (
            503,
            ErrorRepresentationPolicy::Empty,
            false,
            &[("content-length", "0")],
            "",
        ),
        (
            503,
            ErrorRepresentationPolicy::Empty,
            true,
            &[("content-length", "0")],
            "",
        ),
    ];

    /// The canonical body the authority renders for a payload-carrying GET.
    fn canonical_body(status: u16) -> &'static str {
        match status {
            100 => "100 Continue\n",
            204 => "204 No Content\n",
            205 => "205 Reset Content\n",
            304 => "304 Not Modified\n",
            400 => "400 Bad Request\n",
            404 => "404 Not Found\n",
            405 => "405 Method Not Allowed\n",
            413 => "413 Payload Too Large\n",
            417 => "417 Expectation Failed\n",
            500 => "500 Internal Server Error\n",
            503 => "503 Service Unavailable\n",
            _ => "",
        }
    }

    fn header_pairs(response: &Response<BoxBodyInner>) -> Vec<(String, String)> {
        response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_owned(),
                    value.to_str().expect("header value is text").to_owned(),
                )
            })
            .collect()
    }

    /// The delegated runtime-error path must reproduce the frozen baseline
    /// capture byte for byte, for every status x policy x HEAD cell.
    #[tokio::test]
    async fn runtime_error_representation_matches_the_baseline_capture() {
        for (status, policy, is_head, expected_headers, expected_body) in BASELINE {
            let hyper_status = hyper::StatusCode::from_u16(*status).expect("canonical status");
            let response = super::canonical_error_owned_with_policy(
                hyper_status,
                canonical_body(*status),
                *is_head,
                *policy,
            );
            assert_eq!(
                response.status().as_u16(),
                *status,
                "status for {status} {policy:?} head={is_head}"
            );
            let expected: Vec<(String, String)> = expected_headers
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect();
            assert_eq!(
                header_pairs(&response),
                expected,
                "headers for {status} {policy:?} head={is_head}"
            );
            let collected = response.into_body().collect().await.expect("body collects");
            assert_eq!(
                collected.to_bytes().as_ref(),
                expected_body.as_bytes(),
                "body for {status} {policy:?} head={is_head}"
            );
        }
    }

    /// A body-forbidden status never emits a payload, so a caller-supplied
    /// body cannot drift on the wire and must not trip the drift check.
    #[test]
    fn body_forbidden_status_tolerates_a_supplied_body() {
        let response = super::canonical_error_owned_with_policy(
            hyper::StatusCode::NO_CONTENT,
            "204 No Content\n",
            false,
            ErrorRepresentationPolicy::Minimal,
        );
        assert_eq!(response.status().as_u16(), 204);
        assert_eq!(
            header_pairs(&response),
            vec![(
                "content-type".to_string(),
                "text/plain; charset=utf-8".to_string()
            )]
        );
    }

    /// A drifting caller body is caught by the debug assertion instead of
    /// being silently ignored.
    #[cfg(debug_assertions)]
    #[test]
    fn a_drifting_caller_body_is_caught() {
        let result = std::panic::catch_unwind(|| {
            super::canonical_error_owned_with_policy(
                hyper::StatusCode::BAD_REQUEST,
                "400 Not A Request\n",
                false,
                ErrorRepresentationPolicy::Minimal,
            );
        });
        assert!(
            result.is_err(),
            "a caller body that drifts from the authority must be caught"
        );
    }
}

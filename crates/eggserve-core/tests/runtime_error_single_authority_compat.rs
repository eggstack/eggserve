//! Milestone 306 — the compatibility facade exposes the single runtime-error
//! representation authority.
//!
//! `eggserve_primitives::canonical::runtime_error_with_policy` is the sole
//! runtime-error representation table for H1, H2, and H3. The compatibility
//! crate (`eggserve-core`) must not carry a second table: it re-exports the
//! authority and delegates to it.
//!
//! This suite proves, from outside the crate, that
//!
//! 1. the compatibility facade (`eggserve_core::primitives::canonical`)
//!    renders byte-identical output to the direct authority for the whole
//!    status x policy x HEAD x body-forbidden matrix, and
//! 2. that output carries the documented status, header set, and body.
//!
//! The crate-internal equivalence test (which covers the crate-private
//! helpers this milestone changed) lives in
//! `crates/eggserve-core/src/response.rs`. It carries the frozen baseline
//! capture of the pre-consolidation wire bytes.

use eggserve_core::policy::ErrorRepresentationPolicy;
use eggserve_core::primitives::canonical::{
    normalize_response, runtime_error_with_policy, NormalizeRequest, StatusCode,
};
use http_body_util::BodyExt;

/// The canonical plain-text body the authority renders for a status.
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

/// The status codes the runtime error path can render.
const MATRIX_STATUSES: &[u16] = &[100, 204, 205, 304, 400, 404, 405, 413, 417, 500, 503];

/// The rendered representation of one runtime error.
struct Representation {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

/// Render a runtime error through the compatibility facade.
async fn compat_representation(
    status: u16,
    is_head: bool,
    policy: ErrorRepresentationPolicy,
) -> Representation {
    let canonical_status = StatusCode::new(status).expect("canonical status");
    let canonical = runtime_error_with_policy(canonical_status, is_head, policy);
    let normalized =
        normalize_response(canonical, &NormalizeRequest::new(is_head)).expect("normalizes");
    let response =
        eggserve_core::layers::server::adapters::to_hyper_response_without_origin_date(normalized)
            .expect("converts to the Hyper boundary");
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                value.to_str().expect("header value is text").to_owned(),
            )
        })
        .collect();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body collects")
        .to_bytes()
        .to_vec();
    Representation {
        status,
        headers,
        body,
    }
}

/// The header set the representation must carry, derived from the documented
/// rules: `Minimal` adds `Content-Type`, `405` adds `Allow`, payload-permitting
/// statuses carry `Content-Length`, and body-forbidden statuses carry none.
fn expected_headers(
    status: u16,
    policy: ErrorRepresentationPolicy,
    is_head: bool,
) -> Vec<(String, String)> {
    let mut expected: Vec<(String, String)> = Vec::new();
    if policy == ErrorRepresentationPolicy::Minimal {
        expected.push((
            "content-type".to_string(),
            "text/plain; charset=utf-8".to_string(),
        ));
    }
    if status == 405 {
        expected.push(("allow".to_string(), "GET, HEAD".to_string()));
    }
    let body_forbidden = status < 200 || status == 204 || status == 205 || status == 304;
    if !body_forbidden {
        let carries_body = policy == ErrorRepresentationPolicy::Minimal && !is_head;
        let length = if carries_body {
            canonical_body(status).len()
        } else {
            0
        };
        expected.push(("content-length".to_string(), length.to_string()));
    }
    expected
}

/// The compatibility facade renders the documented representation for the
/// whole matrix.
#[tokio::test]
async fn the_compat_facade_renders_the_documented_representation() {
    for status in MATRIX_STATUSES {
        for policy in [
            ErrorRepresentationPolicy::Minimal,
            ErrorRepresentationPolicy::Empty,
        ] {
            for is_head in [false, true] {
                let rendered = compat_representation(*status, is_head, policy).await;
                assert_eq!(
                    rendered.status, *status,
                    "status for {status} {policy:?} head={is_head}"
                );
                let body_forbidden =
                    *status < 200 || *status == 204 || *status == 205 || *status == 304;
                let expected_body =
                    if policy == ErrorRepresentationPolicy::Empty || is_head || body_forbidden {
                        Vec::new()
                    } else {
                        canonical_body(*status).as_bytes().to_vec()
                    };
                assert_eq!(
                    rendered.headers,
                    expected_headers(*status, policy, is_head),
                    "headers for {status} {policy:?} head={is_head}"
                );
                assert_eq!(
                    rendered.body, expected_body,
                    "body for {status} {policy:?} head={is_head}"
                );
            }
        }
    }
}

/// The compatibility facade and the direct authority are the same
/// representation, byte for byte.
#[tokio::test]
async fn the_compat_facade_matches_the_direct_authority() {
    for status in MATRIX_STATUSES {
        for policy in [
            ErrorRepresentationPolicy::Minimal,
            ErrorRepresentationPolicy::Empty,
        ] {
            for is_head in [false, true] {
                let compat = compat_representation(*status, is_head, policy).await;
                let direct_status =
                    eggserve_primitives::canonical::StatusCode::new(*status).expect("canonical");
                let canonical = eggserve_primitives::canonical::runtime_error_with_policy(
                    direct_status,
                    is_head,
                    policy,
                );
                let normalized = eggserve_primitives::canonical::normalize_response(
                    canonical,
                    &eggserve_primitives::canonical::NormalizeRequest::new(is_head),
                )
                .expect("normalizes");
                let direct =
                    eggserve_core::layers::server::adapters::to_hyper_response_without_origin_date(
                        normalized,
                    )
                    .expect("converts to the Hyper boundary");
                let direct_status_code = direct.status().as_u16();
                let direct_headers: Vec<(String, String)> = direct
                    .headers()
                    .iter()
                    .map(|(name, value)| {
                        (
                            name.as_str().to_owned(),
                            value.to_str().expect("header value is text").to_owned(),
                        )
                    })
                    .collect();
                let direct_body = direct
                    .into_body()
                    .collect()
                    .await
                    .expect("body collects")
                    .to_bytes()
                    .to_vec();
                assert_eq!(
                    compat.status, direct_status_code,
                    "status for {status} {policy:?} head={is_head}"
                );
                assert_eq!(
                    compat.headers, direct_headers,
                    "headers for {status} {policy:?} head={is_head}"
                );
                assert_eq!(
                    compat.body, direct_body,
                    "body for {status} {policy:?} head={is_head}"
                );
            }
        }
    }
}

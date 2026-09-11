//! Canonical response types for transport-independent response construction.
//!
//! [`Response`] is the unified response value that all response producers
//! converge on before transport conversion. The [`normalize_response`] function
//! applies the final normalization rules (HEAD suppression, body-forbidden
//! enforcement, hop-by-hop stripping, content-length computation) immediately
//! before the response is sent on the wire.
//!
//! # Conversion model
//!
//! Existing response producers ([`super::response::StaticResponsePlan`],
//! Python callback handlers) are adapted to [`Response`] via `From`/`Into`
//! impls. The normalization function consumes the response body for HEAD and
//! body-forbidden statuses, enforcing the invariant that no body bytes are
//! transmitted for these responses.
//!
//! # Streaming bodies (Plan 162)
//!
//! A Rust [`crate::server::Service`] may return [`ResponseBody::Stream`] for
//! incrementally produced bodies. The runtime remains the only authority for
//! `Content-Length`, `Transfer-Encoding`, and connection reuse:
//!
//! - known-length streams send runtime-generated `Content-Length`; underrun or
//!   overrun closes the connection after commitment;
//! - unknown-length streams omit `Content-Length` and let HTTP/1 select
//!   chunked framing; successful completion may keep the connection reusable;
//! - `HEAD` and 1xx/204/205/304 never poll the stream; dropping releases the
//!   producer promptly.
//!
//! `handler_timeout` bounds only the service future (time to produce the
//! `Response`), not the subsequent body stream. Streaming is bounded by
//! `connection_total_timeout` and shutdown. Plan 164 will add
//! write/no-progress controls.

//! Module layout (Plan 206 Track C): `status` owns `StatusCode`/
//! `ResponseConstructionError`; `headers` owns `ResponseHead` + hop-by-hop
//! authority; `response_body` owns `BodyLength`/`ResponseBody`; `response`
//! owns `Response`/`ResponseBuilder`/normalization; `adapters` owns the
//! Hyper conversion boundary. Public paths are preserved via re-exports.

pub mod adapters;
pub mod headers;
pub mod response;
pub mod response_body;
pub mod status;

pub use super::response_stream::{ResponseStream, ResponseStreamError};
pub use adapters::to_hyper_response;
#[allow(unused_imports)]
pub(crate) use adapters::{
    to_hyper_response_with_file_stream_semaphore,
    to_hyper_response_with_file_stream_semaphore_and_chunk_size,
};
pub use headers::{is_hop_by_hop_header, ResponseHead};
pub(crate) use response::runtime_error_with_policy;
pub use response::{
    normalize_metadata, normalize_response, NormalizeRequest, Response, ResponseBuilder,
};
pub use response_body::{BodyLength, ResponseBody};
pub use status::{ResponseConstructionError, StatusCode};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::body::BodySource;
    use crate::primitives::header_block::HeaderBlock;
    use crate::primitives::FileRange;
    use http_body_util::BodyExt;
    use std::fs::File;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn file_response(path: &std::path::Path, range: Option<FileRange>) -> Response {
        let file = File::open(path).unwrap();
        let metadata = file.metadata().unwrap();
        let source = match range {
            Some(range) => BodySource::FileRange {
                file,
                range,
                total_len: metadata.len(),
                mime: "application/octet-stream",
            },
            None => BodySource::FileFull {
                file,
                len: metadata.len(),
                mime: "application/octet-stream",
            },
        };
        Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::File(source))
            .unwrap()
    }

    #[test]
    fn runtime_error_representation_preserves_status_and_reason() {
        for code in [400, 405, 408, 413, 414, 431, 500, 503] {
            let status = StatusCode::new(code).unwrap();
            let response = normalize_response(
                runtime_error_with_policy(
                    status,
                    false,
                    crate::policy::ErrorRepresentationPolicy::Minimal,
                ),
                &NormalizeRequest::new(false),
            )
            .unwrap();
            assert_eq!(response.status(), status);
            assert_eq!(
                response
                    .headers()
                    .get_first("content-type")
                    .unwrap()
                    .to_str()
                    .unwrap(),
                "text/plain; charset=utf-8"
            );
            assert!(
                matches!(response.body(), Some(ResponseBody::Bytes(body)) if body.starts_with(code.to_string().as_bytes()))
            );
            if code == 405 {
                assert_eq!(
                    response
                        .headers()
                        .get_first("allow")
                        .unwrap()
                        .to_str()
                        .unwrap(),
                    "GET, HEAD"
                );
            }
        }

        let unassigned = StatusCode::new(499).unwrap();
        let response = normalize_response(
            runtime_error_with_policy(
                unassigned,
                false,
                crate::policy::ErrorRepresentationPolicy::Minimal,
            ),
            &NormalizeRequest::new(false),
        )
        .unwrap();
        assert_eq!(response.status(), unassigned);
        assert!(matches!(response.body(), Some(ResponseBody::Empty)));
    }

    #[test]
    fn runtime_error_empty_and_head_suppress_representation_bytes() {
        let status = StatusCode::INTERNAL_SERVER_ERROR;
        for (is_head, policy) in [
            (true, crate::policy::ErrorRepresentationPolicy::Minimal),
            (false, crate::policy::ErrorRepresentationPolicy::Empty),
        ] {
            let response = normalize_response(
                runtime_error_with_policy(status, is_head, policy),
                &NormalizeRequest::new(is_head),
            )
            .unwrap();
            if is_head {
                assert!(matches!(
                    response.body(),
                    Some(ResponseBody::EmptyWithLength(0))
                ));
            } else {
                assert!(matches!(response.body(), Some(ResponseBody::Empty)));
            }
            assert_eq!(
                response
                    .headers()
                    .get_first("content-length")
                    .unwrap()
                    .to_str()
                    .unwrap(),
                "0"
            );
        }
    }

    #[tokio::test]
    async fn full_file_transport_body_owns_permit_until_drop() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("full.bin");
        std::fs::write(&path, b"full body").unwrap();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(1));

        let first =
            to_hyper_response_with_file_stream_semaphore(file_response(&path, None), &semaphore)
                .unwrap();
        assert!(matches!(
            to_hyper_response_with_file_stream_semaphore(file_response(&path, None), &semaphore),
            Err(ResponseConstructionError::FileStreamLimit)
        ));

        drop(first);
        assert!(to_hyper_response_with_file_stream_semaphore(
            file_response(&path, None),
            &semaphore
        )
        .is_ok());
    }

    #[tokio::test]
    async fn range_file_transport_body_releases_permit_on_completion() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("range.bin");
        std::fs::write(&path, b"range body").unwrap();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(1));

        let response = to_hyper_response_with_file_stream_semaphore(
            file_response(&path, Some(FileRange::new(0, 4))),
            &semaphore,
        )
        .unwrap();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"range");
        assert!(to_hyper_response_with_file_stream_semaphore(
            file_response(&path, Some(FileRange::new(5, 9))),
            &semaphore
        )
        .is_ok());
    }

    #[tokio::test]
    async fn file_transport_uses_configured_chunk_size() {
        use futures_util::StreamExt;

        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("chunked.bin");
        std::fs::write(&path, vec![b'x'; 130]).unwrap();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(1));
        let response = to_hyper_response_with_file_stream_semaphore_and_chunk_size(
            file_response(&path, None),
            &semaphore,
            64,
            None,
        )
        .unwrap();

        let mut body = response.into_body().into_data_stream();
        let mut chunk_lengths = Vec::new();
        while let Some(chunk) = body.next().await {
            chunk_lengths.push(chunk.unwrap().len());
        }
        assert_eq!(chunk_lengths, [64, 64, 2]);
    }

    #[tokio::test]
    async fn truncated_file_transport_reports_unexpected_eof() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("truncated.bin");
        std::fs::write(&path, b"short").unwrap();
        let file = File::open(&path).unwrap();
        let response = Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::File(BodySource::FileFull {
                file,
                len: 10,
                mime: "application/octet-stream",
            }))
            .unwrap();

        let error = to_hyper_response(response)
            .unwrap()
            .into_body()
            .collect()
            .await
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn non_file_and_normalized_head_bodies_bypass_file_admission() {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(1));
        let held = semaphore.clone().try_acquire_owned().unwrap();

        for body in [
            ResponseBody::Bytes(b"bytes".to_vec()),
            ResponseBody::Empty,
            ResponseBody::EmptyWithLength(5),
        ] {
            let response = Response::builder()
                .status(StatusCode::OK)
                .body(body)
                .unwrap();
            assert!(to_hyper_response_with_file_stream_semaphore(response, &semaphore).is_ok());
        }

        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("head.bin");
        std::fs::write(&path, b"head body").unwrap();
        let normalized =
            normalize_response(file_response(&path, None), &NormalizeRequest::new(true)).unwrap();
        assert!(to_hyper_response_with_file_stream_semaphore(normalized, &semaphore).is_ok());

        drop(held);
    }

    #[test]
    fn status_code_valid_range() {
        assert!(StatusCode::new(100).is_ok());
        assert!(StatusCode::new(200).is_ok());
        assert!(StatusCode::new(600).is_err());
    }

    #[test]
    fn status_code_zero_rejected() {
        assert!(StatusCode::new(0).is_err());
    }

    #[test]
    fn status_code_below_100_rejected() {
        assert!(StatusCode::new(1).is_err());
        assert!(StatusCode::new(42).is_err());
        assert!(StatusCode::new(99).is_err());
    }

    #[test]
    fn status_code_over_599_rejected() {
        assert!(StatusCode::new(600).is_err());
        assert!(StatusCode::new(1000).is_err());
    }

    #[test]
    fn status_code_boundary_values() {
        assert!(StatusCode::new(100).is_ok());
        assert!(StatusCode::new(199).is_ok());
        assert!(StatusCode::new(200).is_ok());
        assert!(StatusCode::new(599).is_ok());
    }

    #[test]
    fn status_code_classification() {
        assert!(StatusCode::CONTINUE.is_informational());
        assert!(!StatusCode::OK.is_informational());
        assert!(StatusCode::OK.is_success());
        assert!(StatusCode::NOT_MODIFIED.is_redirection());
        assert!(StatusCode::BAD_REQUEST.is_client_error());
        assert!(StatusCode::INTERNAL_SERVER_ERROR.is_server_error());
    }

    #[test]
    fn status_code_permits_payload() {
        assert!(!StatusCode::CONTINUE.permits_payload_body());
        assert!(!StatusCode::NO_CONTENT.permits_payload_body());
        assert!(!StatusCode::NOT_MODIFIED.permits_payload_body());
        assert!(!StatusCode::new(205).unwrap().permits_payload_body());
        assert!(StatusCode::OK.permits_payload_body());
        assert!(StatusCode::RANGE_NOT_SATISFIABLE.permits_payload_body());
    }

    #[test]
    fn response_body_len() {
        assert_eq!(ResponseBody::Empty.len(), 0);
        assert_eq!(ResponseBody::Bytes(b"hello".to_vec()).len(), 5);
    }

    #[test]
    fn response_body_into_bytes() {
        assert!(ResponseBody::Empty.into_bytes().is_none());
        assert_eq!(
            ResponseBody::Bytes(b"hi".to_vec()).into_bytes(),
            Some(b"hi".to_vec())
        );
    }

    #[test]
    fn response_builder_creates_response() {
        let resp = Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/plain")
            .unwrap()
            .body(ResponseBody::Bytes(b"ok".to_vec()))
            .unwrap();

        assert_eq!(resp.status().as_u16(), 200);
        assert_eq!(
            resp.headers()
                .get_first("content-type")
                .unwrap()
                .to_str()
                .unwrap(),
            "text/plain"
        );
    }

    #[test]
    fn response_builder_empty_body() {
        let resp = Response::builder()
            .status(StatusCode::NO_CONTENT)
            .empty()
            .unwrap();
        assert_eq!(resp.status().as_u16(), 204);
        assert!(resp.body().unwrap().is_empty());
    }

    #[test]
    fn response_builder_no_status_returns_error() {
        let result = Response::builder()
            .header("content-type", "text/plain")
            .unwrap()
            .empty();
        assert!(result.is_err());
    }

    #[test]
    fn response_builder_invalid_header_name_rejected() {
        let result = Response::builder()
            .status(StatusCode::OK)
            .header("", "value");
        assert!(result.is_err());
    }

    #[test]
    fn response_builder_invalid_header_value_rejected() {
        let result = Response::builder()
            .status(StatusCode::OK)
            .header("x-test", "val\r\ninjection");
        assert!(result.is_err());
    }

    #[test]
    fn normalize_head_suppresses_body() {
        let resp = Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(b"hello".to_vec()))
            .unwrap();

        let req = NormalizeRequest::new(true);
        let normalized = normalize_response(resp, &req).unwrap();
        // No bytes are sent for HEAD, but the equivalent-GET representation
        // length is retained so consumers still observe it.
        assert!(matches!(
            normalized.body().unwrap(),
            ResponseBody::EmptyWithLength(5)
        ));
        assert_eq!(normalized.body().unwrap().len(), 5);
        assert_eq!(
            normalized
                .headers()
                .get_first("content-length")
                .unwrap()
                .to_str()
                .unwrap(),
            "5"
        );
    }

    #[test]
    fn normalize_head_unknown_length_sends_no_body_and_omits_length() {
        use futures_util::stream;
        let inner = stream::iter(vec![Ok::<_, ResponseStreamError>(
            bytes::Bytes::from_static(b"chunk"),
        )]);
        let resp = Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Stream(ResponseStream::new(inner)))
            .unwrap();

        let req = NormalizeRequest::new(true);
        let normalized = normalize_response(resp, &req).unwrap();
        assert!(matches!(normalized.body().unwrap(), ResponseBody::Empty));
        assert!(!normalized.headers().contains("content-length"));
    }

    #[test]
    fn normalize_304_suppresses_body() {
        let resp = Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header("etag", "W/\"123\"")
            .unwrap()
            .body(ResponseBody::Empty)
            .unwrap();

        let req = NormalizeRequest::new(false);
        let normalized = normalize_response(resp, &req).unwrap();
        assert_eq!(normalized.status().as_u16(), 304);
        assert!(normalized.body().unwrap().is_empty());
    }

    #[test]
    fn normalize_304_discards_buffered_body_length() {
        let response = Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header("content-length", "5")
            .unwrap()
            .body(ResponseBody::Bytes(b"hello".to_vec()))
            .unwrap();
        let normalized = normalize_response(response, &NormalizeRequest::new(false)).unwrap();
        assert_eq!(
            normalized
                .headers()
                .get_first("content-length")
                .unwrap()
                .to_str()
                .unwrap(),
            "5"
        );

        let mismatched = Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header("content-length", "4")
            .unwrap()
            .body(ResponseBody::Bytes(b"hello".to_vec()))
            .unwrap();
        let normalized = normalize_response(mismatched, &NormalizeRequest::new(false)).unwrap();
        assert!(!normalized.headers().contains("content-length"));
    }

    #[test]
    fn normalize_head_304_preserves_representation_length() {
        let response = Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header("content-length", "10")
            .unwrap()
            .body(ResponseBody::Bytes(vec![b'x'; 10]))
            .unwrap();

        let normalized = normalize_response(response, &NormalizeRequest::new(true)).unwrap();
        assert!(normalized.body().unwrap().is_empty());
        assert_eq!(
            normalized
                .headers()
                .get_first("content-length")
                .unwrap()
                .to_str()
                .unwrap(),
            "10"
        );
    }

    #[test]
    fn normalize_204_suppresses_body() {
        let resp = Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(ResponseBody::Bytes(b"unexpected".to_vec()))
            .unwrap();

        let req = NormalizeRequest::new(false);
        let normalized = normalize_response(resp, &req).unwrap();
        assert!(normalized.body().unwrap().is_empty());
    }

    #[test]
    fn normalize_205_suppresses_body_and_content_length() {
        let resp = Response::builder()
            .status(StatusCode::RESET_CONTENT)
            .body(ResponseBody::Bytes(b"unexpected".to_vec()))
            .unwrap();
        let normalized = normalize_response(resp, &NormalizeRequest::new(false)).unwrap();
        assert!(normalized.body().unwrap().is_empty());
        assert!(!normalized.headers().contains("content-length"));
    }

    #[test]
    fn normalize_205_rejects_caller_content_length() {
        let response = Response::builder()
            .status(StatusCode::RESET_CONTENT)
            .header("content-length", "5")
            .unwrap()
            .body(ResponseBody::Empty)
            .unwrap();

        assert_eq!(
            normalize_response(response, &NormalizeRequest::new(false)).unwrap_err(),
            ResponseConstructionError::ForbiddenFramingHeader("content-length".to_owned())
        );
    }

    #[test]
    fn normalize_strips_transfer_encoding() {
        let resp = Response::builder()
            .status(StatusCode::OK)
            .header("transfer-encoding", "chunked")
            .unwrap()
            .body(ResponseBody::Bytes(b"hello".to_vec()))
            .unwrap();

        let req = NormalizeRequest::new(false);
        let normalized = normalize_response(resp, &req).unwrap();
        assert!(!normalized.headers().contains("transfer-encoding"));
    }

    #[test]
    fn normalize_sets_content_length() {
        let resp = Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(b"hello".to_vec()))
            .unwrap();

        let req = NormalizeRequest::new(false);
        let normalized = normalize_response(resp, &req).unwrap();
        assert_eq!(
            normalized
                .headers()
                .get_first("content-length")
                .unwrap()
                .to_str()
                .unwrap(),
            "5"
        );
    }

    #[test]
    fn normalize_1xx_suppresses_body() {
        let resp = Response::builder()
            .status(StatusCode::CONTINUE)
            .body(ResponseBody::Bytes(b"data".to_vec()))
            .unwrap();

        let req = NormalizeRequest::new(false);
        let normalized = normalize_response(resp, &req).unwrap();
        assert!(normalized.body().unwrap().is_empty());
    }

    #[test]
    fn normalize_duplicate_headers_preserved() {
        let mut resp = Response::builder()
            .status(StatusCode::OK)
            .body(ResponseBody::Bytes(b"ok".to_vec()))
            .unwrap();
        resp.head_mut()
            .headers_mut()
            .push_str("set-cookie", "a=1")
            .unwrap();
        resp.head_mut()
            .headers_mut()
            .push_str("set-cookie", "b=2")
            .unwrap();

        let req = NormalizeRequest::new(false);
        let normalized = normalize_response(resp, &req).unwrap();
        let all = normalized.headers().get_all("set-cookie");
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn response_construction_error_display() {
        let err = ResponseConstructionError::InvalidStatus(0);
        assert!(err.to_string().contains("0"));

        let err = ResponseConstructionError::ForbiddenFramingHeader("transfer-encoding".into());
        assert!(err.to_string().contains("transfer-encoding"));

        let err = ResponseConstructionError::BodyAlreadyConsumed;
        assert!(!err.to_string().is_empty());

        let err = ResponseConstructionError::ContentLengthMismatch {
            declared: 100,
            actual: 50,
        };
        assert!(err.to_string().contains("100"));
        assert!(err.to_string().contains("50"));
    }

    #[test]
    fn status_code_display() {
        assert_eq!(format!("{}", StatusCode::OK), "200");
        assert_eq!(format!("{}", StatusCode::NOT_FOUND), "404");
    }

    #[test]
    fn status_code_into_u16() {
        let code: u16 = StatusCode::OK.into();
        assert_eq!(code, 200);
    }

    #[test]
    fn is_hop_by_hop_header_recognizes_all_variants() {
        assert!(is_hop_by_hop_header("connection"));
        assert!(is_hop_by_hop_header("Connection"));
        assert!(is_hop_by_hop_header("CONNECTION"));
        assert!(is_hop_by_hop_header("keep-alive"));
        assert!(is_hop_by_hop_header("Keep-Alive"));
        assert!(is_hop_by_hop_header("proxy-authenticate"));
        assert!(is_hop_by_hop_header("proxy-authorization"));
        assert!(is_hop_by_hop_header("proxy-connection"));
        assert!(is_hop_by_hop_header("te"));
        assert!(is_hop_by_hop_header("TE"));
        assert!(is_hop_by_hop_header("trailer"));
        assert!(is_hop_by_hop_header("Trailer"));
        assert!(is_hop_by_hop_header("transfer-encoding"));
        assert!(is_hop_by_hop_header("Transfer-Encoding"));
        assert!(is_hop_by_hop_header("upgrade"));
        assert!(is_hop_by_hop_header("Upgrade"));
    }

    #[test]
    fn is_hop_by_hop_header_rejects_end_to_end() {
        assert!(!is_hop_by_hop_header("content-type"));
        assert!(!is_hop_by_hop_header("content-length"));
        assert!(!is_hop_by_hop_header("host"));
        assert!(!is_hop_by_hop_header("set-cookie"));
        assert!(!is_hop_by_hop_header("etag"));
        assert!(!is_hop_by_hop_header("authorization"));
        assert!(!is_hop_by_hop_header("cache-control"));
    }

    #[test]
    fn normalize_metadata_strips_all_hop_by_hop() {
        let code = StatusCode::OK;
        let mut headers = HeaderBlock::new();
        headers.push_str("content-type", "text/plain").unwrap();
        headers.push_str("transfer-encoding", "chunked").unwrap();
        headers.push_str("connection", "keep-alive").unwrap();
        headers.push_str("trailer", "x-checksum").unwrap();
        headers.push_str("upgrade", "h2c").unwrap();
        headers.push_str("te", "deflate").unwrap();

        normalize_metadata(code, &mut headers, 5).unwrap();

        assert!(!headers.contains("transfer-encoding"));
        assert!(!headers.contains("connection"));
        assert!(!headers.contains("trailer"));
        assert!(!headers.contains("upgrade"));
        assert!(!headers.contains("te"));
        assert!(headers.contains("content-type"));
        assert_eq!(
            headers
                .get_first("content-length")
                .unwrap()
                .to_str()
                .unwrap(),
            "5"
        );
    }

    #[test]
    fn normalize_metadata_strips_connection_nominated_headers() {
        let mut headers = HeaderBlock::new();
        headers
            .push_str("Connection", "keep-alive, X-Secret")
            .unwrap();
        headers.push_str("X-Secret", "private").unwrap();
        headers.push_str("x-visible", "public").unwrap();

        normalize_metadata(StatusCode::OK, &mut headers, 0).unwrap();

        assert!(!headers.contains("connection"));
        assert!(!headers.contains("x-secret"));
        assert!(headers.contains("x-visible"));
    }

    #[test]
    fn duplicate_content_length_replaced_by_normalized_value() {
        let code = StatusCode::OK;
        let mut headers = HeaderBlock::new();
        headers.push_str("content-length", "999").unwrap();
        headers.push_str("content-length", "888").unwrap();

        normalize_metadata(code, &mut headers, 42).unwrap();

        let all_cl = headers.get_all("content-length");
        assert_eq!(all_cl.len(), 1, "only one Content-Length must remain");
        assert_eq!(all_cl[0].to_str().unwrap(), "42");
    }

    #[test]
    fn duplicate_content_length_rejected_for_not_modified() {
        let mut headers = HeaderBlock::new();
        headers.push_str("content-length", "42").unwrap();
        headers.push_str("Content-Length", "42").unwrap();

        let error = normalize_metadata(StatusCode::NOT_MODIFIED, &mut headers, 42).unwrap_err();
        assert_eq!(
            error,
            ResponseConstructionError::ForbiddenFramingHeader("content-length".to_owned())
        );
    }

    #[test]
    fn transfer_encoding_plus_content_length_strips_te() {
        let resp = Response::builder()
            .status(StatusCode::OK)
            .header("transfer-encoding", "chunked")
            .unwrap()
            .header("content-length", "100")
            .unwrap()
            .body(ResponseBody::Bytes(b"hello".to_vec()))
            .unwrap();

        let req = NormalizeRequest::new(false);
        let normalized = normalize_response(resp, &req).unwrap();

        assert!(!normalized.headers().contains("transfer-encoding"));
        assert_eq!(
            normalized
                .headers()
                .get_first("content-length")
                .unwrap()
                .to_str()
                .unwrap(),
            "5"
        );
    }

    #[test]
    fn normalize_metadata_preserves_duplicate_set_cookie() {
        let code = StatusCode::OK;
        let mut headers = HeaderBlock::new();
        headers.push_str("set-cookie", "a=1").unwrap();
        headers.push_str("set-cookie", "b=2").unwrap();

        normalize_metadata(code, &mut headers, 0).unwrap();

        let all = headers.get_all("set-cookie");
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].to_str().unwrap(), "a=1");
        assert_eq!(all[1].to_str().unwrap(), "b=2");
    }

    #[test]
    fn normalize_metadata_head_preserves_content_length_when_body_nonempty() {
        let code = StatusCode::OK;
        let mut headers = HeaderBlock::new();
        headers.push_str("content-length", "100").unwrap();

        normalize_metadata(code, &mut headers, 100).unwrap();

        assert_eq!(
            headers
                .get_first("content-length")
                .unwrap()
                .to_str()
                .unwrap(),
            "100",
            "HEAD with non-empty body must preserve Content-Length"
        );
    }

    #[test]
    fn normalize_metadata_head_preserves_zero_content_length_when_body_empty() {
        let code = StatusCode::OK;
        let mut headers = HeaderBlock::new();
        headers.push_str("content-length", "100").unwrap();

        normalize_metadata(code, &mut headers, 0).unwrap();

        assert_eq!(
            headers
                .get_first("content-length")
                .unwrap()
                .to_str()
                .unwrap(),
            "0",
            "HEAD with empty body must preserve zero Content-Length"
        );
    }
}

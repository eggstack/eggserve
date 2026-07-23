//! HTTP request handler for static file serving.

use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use hyper::{Method, Request, Response, StatusCode};

use crate::config::ServeState;
use crate::fs::{ResolvedDirectory, ResolvedResource, RootGuard};
use crate::mime::mime_for_path;
use crate::path::{ConfinedPath, PathPolicy};
use crate::policy::{DirectoryListingPolicy, DotfilePolicy};
use crate::primitives::body::BodySource;
use crate::primitives::canonical::{
    normalize_response, NormalizeRequest, Response as CanonicalResponse, ResponseBody,
    StatusCode as CanonicalStatusCode,
};
use crate::primitives::http::ReadOnlyMethod;
use crate::primitives::planner::plan_file_response;
use crate::primitives::response::HeaderMapPlan;
use crate::response::BoxBodyInner;
use crate::response::{
    bad_request, directory_listing_response, file_response, file_response_range, forbidden,
    internal_error, method_not_allowed, not_found, payload_too_large, planned_response,
    service_unavailable,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyRejection {
    InvalidContentLength,
    BodyTooLarge,
    UnsupportedTransferEncoding,
    ConflictingBodyHeaders,
}

pub(crate) fn validate_no_request_body<B>(
    req: &hyper::Request<B>,
    max_body_bytes: u64,
) -> Result<(), BodyRejection> {
    let headers = req.headers();
    let content_length_header = headers.get(hyper::header::CONTENT_LENGTH);
    let transfer_encoding_header = headers.get(hyper::header::TRANSFER_ENCODING);
    let content_length = content_length_header
        .map(|value| {
            value
                .to_str()
                .map_err(|_| BodyRejection::InvalidContentLength)
        })
        .transpose()?;
    let transfer_encoding = transfer_encoding_header
        .map(|value| {
            value
                .to_str()
                .map_err(|_| BodyRejection::UnsupportedTransferEncoding)
        })
        .transpose()?;

    crate::primitives::http::validate_request_body(
        content_length,
        transfer_encoding,
        max_body_bytes,
    )
    .map_err(|error| match error {
        crate::primitives::http::RequestValidationError::InvalidContentLength => {
            BodyRejection::InvalidContentLength
        }
        crate::primitives::http::RequestValidationError::BodyTooLarge => {
            BodyRejection::BodyTooLarge
        }
        crate::primitives::http::RequestValidationError::UnsupportedTransferEncoding => {
            BodyRejection::UnsupportedTransferEncoding
        }
        crate::primitives::http::RequestValidationError::ConflictingBodyHeaders => {
            BodyRejection::ConflictingBodyHeaders
        }
        crate::primitives::http::RequestValidationError::MethodNotAllowed
        | crate::primitives::http::RequestValidationError::InvalidRequestTarget => {
            BodyRejection::InvalidContentLength
        }
    })
}

pub async fn handle_request<B>(req: Request<B>, state: &ServeState) -> Response<BoxBodyInner> {
    let config = &state.config;

    match *req.method() {
        Method::GET | Method::HEAD => {
            let uri = req.uri();
            if uri.authority().is_some() {
                return bad_request();
            }
            let path_str = uri.path();
            let is_head = *req.method() == Method::HEAD;

            if let Err(rejection) =
                validate_no_request_body(&req, config.limits.max_request_body_bytes)
            {
                return match rejection {
                    BodyRejection::BodyTooLarge => payload_too_large(),
                    BodyRejection::InvalidContentLength
                    | BodyRejection::UnsupportedTransferEncoding
                    | BodyRejection::ConflictingBodyHeaders => bad_request(),
                };
            }

            let path_policy = PathPolicy {
                dotfiles: match config.static_policy.dotfiles {
                    DotfilePolicy::Denied => PathPolicy::default().dotfiles,
                    DotfilePolicy::Serve => crate::path::DotfilePolicy::Allow,
                },
                reject_backslash: true,
            };

            let confined = match ConfinedPath::parse(path_str, &path_policy) {
                Ok(p) => p,
                Err(rejection) => {
                    return map_rejection(rejection);
                }
            };

            let guard = RootGuard::new(state.pinned_root());

            let if_none_match = req
                .headers()
                .get(hyper::header::IF_NONE_MATCH)
                .and_then(|v| v.to_str().ok());
            let if_modified_since = req
                .headers()
                .get(hyper::header::IF_MODIFIED_SINCE)
                .and_then(|v| v.to_str().ok());
            let range = req
                .headers()
                .get(hyper::header::RANGE)
                .and_then(|v| v.to_str().ok());
            let if_range = req
                .headers()
                .get(hyper::header::IF_RANGE)
                .and_then(|v| v.to_str().ok());

            match guard.resolve(&confined, &config.static_policy) {
                ResolvedResource::File(file) => {
                    let etag = generate_etag(&file.metadata);
                    let last_modified = file.metadata.modified().ok();
                    let safe_path: PathBuf = file.safe_relative_components.iter().collect();
                    let content_type = mime_for_path(&safe_path);

                    let method = if is_head {
                        ReadOnlyMethod::Head
                    } else {
                        ReadOnlyMethod::Get
                    };

                    let plan = plan_file_response(
                        method,
                        &file.metadata,
                        content_type,
                        if_none_match,
                        if_modified_since,
                        range,
                        if_range,
                    );

                    let status = match plan.status.as_u16() {
                        200 => StatusCode::OK,
                        206 => StatusCode::PARTIAL_CONTENT,
                        304 => StatusCode::NOT_MODIFIED,
                        416 => StatusCode::RANGE_NOT_SATISFIABLE,
                        other => {
                            let _ = other;
                            return internal_error();
                        }
                    };

                    if is_head {
                        return planned_response(status, &plan.headers, true);
                    }

                    let body_source = match file.into_body(&plan) {
                        Ok(bs) => bs,
                        Err(_) => return planned_response(status, &plan.headers, is_head),
                    };
                    body_source_to_response(
                        body_source,
                        status,
                        &plan.headers,
                        etag,
                        last_modified,
                        state,
                    )
                    .await
                }
                ResolvedResource::Directory(dir) => {
                    handle_directory(
                        &dir,
                        config,
                        state,
                        is_head,
                        if_none_match,
                        if_modified_since,
                        range,
                        if_range,
                    )
                    .await
                }
                ResolvedResource::NotFound => {
                    crate::ops::Logger::global().emit(
                        crate::ops::Event::new(
                            crate::ops::Severity::Debug,
                            crate::ops::EventKind::FileNotFound,
                            "file not found",
                        )
                        .field(crate::ops::Field::Str(
                            "path".into(),
                            crate::ops::sanitize_path(path_str),
                        )),
                    );
                    not_found()
                }
                ResolvedResource::Denied(rejection) => {
                    let (event_kind, severity) = match rejection {
                        crate::path::PathRejection::DotfileDenied => (
                            crate::ops::EventKind::DotfileDenied,
                            crate::ops::Severity::Debug,
                        ),
                        crate::path::PathRejection::SymlinkDenied => (
                            crate::ops::EventKind::SymlinkDenied,
                            crate::ops::Severity::Debug,
                        ),
                        crate::path::PathRejection::RootEscapeDenied => (
                            crate::ops::EventKind::RootEscapeDenied,
                            crate::ops::Severity::Warn,
                        ),
                        _ => (
                            crate::ops::EventKind::FileDenied,
                            crate::ops::Severity::Debug,
                        ),
                    };
                    crate::ops::Logger::global().emit(
                        crate::ops::Event::new(severity, event_kind, "access denied").field(
                            crate::ops::Field::Str(
                                "path".into(),
                                crate::ops::sanitize_path(path_str),
                            ),
                        ),
                    );
                    forbidden()
                }
            }
        }
        _ => method_not_allowed(),
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_directory(
    dir: &ResolvedDirectory,
    config: &crate::config::ServeConfig,
    state: &crate::config::ServeState,
    is_head: bool,
    if_none_match: Option<&str>,
    if_modified_since: Option<&str>,
    range: Option<&str>,
    if_range: Option<&str>,
) -> Response<BoxBodyInner> {
    let guard = RootGuard::new(state.pinned_root());

    match guard.resolve_child(dir, "index.html", &config.static_policy) {
        ResolvedResource::File(file) => {
            let etag = generate_etag(&file.metadata);
            let last_modified = file.metadata.modified().ok();
            let safe_path: PathBuf = file.safe_relative_components.iter().collect();
            let content_type = mime_for_path(&safe_path);

            let method = if is_head {
                ReadOnlyMethod::Head
            } else {
                ReadOnlyMethod::Get
            };

            let plan = plan_file_response(
                method,
                &file.metadata,
                content_type,
                if_none_match,
                if_modified_since,
                range,
                if_range,
            );

            let status = match plan.status.as_u16() {
                200 => StatusCode::OK,
                304 => StatusCode::NOT_MODIFIED,
                other => {
                    let _ = other;
                    return internal_error();
                }
            };

            if is_head {
                return planned_response(status, &plan.headers, true);
            }

            let body_source = match file.into_body(&plan) {
                Ok(bs) => bs,
                Err(_) => return planned_response(status, &plan.headers, is_head),
            };
            body_source_to_response(
                body_source,
                status,
                &plan.headers,
                etag,
                last_modified,
                state,
            )
            .await
        }
        ResolvedResource::NotFound => match config.static_policy.directory_listing {
            DirectoryListingPolicy::Enabled => {
                let entries = match guard.list_directory(
                    dir,
                    &config.static_policy,
                    config.limits.max_listing_entries,
                ) {
                    Ok(e) => e,
                    Err(_) => return internal_error(),
                };
                directory_listing_response(&entries, is_head)
            }
            DirectoryListingPolicy::Disabled => forbidden(),
        },
        ResolvedResource::Denied(_) => forbidden(),
        ResolvedResource::Directory(_) => internal_error(),
    }
}

fn generate_etag(metadata: &fs::Metadata) -> Option<String> {
    let size = metadata.len();
    let mtime = metadata.modified().ok()?;
    let epoch = mtime.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    let mtime_secs = epoch.as_secs();
    let mtime_nanos = epoch.subsec_nanos();
    Some(format!("W/\"{}-{}-{}\"", size, mtime_secs, mtime_nanos))
}

fn map_rejection(rejection: crate::path::PathRejection) -> Response<BoxBodyInner> {
    let is_malformed = matches!(
        rejection,
        crate::path::PathRejection::MalformedPercentEncoding
            | crate::path::PathRejection::InvalidUtf8
            | crate::path::PathRejection::NulByte
            | crate::path::PathRejection::Empty
            | crate::path::PathRejection::UnsupportedUriForm
            | crate::path::PathRejection::TooLong
    );

    if is_malformed {
        bad_request()
    } else {
        forbidden()
    }
}

pub(crate) async fn body_source_to_response(
    source: BodySource,
    status: StatusCode,
    headers: &HeaderMapPlan,
    etag: Option<String>,
    last_modified: Option<SystemTime>,
    state: &ServeState,
) -> Response<BoxBodyInner> {
    match source {
        BodySource::Empty => planned_response(status, headers, false),
        BodySource::Bytes(b) => {
            let code = match CanonicalStatusCode::new(status.as_u16()) {
                Ok(c) => c,
                Err(_) => return internal_error(),
            };
            let mut canonical = match CanonicalResponse::builder()
                .status(code)
                .body(ResponseBody::Bytes(b))
            {
                Ok(r) => r,
                Err(_) => return internal_error(),
            };
            for header in headers.iter() {
                if let (Ok(name), Ok(value)) = (
                    crate::primitives::header_block::HeaderName::new(&header.name),
                    crate::primitives::header_block::HeaderValue::new(&header.value),
                ) {
                    canonical.head_mut().headers_mut().push(name, value);
                }
            }
            let req = NormalizeRequest::new(false);
            match normalize_response(canonical, &req) {
                Ok(normalized) => {
                    match crate::primitives::canonical::to_hyper_response(normalized) {
                        Ok(hyper_resp) => hyper_resp,
                        Err(_) => internal_error(),
                    }
                }
                Err(_) => internal_error(),
            }
        }
        BodySource::FileFull { file, len, mime } => {
            let tokio_file = tokio::fs::File::from_std(file);
            let permit = match state.file_stream_semaphore.clone().try_acquire_owned() {
                Ok(p) => p,
                Err(_) => return service_unavailable(),
            };
            file_response(tokio_file, len, mime, last_modified, etag, permit)
        }
        BodySource::FileRange { file, range, .. } => {
            let tokio_file = tokio::fs::File::from_std(file);
            let permit = match state.file_stream_semaphore.clone().try_acquire_owned() {
                Ok(p) => p,
                Err(_) => return service_unavailable(),
            };
            file_response_range(
                tokio_file,
                range.start,
                range.end_inclusive,
                status,
                headers,
                permit,
            )
            .await
        }
    }
}

/// Build a normalized error response through the canonical path.
///
/// This ensures all error responses go through the same normalization rules
/// as handler responses: hop-by-hop stripping, content-length computation,
/// and body-forbidden enforcement.
#[allow(dead_code)]
pub(crate) fn canonical_error_response(
    status: u16,
    body: &'static str,
    is_head: bool,
) -> Response<BoxBodyInner> {
    let code = match CanonicalStatusCode::new(status) {
        Ok(c) => c,
        Err(_) => return internal_error(),
    };
    let body_bytes = body.as_bytes().to_vec();
    let resp = CanonicalResponse::builder()
        .status(code)
        .header("content-type", "text/plain; charset=utf-8")
        .ok()
        .and_then(|b| b.body(ResponseBody::Bytes(body_bytes)).ok());
    match resp {
        Some(r) => {
            let req = NormalizeRequest::new(is_head);
            match normalize_response(r, &req) {
                Ok(normalized) => match crate::primitives::canonical::to_hyper_response(normalized)
                {
                    Ok(hyper_resp) => hyper_resp,
                    Err(_) => internal_error(),
                },
                Err(_) => internal_error(),
            }
        }
        None => internal_error(),
    }
}

/// Normalize a handler response through the canonical path and convert to Hyper.
///
/// This is the standard normalization entry point for Python callback handlers
/// and any future Rust handler integration. It applies:
///
/// 1. HEAD suppression (body discarded, headers preserved)
/// 2. Body-forbidden status enforcement (1xx, 204, 304)
/// 3. Hop-by-hop header stripping
/// 4. Content-Length computation
///
/// Returns a Hyper `Response<BoxBodyInner>` ready for transport.
#[allow(dead_code)]
pub(crate) fn normalize_handler_response(
    status: u16,
    headers: &HeaderMapPlan,
    body_bytes: Vec<u8>,
    is_head: bool,
) -> Response<BoxBodyInner> {
    let code = match CanonicalStatusCode::new(status) {
        Ok(c) => c,
        Err(_) => return internal_error(),
    };

    let mut canonical = match CanonicalResponse::builder()
        .status(code)
        .body(ResponseBody::Bytes(body_bytes))
    {
        Ok(r) => r,
        Err(_) => return internal_error(),
    };

    // Copy headers from HeaderMapPlan into the canonical response.
    for header in headers.iter() {
        if let (Ok(name), Ok(value)) = (
            crate::primitives::header_block::HeaderName::new(&header.name),
            crate::primitives::header_block::HeaderValue::new(&header.value),
        ) {
            canonical.head_mut().headers_mut().push(name, value);
        }
    }

    let req = NormalizeRequest::new(is_head);
    match normalize_response(canonical, &req) {
        Ok(normalized) => match crate::primitives::canonical::to_hyper_response(normalized) {
            Ok(hyper_resp) => hyper_resp,
            Err(_) => internal_error(),
        },
        Err(_) => internal_error(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ServeConfig, ServeState};
    use http_body_util::BodyExt;
    use http_body_util::Empty;
    use hyper::body::Bytes;
    use hyper::StatusCode;
    use std::fs;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn setup_test_state() -> (TempDir, ServeState) {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("hello.txt"), "hello").unwrap();
        fs::write(tmp.path().join(".env"), "secret").unwrap();
        fs::create_dir(tmp.path().join("subdir")).unwrap();
        fs::write(tmp.path().join("subdir").join("file.txt"), "file").unwrap();

        let config = Arc::new(ServeConfig {
            root: tmp.path().to_path_buf(),
            ..ServeConfig::default()
        });
        let state = ServeState::new(config).unwrap();
        (tmp, state)
    }

    fn req_with_path(method: Method, path: &str) -> Request<Empty<Bytes>> {
        Request::builder()
            .method(method)
            .uri(path)
            .body(Empty::new())
            .unwrap()
    }

    #[tokio::test]
    async fn handle_get_existing_file_returns_200() {
        let (_tmp, state) = setup_test_state();
        let resp = handle_request(req_with_path(Method::GET, "/hello.txt"), &state).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "text/plain; charset=utf-8"
        );
        assert_eq!(resp.headers().get("content-length").unwrap(), "5");
    }

    #[tokio::test]
    async fn handle_head_existing_file_returns_200() {
        let (_tmp, state) = setup_test_state();
        let resp = handle_request(req_with_path(Method::HEAD, "/hello.txt"), &state).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers().get("content-length").unwrap(), "5");
    }

    #[tokio::test]
    async fn handle_get_missing_file_returns_404() {
        let (_tmp, state) = setup_test_state();
        let resp = handle_request(req_with_path(Method::GET, "/nope.txt"), &state).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn handle_get_dotfile_returns_403() {
        let (_tmp, state) = setup_test_state();
        let resp = handle_request(req_with_path(Method::GET, "/.env"), &state).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn handle_get_directory_without_index_returns_403() {
        let (_tmp, state) = setup_test_state();
        let resp = handle_request(req_with_path(Method::GET, "/subdir"), &state).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn handle_get_directory_with_index_serves_index() {
        let (_tmp, state) = setup_test_state();
        fs::write(
            state.config.root.join("subdir").join("index.html"),
            "<html>hi</html>",
        )
        .unwrap();
        let resp = handle_request(req_with_path(Method::GET, "/subdir"), &state).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "text/html; charset=utf-8"
        );
    }

    #[tokio::test]
    async fn handle_get_post_returns_405() {
        let (_tmp, state) = setup_test_state();
        let resp = handle_request(req_with_path(Method::POST, "/hello.txt"), &state).await;
        assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(resp.headers().get("allow").unwrap(), "GET, HEAD");
    }

    #[tokio::test]
    async fn handle_get_put_returns_405() {
        let (_tmp, state) = setup_test_state();
        let resp = handle_request(req_with_path(Method::PUT, "/hello.txt"), &state).await;
        assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn handle_get_windows_reserved_returns_403() {
        let (_tmp, state) = setup_test_state();
        let resp = handle_request(req_with_path(Method::GET, "/CON"), &state).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn handle_get_malformed_percent_returns_400() {
        let (_tmp, state) = setup_test_state();
        let resp = handle_request(req_with_path(Method::GET, "/%ZZ"), &state).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn handle_get_etag_and_last_modified_present() {
        let (_tmp, state) = setup_test_state();
        let resp = handle_request(req_with_path(Method::GET, "/hello.txt"), &state).await;
        assert!(resp.headers().get("etag").is_some());
        assert!(resp.headers().get("last-modified").is_some());
        assert_eq!(
            resp.headers().get("x-content-type-options").unwrap(),
            "nosniff"
        );
    }

    #[tokio::test]
    async fn handle_get_nosniff_header() {
        let (_tmp, state) = setup_test_state();
        let resp = handle_request(req_with_path(Method::GET, "/hello.txt"), &state).await;
        assert_eq!(
            resp.headers().get("x-content-type-options").unwrap(),
            "nosniff"
        );
    }

    #[tokio::test]
    async fn handle_get_with_content_length_body_returns_413() {
        let (_tmp, state) = setup_test_state();
        let req = Request::builder()
            .method(Method::GET)
            .uri("/hello.txt")
            .header("content-length", "1024")
            .body(Empty::<Bytes>::new())
            .unwrap();
        let resp = handle_request(req, &state).await;
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn handle_get_with_zero_content_length_allowed() {
        let (_tmp, state) = setup_test_state();
        let req = Request::builder()
            .method(Method::GET)
            .uri("/hello.txt")
            .header("content-length", "0")
            .body(Empty::<Bytes>::new())
            .unwrap();
        let resp = handle_request(req, &state).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn file_stream_exhaustion_returns_503() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("big.txt"), "x").unwrap();
        let config = Arc::new(ServeConfig {
            root: tmp.path().to_path_buf(),
            ..ServeConfig::default()
        });
        let state = ServeState::new(config).unwrap();
        let max = state.config.limits.max_file_streams;
        let mut permits = Vec::with_capacity(max);
        for _ in 0..max {
            permits.push(
                state
                    .file_stream_semaphore
                    .clone()
                    .try_acquire_owned()
                    .unwrap(),
            );
        }
        let resp = handle_request(req_with_path(Method::GET, "/big.txt"), &state).await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        drop(permits);
    }

    #[tokio::test]
    async fn get_content_length_zero_allowed() {
        let (_tmp, state) = setup_test_state();
        let req = Request::builder()
            .method(Method::GET)
            .uri("/hello.txt")
            .header("content-length", "0")
            .body(Empty::<Bytes>::new())
            .unwrap();
        let resp = handle_request(req, &state).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn head_content_length_positive_rejected_413() {
        let (_tmp, state) = setup_test_state();
        let req = Request::builder()
            .method(Method::HEAD)
            .uri("/hello.txt")
            .header("content-length", "1")
            .body(Empty::<Bytes>::new())
            .unwrap();
        let resp = handle_request(req, &state).await;
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn get_invalid_content_length_rejected_400() {
        let (_tmp, state) = setup_test_state();
        let req = Request::builder()
            .method(Method::GET)
            .uri("/hello.txt")
            .header("content-length", "not-a-number")
            .body(Empty::<Bytes>::new())
            .unwrap();
        let resp = handle_request(req, &state).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn get_negative_content_length_rejected_400() {
        let (_tmp, state) = setup_test_state();
        let req = Request::builder()
            .method(Method::GET)
            .uri("/hello.txt")
            .header("content-length", "-1")
            .body(Empty::<Bytes>::new())
            .unwrap();
        let resp = handle_request(req, &state).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn get_overflow_content_length_rejected_400() {
        let (_tmp, state) = setup_test_state();
        let req = Request::builder()
            .method(Method::GET)
            .uri("/hello.txt")
            .header("content-length", "99999999999999999999")
            .body(Empty::<Bytes>::new())
            .unwrap();
        let resp = handle_request(req, &state).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn get_transfer_encoding_chunked_rejected_400() {
        let (_tmp, state) = setup_test_state();
        let req = Request::builder()
            .method(Method::GET)
            .uri("/hello.txt")
            .header("transfer-encoding", "chunked")
            .body(Empty::<Bytes>::new())
            .unwrap();
        let resp = handle_request(req, &state).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn get_content_length_and_transfer_encoding_rejected_400() {
        let (_tmp, state) = setup_test_state();
        let req = Request::builder()
            .method(Method::GET)
            .uri("/hello.txt")
            .header("content-length", "0")
            .header("transfer-encoding", "chunked")
            .body(Empty::<Bytes>::new())
            .unwrap();
        let resp = handle_request(req, &state).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn unsupported_method_with_content_length_still_returns_405() {
        let (_tmp, state) = setup_test_state();
        let req = Request::builder()
            .method(Method::POST)
            .uri("/hello.txt")
            .header("content-length", "1024")
            .body(Empty::<Bytes>::new())
            .unwrap();
        let resp = handle_request(req, &state).await;
        assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn directory_listing_hides_symlink_entries_when_symlinks_denied() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("real.txt"), "real").unwrap();
        std::os::unix::fs::symlink(tmp.path().join("real.txt"), tmp.path().join("link.txt"))
            .unwrap();
        fs::create_dir(tmp.path().join("subdir")).unwrap();

        let config = Arc::new(ServeConfig {
            root: tmp.path().to_path_buf(),
            static_policy: crate::policy::StaticPolicy {
                directory_listing: DirectoryListingPolicy::Enabled,
                ..crate::policy::StaticPolicy::safe_default()
            },
            ..ServeConfig::default()
        });
        let state = ServeState::new(config).unwrap();

        let resp = handle_request(req_with_path(Method::GET, "/"), &state).await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let body_str = std::str::from_utf8(&body).unwrap();
        assert!(
            !body_str.contains("link.txt"),
            "symlink should be hidden: {}",
            body_str
        );
        assert!(
            body_str.contains("real.txt"),
            "real file should be shown: {}",
            body_str
        );
        assert!(
            body_str.contains("subdir"),
            "directory should be shown: {}",
            body_str
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn listing_does_not_classify_symlink_to_dir_as_dir_when_denied() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("real_dir")).unwrap();
        std::os::unix::fs::symlink(tmp.path().join("real_dir"), tmp.path().join("link_dir"))
            .unwrap();

        let config = Arc::new(ServeConfig {
            root: tmp.path().to_path_buf(),
            static_policy: crate::policy::StaticPolicy {
                directory_listing: DirectoryListingPolicy::Enabled,
                ..crate::policy::StaticPolicy::safe_default()
            },
            ..ServeConfig::default()
        });
        let state = ServeState::new(config).unwrap();

        let resp = handle_request(req_with_path(Method::GET, "/"), &state).await;
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let body_str = std::str::from_utf8(&body).unwrap();
        assert!(
            !body_str.contains("link_dir"),
            "symlink-to-dir should be hidden: {}",
            body_str
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn listing_never_contains_symlink_target_path() {
        let tmp = TempDir::new().unwrap();
        std::os::unix::fs::symlink("target.txt", tmp.path().join("link.txt")).unwrap();

        let config = Arc::new(ServeConfig {
            root: tmp.path().to_path_buf(),
            static_policy: crate::policy::StaticPolicy {
                directory_listing: DirectoryListingPolicy::Enabled,
                ..crate::policy::StaticPolicy::safe_default()
            },
            ..ServeConfig::default()
        });
        let state = ServeState::new(config).unwrap();

        let resp = handle_request(req_with_path(Method::GET, "/"), &state).await;
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let body_str = std::str::from_utf8(&body).unwrap();
        assert!(
            !body_str.contains("target.txt"),
            "symlink target should not be exposed: {}",
            body_str
        );
    }

    #[tokio::test]
    async fn handle_get_range_returns_206() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("hello.txt"), "hello world").unwrap();
        let config = Arc::new(ServeConfig {
            root: tmp.path().to_path_buf(),
            ..ServeConfig::default()
        });
        let state = ServeState::new(config).unwrap();

        let req = Request::builder()
            .method(Method::GET)
            .uri("/hello.txt")
            .header("range", "bytes=0-4")
            .body(Empty::<Bytes>::new())
            .unwrap();
        let resp = handle_request(req, &state).await;
        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(resp.headers().get("content-range").unwrap(), "bytes 0-4/11");
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"hello");
    }

    #[tokio::test]
    async fn handle_get_unsatisfiable_range_returns_416() {
        let (_tmp, state) = setup_test_state();
        let req = Request::builder()
            .method(Method::GET)
            .uri("/hello.txt")
            .header("range", "bytes=100-200")
            .body(Empty::<Bytes>::new())
            .unwrap();
        let resp = handle_request(req, &state).await;
        assert_eq!(resp.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    }

    #[tokio::test]
    async fn handle_get_if_none_match_returns_304() {
        let (_tmp, state) = setup_test_state();
        let etag = crate::primitives::planner::generate_etag(
            &fs::metadata(state.config.root.join("hello.txt")).unwrap(),
        )
        .unwrap();

        let req = Request::builder()
            .method(Method::GET)
            .uri("/hello.txt")
            .header("if-none-match", &etag)
            .body(Empty::<Bytes>::new())
            .unwrap();
        let resp = handle_request(req, &state).await;
        assert_eq!(resp.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(resp.headers().get("etag").unwrap(), &etag);
    }

    #[tokio::test]
    async fn handle_get_absolute_form_returns_400() {
        let (_tmp, state) = setup_test_state();
        let req = Request::builder()
            .method(Method::GET)
            .uri("http://example.com/hello.txt")
            .body(Empty::<Bytes>::new())
            .unwrap();
        let resp = handle_request(req, &state).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn handle_head_range_returns_206_empty_body() {
        let (_tmp, state) = setup_test_state();
        let req = Request::builder()
            .method(Method::HEAD)
            .uri("/hello.txt")
            .header("range", "bytes=0-2")
            .body(Empty::<Bytes>::new())
            .unwrap();
        let resp = handle_request(req, &state).await;
        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(resp.headers().get("content-length").unwrap(), "3");
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert!(body.is_empty());
    }
}

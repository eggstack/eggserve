//! HTTP request handler for static file serving.

use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use hyper::{Method, Request, Response, StatusCode};

use crate::config::ServeState;
use crate::fs::{ResolvedDirectory, ResolvedFile, ResolvedResource, RootGuard};
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

/// Typed request input for static-file response planning.
///
/// Both direct-file and directory-index routes construct this identically
/// from the canonical request, ensuring conditional and range headers are
/// never silently dropped.
pub(crate) struct StaticRequestInput<'a> {
    pub method: ReadOnlyMethod,
    pub if_none_match: Option<&'a str>,
    pub if_modified_since: Option<&'a str>,
    pub range: Option<&'a str>,
    pub if_range: Option<&'a str>,
}

/// Serve a resolved file through the canonical planner and body-construction path.
///
/// This is the single entry point for both direct-file and directory-index routes.
/// It applies conditional/range planning, constructs the response body from the
/// opened handle, and normalizes the response through the canonical path.
async fn serve_resolved_file(
    file: ResolvedFile,
    input: &StaticRequestInput<'_>,
    state: &ServeState,
) -> Response<BoxBodyInner> {
    let etag = generate_etag(&file.metadata);
    let last_modified = file.metadata.modified().ok();
    let safe_path: PathBuf = file.safe_relative_components.iter().collect();
    let content_type = mime_for_path(&safe_path);

    let plan = plan_file_response(
        input.method,
        &file.metadata,
        content_type,
        input.if_none_match,
        input.if_modified_since,
        input.range,
        input.if_range,
    );

    let status = match plan.status.as_u16() {
        200 => StatusCode::OK,
        206 => StatusCode::PARTIAL_CONTENT,
        304 => StatusCode::NOT_MODIFIED,
        416 => StatusCode::RANGE_NOT_SATISFIABLE,
        _ => return internal_error(),
    };

    let is_head = input.method == ReadOnlyMethod::Head;
    if is_head {
        return planned_response(status, &plan.headers, true);
    }

    let body_source = match file.into_body(&plan) {
        Ok(bs) => bs,
        Err(e) => {
            crate::ops::Logger::global().emit(crate::ops::Event::new(
                crate::ops::Severity::Warn,
                crate::ops::EventKind::FileError,
                format!("file body conversion failed: {e}"),
            ));
            return internal_error();
        }
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
    handle_request_with_metadata(
        req,
        state,
        std::net::SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 0),
        std::net::SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), 0),
        None,
    )
    .await
}

/// Handle an HTTP request with real connection metadata.
///
/// This is identical to [`handle_request`] but accepts transport-level metadata
/// (local/remote addresses, TLS info) that may be used for logging or diagnostics.
/// The metadata is captured at accept time and reflects the actual transport peer,
/// not end-client identity behind a reverse proxy.
pub async fn handle_request_with_metadata<B>(
    req: Request<B>,
    state: &ServeState,
    _local_addr: std::net::SocketAddr,
    _remote_addr: std::net::SocketAddr,
    _tls_info: Option<crate::primitives::connection_info::TlsInfo>,
) -> Response<BoxBodyInner> {
    let config = &state.config;

    match *req.method() {
        Method::GET | Method::HEAD => {
            let uri = req.uri();
            let is_head = *req.method() == Method::HEAD;
            if uri.authority().is_some() {
                return bad_request(is_head);
            }
            let path_str = uri.path();

            if let Err(rejection) =
                validate_no_request_body(&req, config.limits.max_request_body_bytes)
            {
                return match rejection {
                    BodyRejection::BodyTooLarge => payload_too_large(is_head),
                    BodyRejection::InvalidContentLength
                    | BodyRejection::UnsupportedTransferEncoding
                    | BodyRejection::ConflictingBodyHeaders => bad_request(is_head),
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
                    return map_rejection(rejection, is_head);
                }
            };

            let guard = RootGuard::new(state.pinned_root());

            let method = if is_head {
                ReadOnlyMethod::Head
            } else {
                ReadOnlyMethod::Get
            };

            let input = StaticRequestInput {
                method,
                if_none_match: req
                    .headers()
                    .get(hyper::header::IF_NONE_MATCH)
                    .and_then(|v| v.to_str().ok()),
                if_modified_since: req
                    .headers()
                    .get(hyper::header::IF_MODIFIED_SINCE)
                    .and_then(|v| v.to_str().ok()),
                range: req
                    .headers()
                    .get(hyper::header::RANGE)
                    .and_then(|v| v.to_str().ok()),
                if_range: req
                    .headers()
                    .get(hyper::header::IF_RANGE)
                    .and_then(|v| v.to_str().ok()),
            };

            match guard.resolve(&confined, &config.static_policy) {
                ResolvedResource::File(file) => serve_resolved_file(file, &input, state).await,
                ResolvedResource::Directory(dir) => {
                    handle_directory(&dir, config, state, &input).await
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
                    not_found(is_head)
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
                    forbidden(is_head)
                }
            }
        }
        _ => method_not_allowed(false),
    }
}

async fn handle_directory(
    dir: &ResolvedDirectory,
    config: &crate::config::ServeConfig,
    state: &crate::config::ServeState,
    input: &StaticRequestInput<'_>,
) -> Response<BoxBodyInner> {
    let guard = RootGuard::new(state.pinned_root());

    let is_head = input.method == ReadOnlyMethod::Head;

    // Try index.html first, then index.htm as fallback.
    let index_candidate = match guard.resolve_child(dir, "index.html", &config.static_policy) {
        ResolvedResource::File(file) => {
            return serve_resolved_file(file, input, state).await;
        }
        ResolvedResource::NotFound => {
            // Try index.htm
            match guard.resolve_child(dir, "index.htm", &config.static_policy) {
                ResolvedResource::File(file) => {
                    return serve_resolved_file(file, input, state).await;
                }
                other => other,
            }
        }
        other => other,
    };

    match index_candidate {
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
                directory_listing_response(
                    &entries,
                    is_head,
                    config.limits.max_listing_response_bytes,
                )
            }
            DirectoryListingPolicy::Disabled => forbidden(is_head),
        },
        ResolvedResource::Denied(_) => forbidden(is_head),
        ResolvedResource::Directory(_) => internal_error(),
        ResolvedResource::File(_) => unreachable!(),
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

fn map_rejection(rejection: crate::path::PathRejection, is_head: bool) -> Response<BoxBodyInner> {
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
        bad_request(is_head)
    } else {
        forbidden(is_head)
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
            let permit = match state
                .legacy_file_stream_semaphore()
                .clone()
                .try_acquire_owned()
            {
                Ok(p) => p,
                Err(_) => return service_unavailable(),
            };
            let chunk_size = state.config.limits.stream_chunk_size;
            file_response(
                tokio_file,
                len,
                mime,
                last_modified,
                etag,
                permit,
                chunk_size,
            )
        }
        BodySource::FileRange { file, range, .. } => {
            let tokio_file = tokio::fs::File::from_std(file);
            let permit = match state
                .legacy_file_stream_semaphore()
                .clone()
                .try_acquire_owned()
            {
                Ok(p) => p,
                Err(_) => return service_unavailable(),
            };
            let chunk_size = state.config.limits.stream_chunk_size;
            file_response_range(
                tokio_file,
                range.start,
                range.end_inclusive,
                status,
                headers,
                permit,
                chunk_size,
            )
            .await
        }
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
                    .legacy_file_stream_semaphore
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

    // -----------------------------------------------------------------------
    // Plan 082 — HEAD error body tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn head_404_returns_no_body() {
        let (_tmp, state) = setup_test_state();
        let resp = handle_request(req_with_path(Method::HEAD, "/nope.txt"), &state).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert!(body.is_empty(), "HEAD 404 should have no body");
    }

    #[tokio::test]
    async fn head_403_returns_no_body() {
        let (_tmp, state) = setup_test_state();
        let resp = handle_request(req_with_path(Method::HEAD, "/.env"), &state).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert!(body.is_empty(), "HEAD 403 should have no body");
    }

    #[tokio::test]
    async fn head_405_returns_no_body() {
        // Note: HEAD to a valid file returns 200, not 405. This test verifies
        // that POST to a file returns 405 WITH a body (non-HEAD error).
        let (_tmp, state) = setup_test_state();
        let resp = handle_request(req_with_path(Method::POST, "/hello.txt"), &state).await;
        assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        // POST 405 should have a body — only HEAD suppresses it
        assert!(!body.is_empty(), "POST 405 should have a body");
    }

    #[tokio::test]
    async fn head_413_returns_no_body() {
        let (_tmp, state) = setup_test_state();
        let req = Request::builder()
            .method(Method::HEAD)
            .uri("/hello.txt")
            .header("content-length", "1024")
            .body(Empty::<Bytes>::new())
            .unwrap();
        let resp = handle_request(req, &state).await;
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert!(body.is_empty(), "HEAD 413 should have no body");
    }

    #[tokio::test]
    async fn head_error_matches_get_status() {
        let (_tmp, state) = setup_test_state();

        // 404
        let get = handle_request(req_with_path(Method::GET, "/nope.txt"), &state).await;
        let head = handle_request(req_with_path(Method::HEAD, "/nope.txt"), &state).await;
        assert_eq!(get.status(), head.status());

        // 403
        let get = handle_request(req_with_path(Method::GET, "/.env"), &state).await;
        let head = handle_request(req_with_path(Method::HEAD, "/.env"), &state).await;
        assert_eq!(get.status(), head.status());

        // 405
        let get = handle_request(req_with_path(Method::POST, "/hello.txt"), &state).await;
        let head = handle_request(req_with_path(Method::HEAD, "/hello.txt"), &state).await;
        assert_eq!(StatusCode::METHOD_NOT_ALLOWED, get.status());
        // HEAD to existing file returns 200, not 405 — this is correct behavior
        assert_eq!(StatusCode::OK, head.status());
    }
}

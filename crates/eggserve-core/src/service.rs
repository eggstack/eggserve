//! HTTP request handler for static file serving.

use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use hyper::{Method, Request, Response};

use crate::config::ServeState;
use crate::fs::{ResolvedDirectory, ResolvedResource, RootGuard};
use crate::mime::mime_for_path;
use crate::path::{ConfinedPath, PathPolicy};
use crate::policy::{DirectoryListingPolicy, DotfilePolicy};
use crate::response::BoxBodyInner;
use crate::response::{
    bad_request, directory_listing_response, file_response, file_response_head, forbidden,
    internal_error, method_not_allowed, not_found, payload_too_large, service_unavailable,
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

    if content_length_header.is_some() && transfer_encoding_header.is_some() {
        return Err(BodyRejection::ConflictingBodyHeaders);
    }

    if let Some(te) = transfer_encoding_header {
        let value = te
            .to_str()
            .map_err(|_| BodyRejection::UnsupportedTransferEncoding)?;
        if !value.trim().is_empty() {
            return Err(BodyRejection::UnsupportedTransferEncoding);
        }
    }

    if let Some(cl) = content_length_header {
        let value = cl
            .to_str()
            .map_err(|_| BodyRejection::InvalidContentLength)?;
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(BodyRejection::InvalidContentLength);
        }
        if !trimmed.chars().all(|c| c.is_ascii_digit()) {
            return Err(BodyRejection::InvalidContentLength);
        }
        let len: u64 = trimmed
            .parse()
            .map_err(|_| BodyRejection::InvalidContentLength)?;
        if len > max_body_bytes {
            return Err(BodyRejection::BodyTooLarge);
        }
    }

    Ok(())
}

pub async fn handle_request<B>(req: Request<B>, state: &ServeState) -> Response<BoxBodyInner> {
    let config = &state.config;

    match *req.method() {
        Method::GET | Method::HEAD => {
            let uri = req.uri();
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

            let guard = match RootGuard::new(&config.root) {
                Ok(g) => g,
                Err(_) => return internal_error(),
            };

            match guard.resolve(&confined, &config.static_policy) {
                ResolvedResource::File(file) => {
                    let etag = generate_etag(&file.metadata);
                    let last_modified = file.metadata.modified().ok();
                    let safe_path: PathBuf = file.safe_relative_components.iter().collect();
                    let content_type = mime_for_path(&safe_path);
                    let len = file.metadata.len();

                    if is_head {
                        return file_response_head(len, content_type, last_modified, etag);
                    }

                    let tokio_file = tokio::fs::File::from_std(file.file);

                    let permit = match state.file_stream_semaphore.clone().try_acquire_owned() {
                        Ok(p) => p,
                        Err(_) => return service_unavailable(),
                    };

                    file_response(tokio_file, len, content_type, last_modified, etag, permit)
                }
                ResolvedResource::Directory(dir) => {
                    handle_directory(&dir, config, state, is_head).await
                }
                ResolvedResource::NotFound => not_found(),
                ResolvedResource::Denied(_) => forbidden(),
            }
        }
        _ => method_not_allowed(),
    }
}

async fn handle_directory(
    dir: &ResolvedDirectory,
    config: &crate::config::ServeConfig,
    state: &crate::config::ServeState,
    is_head: bool,
) -> Response<BoxBodyInner> {
    let guard = match RootGuard::new(&config.root) {
        Ok(g) => g,
        Err(_) => return internal_error(),
    };

    match guard.resolve_child(dir, "index.html", &config.static_policy) {
        ResolvedResource::File(file) => {
            let etag = generate_etag(&file.metadata);
            let last_modified = file.metadata.modified().ok();
            let safe_path: PathBuf = file.safe_relative_components.iter().collect();
            let content_type = mime_for_path(&safe_path);
            let len = file.metadata.len();

            if is_head {
                return file_response_head(len, content_type, last_modified, etag);
            }

            let tokio_file = tokio::fs::File::from_std(file.file);

            let permit = match state.file_stream_semaphore.clone().try_acquire_owned() {
                Ok(p) => p,
                Err(_) => return service_unavailable(),
            };

            file_response(tokio_file, len, content_type, last_modified, etag, permit)
        }
        ResolvedResource::NotFound => match config.static_policy.directory_listing {
            DirectoryListingPolicy::Enabled => {
                let entries = match guard.list_directory(dir, &config.static_policy) {
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
    Some(format!("W/\"{}-{}\"", size, mtime_secs))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ServeConfig, ServeState};
    #[cfg(unix)]
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
        let state = ServeState::new(config);
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
        let state = ServeState::new(config);
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
        let state = ServeState::new(config);

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
        let state = ServeState::new(config);

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
        let state = ServeState::new(config);

        let resp = handle_request(req_with_path(Method::GET, "/"), &state).await;
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let body_str = std::str::from_utf8(&body).unwrap();
        assert!(
            !body_str.contains("target.txt"),
            "symlink target should not be exposed: {}",
            body_str
        );
    }
}

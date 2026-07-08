use std::fs;
use std::time::SystemTime;

use hyper::{Method, Request, Response};

use crate::config::ServeState;
use crate::fs::{ResolvedResource, RootGuard};
use crate::mime::mime_for_path;
use crate::path::{ConfinedPath, PathPolicy};
use crate::policy::{DirectoryListingPolicy, DotfilePolicy};
use crate::response::BoxBodyInner;
use crate::response::{
    bad_request, directory_listing_response, file_response, file_response_head, forbidden,
    internal_error, method_not_allowed, not_found, payload_too_large, service_unavailable,
};

pub async fn handle_request<B>(req: Request<B>, state: &ServeState) -> Response<BoxBodyInner> {
    let config = &state.config;

    match *req.method() {
        Method::GET | Method::HEAD => {
            let uri = req.uri();
            let path_str = uri.path();
            let is_head = *req.method() == Method::HEAD;

            if let Some(content_length) = req.headers().get("content-length") {
                if let Ok(len) = content_length.to_str().unwrap_or("0").parse::<u64>() {
                    if len > config.limits.max_request_body_bytes {
                        return payload_too_large();
                    }
                }
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
                    let content_type = mime_for_path(&file.path);
                    let len = file.metadata.len();

                    if is_head {
                        return file_response_head(len, content_type, last_modified, etag);
                    }

                    let tokio_file = match tokio::fs::File::open(&file.path).await {
                        Ok(f) => f,
                        Err(_) => return internal_error(),
                    };

                    let permit = match state.file_stream_semaphore.clone().try_acquire_owned() {
                        Ok(p) => p,
                        Err(_) => return service_unavailable(),
                    };

                    file_response(tokio_file, len, content_type, last_modified, etag, permit)
                }
                ResolvedResource::Directory(dir) => {
                    handle_directory(&dir.path, config, state, is_head).await
                }
                ResolvedResource::NotFound => not_found(),
                ResolvedResource::Denied(_) => forbidden(),
            }
        }
        _ => method_not_allowed(),
    }
}

async fn handle_directory(
    dir_path: &std::path::Path,
    config: &crate::config::ServeConfig,
    state: &crate::config::ServeState,
    is_head: bool,
) -> Response<BoxBodyInner> {
    let guard = match RootGuard::new(&config.root) {
        Ok(g) => g,
        Err(_) => return internal_error(),
    };

    match guard.resolve_index_at(dir_path, &config.static_policy) {
        ResolvedResource::File(file) => {
            let etag = generate_etag(&file.metadata);
            let last_modified = file.metadata.modified().ok();
            let content_type = mime_for_path(&file.path);
            let len = file.metadata.len();

            if is_head {
                return file_response_head(len, content_type, last_modified, etag);
            }

            let tokio_file = match tokio::fs::File::open(&file.path).await {
                Ok(f) => f,
                Err(_) => return internal_error(),
            };

            let permit = match state.file_stream_semaphore.clone().try_acquire_owned() {
                Ok(p) => p,
                Err(_) => return service_unavailable(),
            };

            file_response(tokio_file, len, content_type, last_modified, etag, permit)
        }
        ResolvedResource::NotFound => match config.static_policy.directory_listing {
            DirectoryListingPolicy::Enabled => {
                let entries = match build_listing_entries(dir_path, &config.static_policy) {
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

fn build_listing_entries(
    dir: &std::path::Path,
    policy: &crate::policy::StaticPolicy,
) -> Result<Vec<(String, bool)>, std::io::Error> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();

        if policy.dotfiles == DotfilePolicy::Denied && name.starts_with('.') {
            continue;
        }

        let is_dir = entry.metadata()?.is_dir();
        entries.push((name, is_dir));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
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
}

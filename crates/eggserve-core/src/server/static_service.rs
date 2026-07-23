//! Hardened static file service.
//!
//! [`StaticService`] wraps the existing static file handling logic into a
//! reusable [`Service`] implementation. It preserves all security properties:
//!
//! - Descriptor-relative path confinement on Unix
//! - Dotfile, symlink, and directory-listing policy enforcement
//! - Request body rejection for GET/HEAD
//! - Conditional and range request handling
//! - ETag and Last-Modified generation
//! - File-stream semaphore-gated concurrency
//!
//! # Example
//!
//! ```ignore
//! use eggserve_core::server::{StaticService, RuntimeConfig};
//! use eggserve_core::policy::StaticPolicy;
//!
//! let service = StaticService::builder("/var/www")
//!     .policy(StaticPolicy::safe_default())
//!     .build()
//!     .unwrap();
//! ```

use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::SystemTime;

use crate::config::ServeState;
use crate::fs::{ResolvedDirectory, ResolvedResource, RootGuard};
use crate::mime::mime_for_path;
use crate::path::{ConfinedPath, PathPolicy};
use crate::policy::{DirectoryListingPolicy, DotfilePolicy, StaticPolicy};
use crate::primitives::body::BodySource;
use crate::primitives::canonical::{
    normalize_response, NormalizeRequest, Response as CanonicalResponse, ResponseBody,
    StatusCode as CanonicalStatusCode,
};
use crate::primitives::header_block::HeaderBlock;
use crate::primitives::http::ReadOnlyMethod;
use crate::primitives::planner::plan_file_response;
use crate::primitives::request::Request;
use crate::primitives::request_head::RequestHead;
use crate::primitives::response::HeaderMapPlan;
use crate::response::{
    bad_request, directory_listing_response, file_response, file_response_range, forbidden,
    internal_error, method_not_allowed, not_found, planned_response, service_unavailable,
    BoxBodyInner,
};
use crate::server::service::{Service, ServiceError};

/// Builder for constructing a [`StaticService`].
#[derive(Debug)]
#[must_use]
pub struct StaticServiceBuilder {
    root: PathBuf,
    policy: StaticPolicy,
    max_file_streams: usize,
}

impl StaticServiceBuilder {
    /// Set the security policy for filesystem access.
    pub fn policy(mut self, policy: StaticPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Set the maximum number of concurrent file streams.
    ///
    /// This overrides the runtime default for this specific service instance.
    pub fn max_file_streams(mut self, max: usize) -> Self {
        self.max_file_streams = max;
        self
    }

    /// Build the static service.
    ///
    /// Validates that the root directory exists and is accessible.
    pub fn build(self) -> Result<StaticService, ServiceError> {
        if !self.root.is_dir() {
            return Err(ServiceError::internal(format!(
                "root directory does not exist or is not a directory: {}",
                self.root.display()
            )));
        }
        let config = Arc::new(crate::config::ServeConfig {
            root: self.root,
            limits: crate::limits::Limits {
                max_file_streams: self.max_file_streams,
                ..Default::default()
            },
            static_policy: self.policy,
            ..Default::default()
        });
        let state = Arc::new(ServeState::new(config).map_err(|e| {
            ServiceError::internal(format!("failed to initialize serve state: {e}"))
        })?);
        Ok(StaticService { state })
    }
}

/// A hardened static file service.
///
/// Implements [`Service`] and handles GET/HEAD requests against a rooted
/// directory tree with full path confinement and policy enforcement.
pub struct StaticService {
    state: Arc<ServeState>,
}

impl StaticService {
    /// Create a builder for a static service rooted at the given path.
    pub fn builder(root: impl AsRef<Path>) -> StaticServiceBuilder {
        StaticServiceBuilder {
            root: root.as_ref().to_path_buf(),
            policy: StaticPolicy::safe_default(),
            max_file_streams: 32,
        }
    }

    /// Create a static service from an existing [`ServeState`].
    ///
    /// This is used internally by the runtime when the CLI or Python frontend
    /// provides a pre-built configuration.
    #[allow(dead_code)]
    pub(crate) fn from_state(state: Arc<ServeState>) -> Self {
        Self { state }
    }

    /// Handle a single request against the static root.
    pub async fn handle(
        &self,
        req: Request,
    ) -> Result<hyper::Response<BoxBodyInner>, ServiceError> {
        let (head, _body) = req.into_head_and_body();
        Ok(handle_static_request(head, &self.state).await)
    }
}

impl Service for StaticService {
    fn request_body_policy(
        &self,
        _head: &RequestHead,
    ) -> crate::primitives::request_body_policy::RequestBodyPolicy {
        crate::primitives::request_body_policy::RequestBodyPolicy::Reject
    }

    fn call(
        &self,
        req: Request,
    ) -> Pin<
        Box<dyn std::future::Future<Output = Result<CanonicalResponse, ServiceError>> + Send + '_>,
    > {
        let state = self.state.clone();
        let (head, _body) = req.into_head_and_body();
        Box::pin(async move {
            let hyper_resp = handle_static_request(head, &state).await;
            // Convert hyper response to canonical response for the public boundary.
            // This is a lossy conversion — the canonical response carries only
            // in-memory bodies. For file-backed responses, the runtime intercepts
            // before this point and streams directly.
            //
            // In practice, the runtime's connection pipeline handles file streaming
            // directly, so this conversion is only hit for error/empty responses
            // from the static service.
            let status = hyper_resp.status().as_u16();
            let code = CanonicalStatusCode::new(status)
                .map_err(|_| ServiceError::internal("invalid status code"))?;
            let mut headers = HeaderBlock::new();
            for (name, value) in hyper_resp.headers().iter() {
                if let (Ok(n), Ok(v)) = (
                    crate::primitives::header_block::HeaderName::new(name.as_str()),
                    crate::primitives::header_block::HeaderValue::new(value.to_str().unwrap_or("")),
                ) {
                    headers.push(n, v);
                }
            }
            Ok(CanonicalResponse::builder()
                .status(code)
                .body(ResponseBody::Empty)
                .unwrap())
        })
    }
}

async fn handle_static_request(
    req: RequestHead,
    state: &ServeState,
) -> hyper::Response<BoxBodyInner> {
    let config = &state.config;

    let method = req.method();
    let is_head = method.is_head();

    if !method.is_get() && !is_head {
        return method_not_allowed();
    }

    let target = req.target();
    let path_str = target.path();

    // Reject absolute-form URIs (authority present in raw target).
    if target.raw().contains("://") {
        return bad_request();
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
        Err(rejection) => return map_rejection(rejection),
    };

    let guard = RootGuard::new(state.pinned_root());

    let if_none_match = req.headers().get_first("if-none-match").map(|v| v.as_str());
    let if_modified_since = req
        .headers()
        .get_first("if-modified-since")
        .map(|v| v.as_str());
    let range = req.headers().get_first("range").map(|v| v.as_str());
    let if_range = req.headers().get_first("if-range").map(|v| v.as_str());

    match guard.resolve(&confined, &config.static_policy) {
        ResolvedResource::File(file) => {
            let etag = generate_etag(&file.metadata);
            let last_modified = file.metadata.modified().ok();
            let safe_path: PathBuf = file.safe_relative_components.iter().collect();
            let content_type = mime_for_path(&safe_path);

            let read_only_method = if is_head {
                ReadOnlyMethod::Head
            } else {
                ReadOnlyMethod::Get
            };

            let plan = plan_file_response(
                read_only_method,
                &file.metadata,
                content_type,
                if_none_match,
                if_modified_since,
                range,
                if_range,
            );

            let status = match plan.status.as_u16() {
                200 => hyper::StatusCode::OK,
                206 => hyper::StatusCode::PARTIAL_CONTENT,
                304 => hyper::StatusCode::NOT_MODIFIED,
                416 => hyper::StatusCode::RANGE_NOT_SATISFIABLE,
                _ => return internal_error(),
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
                    crate::ops::Field::Str("path".into(), crate::ops::sanitize_path(path_str)),
                ),
            );
            forbidden()
        }
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
) -> hyper::Response<BoxBodyInner> {
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
                200 => hyper::StatusCode::OK,
                206 => hyper::StatusCode::PARTIAL_CONTENT,
                304 => hyper::StatusCode::NOT_MODIFIED,
                416 => hyper::StatusCode::RANGE_NOT_SATISFIABLE,
                _ => return internal_error(),
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

fn generate_etag(metadata: &std::fs::Metadata) -> Option<String> {
    let size = metadata.len();
    let mtime = metadata.modified().ok()?;
    let epoch = mtime.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    let mtime_secs = epoch.as_secs();
    let mtime_nanos = epoch.subsec_nanos();
    Some(format!("W/\"{}-{}-{}\"", size, mtime_secs, mtime_nanos))
}

fn map_rejection(rejection: crate::path::PathRejection) -> hyper::Response<BoxBodyInner> {
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

async fn body_source_to_response(
    source: BodySource,
    status: hyper::StatusCode,
    headers: &HeaderMapPlan,
    etag: Option<String>,
    last_modified: Option<SystemTime>,
    state: &ServeState,
) -> hyper::Response<BoxBodyInner> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_static_service() -> (TempDir, StaticService) {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("hello.txt"), "hello").unwrap();
        std::fs::write(tmp.path().join(".env"), "secret").unwrap();
        std::fs::create_dir(tmp.path().join("subdir")).unwrap();

        let service = StaticService::builder(tmp.path()).build().unwrap();
        (tmp, service)
    }

    fn make_head(path: &str) -> Request {
        Request::new(
            RequestHead::new(
                crate::primitives::method::Method::head(),
                crate::primitives::request_target::RequestTarget::parse(path).unwrap(),
                crate::primitives::version::HttpVersion::Http11,
                HeaderBlock::new(),
            ),
            crate::primitives::request_body::RequestBody::empty(),
            crate::primitives::connection_info::ConnectionInfo {
                local_addr: "127.0.0.1:8000".parse().unwrap(),
                remote_addr: "127.0.0.1:12345".parse().unwrap(),
                scheme: crate::primitives::connection_info::Scheme::Http,
                tls: None,
            },
        )
    }

    fn make_get(path: &str) -> Request {
        Request::new(
            RequestHead::new(
                crate::primitives::method::Method::get(),
                crate::primitives::request_target::RequestTarget::parse(path).unwrap(),
                crate::primitives::version::HttpVersion::Http11,
                HeaderBlock::new(),
            ),
            crate::primitives::request_body::RequestBody::empty(),
            crate::primitives::connection_info::ConnectionInfo {
                local_addr: "127.0.0.1:8000".parse().unwrap(),
                remote_addr: "127.0.0.1:12345".parse().unwrap(),
                scheme: crate::primitives::connection_info::Scheme::Http,
                tls: None,
            },
        )
    }

    #[tokio::test]
    async fn static_service_get_existing_file() {
        let (_tmp, service) = setup_static_service();
        let resp = service.handle(make_get("/hello.txt")).await.unwrap();
        assert_eq!(resp.status(), hyper::StatusCode::OK);
    }

    #[tokio::test]
    async fn static_service_head_existing_file() {
        let (_tmp, service) = setup_static_service();
        let resp = service.handle(make_head("/hello.txt")).await.unwrap();
        assert_eq!(resp.status(), hyper::StatusCode::OK);
    }

    #[tokio::test]
    async fn static_service_get_missing_file() {
        let (_tmp, service) = setup_static_service();
        let resp = service.handle(make_get("/nope.txt")).await.unwrap();
        assert_eq!(resp.status(), hyper::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn static_service_get_dotfile_forbidden() {
        let (_tmp, service) = setup_static_service();
        let resp = service.handle(make_get("/.env")).await.unwrap();
        assert_eq!(resp.status(), hyper::StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn static_service_post_forbidden() {
        let (_tmp, service) = setup_static_service();
        let req = Request::new(
            RequestHead::new(
                crate::primitives::method::Method::post(),
                crate::primitives::request_target::RequestTarget::parse("/hello.txt").unwrap(),
                crate::primitives::version::HttpVersion::Http11,
                HeaderBlock::new(),
            ),
            crate::primitives::request_body::RequestBody::empty(),
            crate::primitives::connection_info::ConnectionInfo {
                local_addr: "127.0.0.1:8000".parse().unwrap(),
                remote_addr: "127.0.0.1:12345".parse().unwrap(),
                scheme: crate::primitives::connection_info::Scheme::Http,
                tls: None,
            },
        );
        let resp = service.handle(req).await.unwrap();
        assert_eq!(resp.status(), hyper::StatusCode::METHOD_NOT_ALLOWED);
    }

    #[test]
    fn static_service_builder_root_must_exist() {
        let result = StaticService::builder("/nonexistent/path/12345").build();
        assert!(result.is_err());
    }
}

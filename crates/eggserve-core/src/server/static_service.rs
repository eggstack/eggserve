//! Canonical, confined static-file service.
//!
//! Static resolution and response planning happen here. The service never
//! constructs a Hyper response and never acquires transport permits. File
//! capabilities remain in [`ResponseBody::File`] until the runtime's single
//! transport conversion boundary.

use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use crate::config::{ServeConfig, ServeState};
use crate::fs::{ResolvedDirectory, ResolvedResource, RootGuard};
use crate::path::{ConfinedPath, PathPolicy};
use crate::policy::{DirectoryListingPolicy, DotfilePolicy, StaticPolicy};
use crate::primitives::body::BodySource;
use crate::primitives::canonical::{
    normalize_response, NormalizeRequest, Response as CanonicalResponse, ResponseBody, StatusCode,
};
use crate::primitives::header_block::{HeaderName, HeaderValue};
use crate::primitives::http::ReadOnlyMethod;
use crate::primitives::planner::plan_file_response_with_preconditions;
use crate::primitives::request::Request;
use crate::primitives::request_head::RequestHead;
use crate::primitives::response::HeaderMapPlan;
use crate::server::service::{Service, ServiceError};

/// Builder for a confined static service.
#[derive(Debug)]
#[must_use]
pub struct StaticServiceBuilder {
    root: PathBuf,
    policy: StaticPolicy,
    default_content_type: String,
    extra_response_headers: Vec<(String, String)>,
}

impl StaticServiceBuilder {
    /// Set the static-file security policy.
    pub fn policy(mut self, policy: StaticPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Set the fallback type used when a file extension has no known MIME type.
    pub fn default_content_type(mut self, content_type: impl Into<String>) -> Self {
        self.default_content_type = content_type.into();
        self
    }

    /// Set ordered, duplicate-preserving headers for successful static 200 responses.
    pub fn extra_response_headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.extra_response_headers = headers;
        self
    }

    /// Build the service and pin its root exactly once.
    pub fn build(self) -> Result<StaticService, ServiceError> {
        let config = Arc::new(ServeConfig {
            root: self.root,
            static_policy: self.policy,
            default_content_type: self.default_content_type,
            extra_response_headers: self.extra_response_headers,
            ..ServeConfig::default()
        });
        StaticService::from_serve_config(config)
            .map_err(|e| ServiceError::internal(format!("failed to initialize static root: {e}")))
    }
}

/// A hardened static file service.
#[derive(Clone)]
pub struct StaticService {
    state: Arc<ServeState>,
}

impl StaticService {
    /// Create a builder for a static service rooted at `root`.
    pub fn builder(root: impl AsRef<Path>) -> StaticServiceBuilder {
        StaticServiceBuilder {
            root: root.as_ref().to_path_buf(),
            policy: StaticPolicy::safe_default(),
            default_content_type: "application/octet-stream".to_string(),
            extra_response_headers: Vec::new(),
        }
    }

    /// Construct a service from already validated static configuration.
    pub(crate) fn from_serve_config(config: Arc<ServeConfig>) -> Result<Self, std::io::Error> {
        let state = Arc::new(ServeState::new(config)?);
        crate::ops::Logger::global().emit(crate::ops::Event::new(
            crate::ops::Severity::Info,
            crate::ops::EventKind::RootInitialized,
            "root initialized",
        ));
        Ok(Self { state })
    }

    /// Legacy adapter for callers that already own a pinned static state.
    #[allow(dead_code)]
    pub(crate) fn from_state(state: Arc<ServeState>) -> Self {
        Self { state }
    }
}

impl Service for StaticService {
    fn request_body_policy(
        &self,
        _head: &RequestHead,
    ) -> crate::primitives::request_body_policy::RequestBodyPolicy {
        // Static serving never consumes request content. Unsupported bodyless
        // methods still reach call() and receive the normal 405 response.
        crate::primitives::request_body_policy::RequestBodyPolicy::Reject
    }

    fn call(
        &self,
        request: Request,
    ) -> Pin<
        Box<dyn std::future::Future<Output = Result<CanonicalResponse, ServiceError>> + Send + '_>,
    > {
        let state = self.state.clone();
        let (head, _body) = request.into_head_and_body();
        Box::pin(async move { plan_static_request(head, &state) })
    }
}

fn plan_static_request(
    request: RequestHead,
    state: &ServeState,
) -> Result<CanonicalResponse, ServiceError> {
    let method = request.method();
    let is_head = method.is_head();
    if !method.is_get() && !is_head {
        return error_response(
            StatusCode::METHOD_NOT_ALLOWED,
            "405 Method Not Allowed\n",
            is_head,
            true,
        );
    }

    let target = request.target();
    let config = state.config();
    let path_policy = PathPolicy {
        dotfiles: match config.static_policy.dotfiles {
            DotfilePolicy::Denied => PathPolicy::default().dotfiles,
            DotfilePolicy::Serve => crate::path::DotfilePolicy::Allow,
        },
        reject_backslash: true,
    };
    let confined = match ConfinedPath::parse(target.path(), &path_policy) {
        Ok(path) => path,
        Err(rejection) => {
            let malformed = matches!(
                rejection,
                crate::path::PathRejection::MalformedPercentEncoding
                    | crate::path::PathRejection::InvalidUtf8
                    | crate::path::PathRejection::NulByte
                    | crate::path::PathRejection::Empty
                    | crate::path::PathRejection::UnsupportedUriForm
                    | crate::path::PathRejection::TooLong
            );
            return error_response(
                if malformed {
                    StatusCode::BAD_REQUEST
                } else {
                    StatusCode::FORBIDDEN
                },
                if malformed {
                    "400 Bad Request\n"
                } else {
                    "403 Forbidden\n"
                },
                is_head,
                false,
            );
        }
    };

    let guard = RootGuard::new(state.pinned_root());
    let if_match = request.headers().get_first("if-match").map(|v| v.as_str());
    let if_unmodified_since = request
        .headers()
        .get_first("if-unmodified-since")
        .map(|v| v.as_str());
    let if_none_match = request
        .headers()
        .get_first("if-none-match")
        .map(|v| v.as_str());
    let if_modified_since = request
        .headers()
        .get_first("if-modified-since")
        .map(|v| v.as_str());
    let range = request.headers().get_first("range").map(|v| v.as_str());
    let if_range = request.headers().get_first("if-range").map(|v| v.as_str());
    let method = if is_head {
        ReadOnlyMethod::Head
    } else {
        ReadOnlyMethod::Get
    };

    match guard.resolve(&confined, &config.static_policy) {
        ResolvedResource::File(file) => planned_file_response(
            file,
            config,
            method,
            if_match,
            if_unmodified_since,
            if_none_match,
            if_modified_since,
            range,
            if_range,
            is_head,
        ),
        ResolvedResource::Directory(dir) => {
            let raw_path = confined.as_str();
            if !raw_path.ends_with('/') {
                // Build the redirect target from percent-encoded components:
                // `as_str()` is the decoded path and must not leak raw
                // spaces/control bytes into the `Location` header value.
                let mut location = String::new();
                for component in confined.components() {
                    location.push('/');
                    location.push_str(&percent_encode_path_segment(component));
                }
                location.push('/');
                if let Some(q) = target.query() {
                    // `RequestTarget::parse` already rejects CR/LF/SP/TAB in
                    // the query, so this branch never produces a header
                    // injection. The assert below is defense-in-depth should
                    // that parsing invariant ever loosen; `HeaderValue::new`
                    // on `location` below is the authoritative gate.
                    debug_assert!(
                        !q.bytes()
                            .any(|b| b == b'\r' || b == b'\n' || b == b'\t' || b == b' '),
                        "query contains header-injection bytes: {q:?}"
                    );
                    location.push('?');
                    location.push_str(q);
                }
                let mut builder =
                    CanonicalResponse::builder().status(StatusCode::MOVED_PERMANENTLY);
                builder = builder.push_header(
                    crate::primitives::header_block::HeaderName::new("location")
                        .map_err(|e| ServiceError::internal(e.to_string()))?,
                    crate::primitives::header_block::HeaderValue::new(&location)
                        .map_err(|e| ServiceError::internal(e.to_string()))?,
                );
                let response = builder
                    .body(ResponseBody::Empty)
                    .map_err(|e| ServiceError::internal(e.to_string()))?;
                return normalize_response(response, &NormalizeRequest::new(is_head))
                    .map_err(|e| ServiceError::internal(e.to_string()));
            }
            plan_directory_response(
                &guard,
                dir,
                config,
                method,
                if_match,
                if_unmodified_since,
                if_none_match,
                if_modified_since,
                range,
                if_range,
                is_head,
            )
        }
        ResolvedResource::NotFound => {
            error_response(StatusCode::NOT_FOUND, "404 Not Found\n", is_head, false)
        }
        ResolvedResource::Denied(_) => {
            error_response(StatusCode::FORBIDDEN, "403 Forbidden\n", is_head, false)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn planned_file_response(
    file: crate::fs::ResolvedFile,
    config: &ServeConfig,
    method: ReadOnlyMethod,
    if_match: Option<&str>,
    if_unmodified_since: Option<&str>,
    if_none_match: Option<&str>,
    if_modified_since: Option<&str>,
    range: Option<&str>,
    if_range: Option<&str>,
    is_head: bool,
) -> Result<CanonicalResponse, ServiceError> {
    let mut plan = plan_file_response_with_preconditions(
        method,
        &file.metadata,
        {
            let detected = crate::mime::mime_for_path(
                &file.safe_relative_components.iter().collect::<PathBuf>(),
            );
            if detected == "application/octet-stream" {
                &config.default_content_type
            } else {
                detected
            }
        },
        if_match,
        if_unmodified_since,
        if_none_match,
        if_modified_since,
        range,
        if_range,
    );
    if plan.status.as_u16() == 200 {
        append_extra_headers(&mut plan.headers, config);
    }
    let body = file
        .into_body(&plan)
        .map_err(|e| ServiceError::internal(format!("file body conversion failed: {e}")))?;
    canonical_response(plan.status.as_u16(), &plan.headers, body, is_head)
}

#[allow(clippy::too_many_arguments)]
fn plan_directory_response(
    guard: &RootGuard<'_>,
    dir: ResolvedDirectory,
    config: &ServeConfig,
    method: ReadOnlyMethod,
    if_match: Option<&str>,
    if_unmodified_since: Option<&str>,
    if_none_match: Option<&str>,
    if_modified_since: Option<&str>,
    range: Option<&str>,
    if_range: Option<&str>,
    is_head: bool,
) -> Result<CanonicalResponse, ServiceError> {
    for index in ["index.html", "index.htm"] {
        match guard.resolve_child(&dir, index, &config.static_policy) {
            ResolvedResource::File(file) => {
                return planned_file_response(
                    file,
                    config,
                    method,
                    if_match,
                    if_unmodified_since,
                    if_none_match,
                    if_modified_since,
                    range,
                    if_range,
                    is_head,
                );
            }
            ResolvedResource::NotFound => continue,
            ResolvedResource::Denied(_) => {
                return error_response(StatusCode::FORBIDDEN, "403 Forbidden\n", is_head, false)
            }
            ResolvedResource::Directory(_) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "500 Internal Server Error\n",
                    is_head,
                    false,
                )
            }
        }
    }

    match config.static_policy.directory_listing {
        DirectoryListingPolicy::Disabled => {
            error_response(StatusCode::FORBIDDEN, "403 Forbidden\n", is_head, false)
        }
        DirectoryListingPolicy::Enabled => {
            let entries = guard
                .list_directory(
                    &dir,
                    &config.static_policy,
                    config.limits.max_listing_entries,
                )
                .map_err(|_| ServiceError::internal("directory listing failed"))?;
            let body =
                render_directory_listing(&entries, config.limits.max_listing_response_bytes)?;
            let mut headers = listing_headers();
            append_extra_headers(&mut headers, config);
            canonical_response(
                StatusCode::OK.as_u16(),
                &headers,
                BodySource::Bytes(body),
                is_head,
            )
        }
    }
}

fn append_extra_headers(headers: &mut HeaderMapPlan, config: &ServeConfig) {
    let owned_names: Vec<String> = headers.iter().map(|header| header.name.clone()).collect();
    for (name, value) in &config.extra_response_headers {
        if !owned_names
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(name))
        {
            headers.push(name.clone(), value.clone());
        }
    }
}

fn canonical_response(
    status: u16,
    planned_headers: &HeaderMapPlan,
    body: BodySource,
    is_head: bool,
) -> Result<CanonicalResponse, ServiceError> {
    let status = StatusCode::new(status).map_err(|e| ServiceError::internal(e.to_string()))?;
    let mut builder = CanonicalResponse::builder().status(status);
    for header in planned_headers.iter() {
        builder = builder.push_header(
            HeaderName::new(&header.name).map_err(|e| ServiceError::internal(e.to_string()))?,
            HeaderValue::new(&header.value).map_err(|e| ServiceError::internal(e.to_string()))?,
        );
    }
    let response_body = match body {
        BodySource::Empty if is_head && status.permits_payload_body() => planned_headers
            .get("content-length")
            .and_then(|value| value.parse::<u64>().ok())
            .map(ResponseBody::EmptyWithLength)
            .unwrap_or(ResponseBody::Empty),
        BodySource::Empty => ResponseBody::Empty,
        BodySource::Bytes(bytes) => ResponseBody::Bytes(bytes),
        body @ (BodySource::FileFull { .. } | BodySource::FileRange { .. }) => {
            ResponseBody::File(body)
        }
    };
    let response = builder
        .body(response_body)
        .map_err(|e| ServiceError::internal(e.to_string()))?;
    normalize_response(response, &NormalizeRequest::new(is_head))
        .map_err(|e| ServiceError::internal(e.to_string()))
}

fn error_response(
    status: StatusCode,
    text: &'static str,
    is_head: bool,
    method_not_allowed: bool,
) -> Result<CanonicalResponse, ServiceError> {
    let mut builder = CanonicalResponse::builder().status(status).push_header(
        crate::primitives::header_block::HeaderName::new("content-type")
            .map_err(|e| ServiceError::internal(e.to_string()))?,
        crate::primitives::header_block::HeaderValue::new("text/plain; charset=utf-8")
            .map_err(|e| ServiceError::internal(e.to_string()))?,
    );
    if method_not_allowed {
        builder = builder.push_header(
            crate::primitives::header_block::HeaderName::new("allow")
                .map_err(|e| ServiceError::internal(e.to_string()))?,
            crate::primitives::header_block::HeaderValue::new("GET, HEAD")
                .map_err(|e| ServiceError::internal(e.to_string()))?,
        );
    }
    let response = builder
        .body(ResponseBody::Bytes(text.as_bytes().to_vec()))
        .map_err(|e| ServiceError::internal(e.to_string()))?;
    normalize_response(response, &NormalizeRequest::new(is_head))
        .map_err(|e| ServiceError::internal(e.to_string()))
}

fn listing_headers() -> HeaderMapPlan {
    let mut headers = HeaderMapPlan::new();
    headers.push("content-type", "text/html; charset=utf-8");
    headers.push(
        "content-security-policy",
        "default-src 'none'; base-uri 'none'; form-action 'none'",
    );
    headers.push("referrer-policy", "no-referrer");
    headers.push("x-content-type-options", "nosniff");
    headers
}

fn render_directory_listing(
    entries: &[(String, bool)],
    max_response_bytes: usize,
) -> Result<Vec<u8>, ServiceError> {
    let prefix = "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>Directory listing</title>\n</head>\n<body>\n<h1>Directory listing</h1>\n<ul>\n";
    let suffix = "</ul>\n</body>\n</html>\n";
    if prefix
        .len()
        .checked_add(suffix.len())
        .is_none_or(|n| n > max_response_bytes)
    {
        return Err(ServiceError::internal(
            "directory listing exceeds configured bound",
        ));
    }
    let mut html = String::from(prefix);
    for (name, is_dir) in entries {
        let visible = html_escape(name);
        let href = html_escape(&percent_encode_path_segment(name));
        let entry = if *is_dir {
            format!("<li><a href=\"{href}/\">{visible}/</a></li>\n")
        } else {
            format!("<li><a href=\"{href}\">{visible}</a></li>\n")
        };
        if html
            .len()
            .checked_add(entry.len())
            .and_then(|n| n.checked_add(suffix.len()))
            .is_none_or(|n| n > max_response_bytes)
        {
            return Err(ServiceError::internal(
                "directory listing exceeds configured bound",
            ));
        }
        html.push_str(&entry);
    }
    html.push_str(suffix);
    Ok(html.into_bytes())
}

fn html_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            c if !c.is_control() => out.push(c),
            _ => {}
        }
    }
    out
}

fn percent_encode_path_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        if matches!(*byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~') {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::header_block::HeaderBlock;
    use crate::primitives::method::Method;
    use crate::primitives::request_target::RequestTarget;
    use crate::primitives::version::HttpVersion;
    use tempfile::TempDir;

    fn request(method: Method, path: &str) -> Request {
        request_with_headers(method, path, HeaderBlock::new())
    }

    fn request_with_headers(method: Method, path: &str, headers: HeaderBlock) -> Request {
        Request::new(
            RequestHead::new(
                method,
                RequestTarget::parse(path).unwrap(),
                HttpVersion::Http11,
                headers,
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
    async fn file_and_range_bodies_remain_canonical_file_sources() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("file.txt"), b"0123456789").unwrap();
        let service = StaticService::builder(tmp.path()).build().unwrap();
        let get = service
            .call(request(Method::get(), "/file.txt"))
            .await
            .unwrap();
        assert!(matches!(
            get.body(),
            Some(ResponseBody::File(BodySource::FileFull { .. }))
        ));

        let mut range_headers = HeaderBlock::new();
        range_headers.push_str("range", "bytes=2-4").unwrap();
        let range_request = Request::new(
            RequestHead::new(
                Method::get(),
                RequestTarget::parse("/file.txt").unwrap(),
                HttpVersion::Http11,
                range_headers,
            ),
            crate::primitives::request_body::RequestBody::empty(),
            crate::primitives::connection_info::ConnectionInfo {
                local_addr: "127.0.0.1:8000".parse().unwrap(),
                remote_addr: "127.0.0.1:12345".parse().unwrap(),
                scheme: crate::primitives::connection_info::Scheme::Http,
                tls: None,
            },
        );
        let range = service.call(range_request).await.unwrap();
        assert!(matches!(
            range.body(),
            Some(ResponseBody::File(BodySource::FileRange { .. }))
        ));
    }

    #[tokio::test]
    async fn head_and_conditional_responses_have_no_file_body() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("file.txt"), b"hello").unwrap();
        let service = StaticService::builder(tmp.path()).build().unwrap();
        let head = service
            .call(request(Method::head(), "/file.txt"))
            .await
            .unwrap();
        assert!(!matches!(head.body(), Some(ResponseBody::File(_))));
        assert_eq!(
            head.headers().get_first("content-length").unwrap().as_str(),
            "5"
        );
    }

    #[tokio::test]
    async fn head_empty_file_preserves_zero_content_length() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("empty.txt"), b"").unwrap();
        let service = StaticService::builder(tmp.path()).build().unwrap();

        let get = service
            .call(request(Method::get(), "/empty.txt"))
            .await
            .unwrap();
        let head = service
            .call(request(Method::head(), "/empty.txt"))
            .await
            .unwrap();
        assert_eq!(
            get.headers().get_first("content-length").unwrap().as_str(),
            "0"
        );
        assert_eq!(
            head.headers().get_first("content-length").unwrap().as_str(),
            "0"
        );
    }

    #[tokio::test]
    async fn canonical_response_preserves_planner_metadata() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("file.txt"), b"0123456789").unwrap();
        let service = StaticService::builder(tmp.path()).build().unwrap();

        let full = service
            .call(request(Method::get(), "/file.txt"))
            .await
            .unwrap();
        assert_eq!(full.status().as_u16(), 200);
        assert_eq!(
            full.headers().get_first("content-type").unwrap().as_str(),
            "text/plain; charset=utf-8"
        );
        assert_eq!(
            full.headers().get_first("content-length").unwrap().as_str(),
            "10"
        );
        assert_eq!(
            full.headers().get_first("accept-ranges").unwrap().as_str(),
            "bytes"
        );
        let etag = full
            .headers()
            .get_first("etag")
            .unwrap()
            .as_str()
            .to_owned();
        assert!(full.headers().contains("last-modified"));

        let mut range_headers = HeaderBlock::new();
        range_headers.push_str("range", "bytes=2-4").unwrap();
        let range = service
            .call(request_with_headers(
                Method::get(),
                "/file.txt",
                range_headers,
            ))
            .await
            .unwrap();
        assert_eq!(range.status().as_u16(), 206);
        assert_eq!(
            range.headers().get_first("content-range").unwrap().as_str(),
            "bytes 2-4/10"
        );
        assert_eq!(
            range
                .headers()
                .get_first("content-length")
                .unwrap()
                .as_str(),
            "3"
        );

        let mut unsatisfiable_headers = HeaderBlock::new();
        unsatisfiable_headers
            .push_str("range", "bytes=20-30")
            .unwrap();
        let unsatisfiable = service
            .call(request_with_headers(
                Method::get(),
                "/file.txt",
                unsatisfiable_headers,
            ))
            .await
            .unwrap();
        assert_eq!(unsatisfiable.status().as_u16(), 416);
        assert_eq!(
            unsatisfiable
                .headers()
                .get_first("content-range")
                .unwrap()
                .as_str(),
            "bytes */10"
        );
        assert!(!matches!(unsatisfiable.body(), Some(ResponseBody::File(_))));

        let mut conditional_headers = HeaderBlock::new();
        conditional_headers
            .push_str("if-none-match", &etag)
            .unwrap();
        let conditional = service
            .call(request_with_headers(
                Method::get(),
                "/file.txt",
                conditional_headers,
            ))
            .await
            .unwrap();
        assert_eq!(conditional.status().as_u16(), 304);
        assert_eq!(
            conditional.headers().get_first("etag").unwrap().as_str(),
            etag
        );
        assert!(!matches!(conditional.body(), Some(ResponseBody::File(_))));

        let head = service
            .call(request(Method::head(), "/file.txt"))
            .await
            .unwrap();
        assert_eq!(head.status().as_u16(), full.status().as_u16());
        assert_eq!(
            head.headers().get_first("content-type").unwrap().as_str(),
            "text/plain; charset=utf-8"
        );
        assert_eq!(
            head.headers().get_first("content-length").unwrap().as_str(),
            "10"
        );
        assert!(!matches!(head.body(), Some(ResponseBody::File(_))));
    }

    #[tokio::test]
    async fn canonical_response_preserves_listing_and_error_metadata() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("dir")).unwrap();
        std::fs::write(tmp.path().join("dir/file.txt"), b"file").unwrap();
        let mut config = ServeConfig {
            root: tmp.path().to_path_buf(),
            ..ServeConfig::default()
        };
        config.static_policy.directory_listing = DirectoryListingPolicy::Enabled;
        let service = StaticService::from_serve_config(Arc::new(config)).unwrap();

        let listing = service.call(request(Method::get(), "/dir/")).await.unwrap();
        assert_eq!(
            listing
                .headers()
                .get_first("content-type")
                .unwrap()
                .as_str(),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            listing
                .headers()
                .get_first("content-security-policy")
                .unwrap()
                .as_str(),
            "default-src 'none'; base-uri 'none'; form-action 'none'"
        );
        assert_eq!(
            listing
                .headers()
                .get_first("referrer-policy")
                .unwrap()
                .as_str(),
            "no-referrer"
        );
        assert_eq!(
            listing
                .headers()
                .get_first("x-content-type-options")
                .unwrap()
                .as_str(),
            "nosniff"
        );
        assert!(listing.headers().contains("content-length"));

        let not_allowed = service
            .call(request(Method::post(), "/dir/"))
            .await
            .unwrap();
        assert_eq!(not_allowed.status().as_u16(), 405);
        assert_eq!(
            not_allowed.headers().get_first("allow").unwrap().as_str(),
            "GET, HEAD"
        );
        assert_eq!(
            not_allowed
                .headers()
                .get_first("content-type")
                .unwrap()
                .as_str(),
            "text/plain; charset=utf-8"
        );
        assert!(not_allowed.headers().contains("content-length"));
    }

    #[tokio::test]
    async fn index_and_listing_use_canonical_bodies() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("dir")).unwrap();
        std::fs::write(tmp.path().join("dir/index.htm"), b"index").unwrap();
        let mut config = ServeConfig {
            root: tmp.path().to_path_buf(),
            ..ServeConfig::default()
        };
        config.static_policy.directory_listing = DirectoryListingPolicy::Enabled;
        let service = StaticService::from_serve_config(Arc::new(config)).unwrap();
        let index = service.call(request(Method::get(), "/dir/")).await.unwrap();
        assert!(matches!(index.body(), Some(ResponseBody::File(_))));
        std::fs::remove_file(tmp.path().join("dir/index.htm")).unwrap();
        let listing = service.call(request(Method::get(), "/dir/")).await.unwrap();
        assert!(matches!(listing.body(), Some(ResponseBody::Bytes(_))));
    }
}

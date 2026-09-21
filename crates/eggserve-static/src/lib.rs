//! Hardened static-file serving built on the mature EggServe resolver.
//!
//! Resolution is descriptor/handle-relative under safe defaults. The service
//! keeps the opened file capability through planning and response construction;
//! it never checks a path and reopens it by pathname. This crate is the sole
//! static request-to-response authority; compatibility layers only project
//! their configuration into it.

use std::path::Path;
use std::sync::Arc;

use eggserve_primitives::{
    BodyPlan, BodySource, ErrorRepresentationPolicy, ReadOnlyMethod, Request, Response,
    ResponseBody, ResponseConstructionError, StaticPolicy, StatusCode,
};
use eggserve_server::{Service, ServiceError, ServiceFuture};

mod fs;
mod mime;
pub mod path;
mod planner;
mod secure_root;

pub use path::{ConfinedPath, DotfilePolicy as PathDotfilePolicy, PathPolicy, PathRejection};
pub use planner::{
    evaluate_conditional_headers, evaluate_if_match, evaluate_if_none_match, evaluate_if_range,
    evaluate_range_header, generate_etag, plan_directory_listing, plan_file_response,
    plan_file_response_with_preconditions, plan_file_response_with_preconditions_and_metadata,
};
pub use secure_root::{
    resolve_and_plan, ResolveAndPlanError, ResolvedDirectory, ResolvedFile, ResolvedResource,
    ResourceDeniedReason, SecureRoot,
};

#[derive(Debug, Clone)]
pub struct StaticServiceBuilder {
    root: std::path::PathBuf,
    policy: StaticPolicy,
    default_content_type: String,
    extra_response_headers: Vec<(String, String)>,
    error_policy: ErrorRepresentationPolicy,
    max_listing_entries: usize,
    max_listing_response_bytes: usize,
}

impl StaticServiceBuilder {
    pub fn policy(mut self, policy: StaticPolicy) -> Self {
        self.policy = policy;
        self
    }

    pub fn default_content_type(mut self, content_type: impl Into<String>) -> Self {
        self.default_content_type = content_type.into();
        self
    }

    pub fn extra_response_headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.extra_response_headers = headers;
        self
    }

    pub fn error_policy(mut self, policy: ErrorRepresentationPolicy) -> Self {
        self.error_policy = policy;
        self
    }

    #[doc(hidden)]
    pub fn listing_limits(mut self, max_entries: usize, max_response_bytes: usize) -> Self {
        self.max_listing_entries = max_entries;
        self.max_listing_response_bytes = max_response_bytes;
        self
    }

    pub fn build(self) -> Result<StaticService, ServiceError> {
        let root = SecureRoot::new(self.root, self.policy).map_err(|error| {
            ServiceError::internal(format!("failed to initialize static root: {error}"))
        })?;
        Ok(StaticService::from_root(
            Arc::new(root),
            self.default_content_type,
            self.extra_response_headers,
            self.error_policy,
            self.max_listing_entries,
            self.max_listing_response_bytes,
        ))
    }
}

#[derive(Clone)]
pub struct StaticService {
    root: Arc<SecureRoot>,
    default_content_type: String,
    extra_response_headers: Vec<(String, String)>,
    error_policy: ErrorRepresentationPolicy,
    max_listing_entries: usize,
    max_listing_response_bytes: usize,
}

impl StaticService {
    pub fn builder(root: impl AsRef<Path>) -> StaticServiceBuilder {
        StaticServiceBuilder {
            root: root.as_ref().to_path_buf(),
            policy: StaticPolicy::safe_default(),
            default_content_type: "application/octet-stream".to_owned(),
            extra_response_headers: Vec::new(),
            error_policy: ErrorRepresentationPolicy::Minimal,
            max_listing_entries: 4096,
            max_listing_response_bytes: 1024 * 1024,
        }
    }

    #[doc(hidden)]
    pub fn from_root(
        root: Arc<SecureRoot>,
        default_content_type: String,
        extra_response_headers: Vec<(String, String)>,
        error_policy: ErrorRepresentationPolicy,
        max_listing_entries: usize,
        max_listing_response_bytes: usize,
    ) -> Self {
        Self {
            root,
            default_content_type,
            extra_response_headers,
            error_policy,
            max_listing_entries,
            max_listing_response_bytes,
        }
    }

    pub fn root(&self) -> &SecureRoot {
        &self.root
    }

    async fn respond(&self, request: Request) -> Result<Response, ServiceError> {
        let head = request.head();
        let is_head = head.is_head();
        if !head.permits_static_resolution() {
            return self.error_response(
                StatusCode::METHOD_NOT_ALLOWED,
                "405 Method Not Allowed\n",
                is_head,
                true,
            );
        }
        let method = if is_head {
            ReadOnlyMethod::Head
        } else {
            ReadOnlyMethod::Get
        };
        let resource = match self.root.resolve_uri(head.target().path()) {
            Ok(resource) => resource,
            Err(rejection) => {
                let (status, body) = path_rejection_response(rejection);
                return self.error_response(status, body, is_head, false);
            }
        };

        match resource {
            ResolvedResource::File(file) => self.file_response(file, method, &request),
            ResolvedResource::Directory(directory) => {
                if !head.target().path().ends_with('/') {
                    let mut location = head.target().path().to_owned();
                    location.push('/');
                    if let Some(query) = head.target().query() {
                        location.push('?');
                        location.push_str(query);
                    }
                    let response = Response::builder()
                        .status(StatusCode::MOVED_PERMANENTLY)
                        .header("location", location)
                        .map_err(|error: ResponseConstructionError| {
                            ServiceError::internal(error.to_string())
                        })?
                        .body(ResponseBody::Empty)
                        .map_err(|error: ResponseConstructionError| {
                            ServiceError::internal(error.to_string())
                        })?;
                    return eggserve_primitives::normalize_response(
                        response,
                        &eggserve_primitives::NormalizeRequest::new(is_head),
                    )
                    .map_err(|error| ServiceError::internal(error.to_string()));
                }
                for index_name in ["index.html", "index.htm"] {
                    match directory.resolve_child(index_name, &self.root) {
                        ResolvedResource::File(file) => {
                            return self.file_response(file, method, &request);
                        }
                        ResolvedResource::Directory(_) => {
                            return self.error_response(
                                StatusCode::FORBIDDEN,
                                "403 Forbidden\n",
                                is_head,
                                false,
                            );
                        }
                        ResolvedResource::Denied(_) | ResolvedResource::IoError(_) => {
                            return self.error_response(
                                StatusCode::FORBIDDEN,
                                "403 Forbidden\n",
                                is_head,
                                false,
                            );
                        }
                        ResolvedResource::NotFound => {}
                    }
                }
                self.directory_response(directory, method)
            }
            ResolvedResource::NotFound => {
                self.error_response(StatusCode::NOT_FOUND, "404 Not Found\n", is_head, false)
            }
            ResolvedResource::Denied(_) => {
                self.error_response(StatusCode::FORBIDDEN, "403 Forbidden\n", is_head, false)
            }
            ResolvedResource::IoError(_) => {
                self.error_response(StatusCode::NOT_FOUND, "404 Not Found\n", is_head, false)
            }
        }
    }

    fn file_response(
        &self,
        file: ResolvedFile,
        method: ReadOnlyMethod,
        request: &Request,
    ) -> Result<Response, ServiceError> {
        let [if_match, if_unmodified_since, if_none_match, if_modified_since, range, if_range] =
            Self::conditional_headers(request);
        let detected_content_type = file.content_type();
        let content_type = if detected_content_type == "application/octet-stream" {
            self.default_content_type.as_str()
        } else {
            detected_content_type
        };
        let mut plan = crate::planner::plan_file_response_with_preconditions_and_metadata(
            method,
            file.metadata(),
            content_type,
            if_match,
            if_unmodified_since,
            if_none_match,
            if_modified_since,
            range,
            if_range,
            self.root.policy().static_metadata,
        );
        if plan.status.as_u16() == 200 {
            self.append_extra_headers(&mut plan.headers);
        }
        let source = file
            .into_body(&plan)
            .map_err(|error| ServiceError::internal(error.to_string()))?;
        response_from_plan(plan, source, matches!(method, ReadOnlyMethod::Head))
    }

    fn conditional_headers(request: &Request) -> [Option<&str>; 6] {
        let mut values = [None; 6];
        for field in request.head().headers().iter() {
            let slot = match field.name.as_str() {
                name if name.eq_ignore_ascii_case("if-match") => 0,
                name if name.eq_ignore_ascii_case("if-unmodified-since") => 1,
                name if name.eq_ignore_ascii_case("if-none-match") => 2,
                name if name.eq_ignore_ascii_case("if-modified-since") => 3,
                name if name.eq_ignore_ascii_case("range") => 4,
                name if name.eq_ignore_ascii_case("if-range") => 5,
                _ => continue,
            };
            if values[slot].is_none() {
                values[slot] = field.value.to_str().ok();
            }
        }
        values
    }

    fn directory_response(
        &self,
        directory: ResolvedDirectory,
        method: ReadOnlyMethod,
    ) -> Result<Response, ServiceError> {
        let is_head = matches!(method, ReadOnlyMethod::Head);
        if self.root.policy().directory_listing
            != eggserve_primitives::DirectoryListingPolicy::Enabled
        {
            return self.error_response(StatusCode::FORBIDDEN, "403 Forbidden\n", is_head, false);
        }
        let entries = directory
            .list(&self.root, self.max_listing_entries)
            .map_err(|_| ServiceError::internal("directory listing failed"))?;
        let body = render_directory_listing(&entries, self.max_listing_response_bytes)?;
        let mut plan = plan_directory_listing(body.len(), is_head);
        self.append_extra_headers(&mut plan.headers);
        if !is_head {
            plan.body = BodyPlan::FullBytes(body);
        }
        response_from_plan(plan, BodySource::Empty, is_head)
    }

    fn append_extra_headers(&self, headers: &mut eggserve_primitives::HeaderMapPlan) {
        let existing: Vec<String> = headers
            .iter()
            .map(|header| header.name.to_ascii_lowercase())
            .collect();
        for (name, value) in &self.extra_response_headers {
            if !existing
                .iter()
                .any(|entry| entry.eq_ignore_ascii_case(name))
            {
                headers.push(name.clone(), value.clone());
            }
        }
    }

    fn error_response(
        &self,
        status: StatusCode,
        text: &'static str,
        is_head: bool,
        method_not_allowed: bool,
    ) -> Result<Response, ServiceError> {
        let mut builder = Response::builder().status(status);
        if self.error_policy == ErrorRepresentationPolicy::Minimal {
            builder = builder
                .header("content-type", "text/plain; charset=utf-8")
                .map_err(|error: ResponseConstructionError| {
                    ServiceError::internal(error.to_string())
                })?;
        }
        if method_not_allowed {
            builder = builder.header("allow", "GET, HEAD").map_err(
                |error: ResponseConstructionError| ServiceError::internal(error.to_string()),
            )?;
        }
        let body = if self.error_policy == ErrorRepresentationPolicy::Minimal && !is_head {
            ResponseBody::Bytes(text.as_bytes().to_vec())
        } else {
            ResponseBody::Empty
        };
        let response = builder
            .body(body)
            .map_err(|error: ResponseConstructionError| {
                ServiceError::internal(error.to_string())
            })?;
        eggserve_primitives::normalize_response(
            response,
            &eggserve_primitives::NormalizeRequest::new(is_head),
        )
        .map_err(|error| ServiceError::internal(error.to_string()))
    }
}

impl Service for StaticService {
    fn request_body_policy(
        &self,
        _head: &eggserve_primitives::RequestHead,
    ) -> eggserve_primitives::RequestBodyPolicy {
        eggserve_primitives::RequestBodyPolicy::Reject
    }

    fn call(&self, request: Request) -> ServiceFuture<'_> {
        Box::pin(self.respond(request))
    }
}

fn response_from_plan(
    plan: eggserve_primitives::StaticResponsePlan,
    source: BodySource,
    is_head: bool,
) -> Result<Response, ServiceError> {
    let status = StatusCode::new(plan.status.as_u16())
        .map_err(|_| ServiceError::internal("static planner returned invalid status"))?;
    let body = match plan.body {
        BodyPlan::Empty if is_head && status.permits_payload_body() => plan
            .headers
            .get("content-length")
            .and_then(|value| value.parse::<u64>().ok())
            .map(ResponseBody::EmptyWithLength)
            .unwrap_or(ResponseBody::Empty),
        BodyPlan::Empty => ResponseBody::Empty,
        BodyPlan::FullBytes(bytes) => ResponseBody::Bytes(bytes),
        BodyPlan::FileFull | BodyPlan::FileRange { .. } => ResponseBody::File(source),
    };
    let mut builder = Response::builder().status(status);
    for field in plan.headers.iter() {
        builder = builder
            .header(field.name.clone(), field.value.clone())
            .map_err(|error: ResponseConstructionError| {
                ServiceError::internal(error.to_string())
            })?;
    }
    let response = builder
        .body(body)
        .map_err(|error: ResponseConstructionError| ServiceError::internal(error.to_string()))?;
    eggserve_primitives::normalize_response(
        response,
        &eggserve_primitives::NormalizeRequest::new(is_head),
    )
    .map_err(|error| ServiceError::internal(error.to_string()))
}

fn path_rejection_response(rejection: PathRejection) -> (StatusCode, &'static str) {
    let malformed = matches!(
        rejection,
        PathRejection::MalformedPercentEncoding
            | PathRejection::InvalidUtf8
            | PathRejection::NulByte
            | PathRejection::ControlCharacter
            | PathRejection::Empty
            | PathRejection::UnsupportedUriForm
            | PathRejection::TooLong
    );
    if malformed {
        (StatusCode::BAD_REQUEST, "400 Bad Request\n")
    } else {
        (StatusCode::FORBIDDEN, "403 Forbidden\n")
    }
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
        .is_none_or(|length| length > max_response_bytes)
    {
        return Err(ServiceError::internal(
            "directory listing exceeds configured bound",
        ));
    }
    let mut html = String::from(prefix);
    html.reserve(entries.len().saturating_mul(64));
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
            .and_then(|length| length.checked_add(suffix.len()))
            .is_none_or(|length| length > max_response_bytes)
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
    use std::fmt::Write;
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#x27;"),
            character if !character.is_control() => output.push(character),
            character => write!(&mut output, "&#x{:X};", character as u32)
                .expect("writing to String cannot fail"),
        }
    }
    output
}

fn percent_encode_path_segment(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut output = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        if matches!(
            *byte,
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~'
        ) {
            output.push(*byte as char);
        } else {
            output.push('%');
            output.push(HEX[(byte >> 4) as usize] as char);
            output.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_defaults_are_preserved() {
        let policy = StaticPolicy::safe_default();
        assert_eq!(
            policy.directory_listing,
            eggserve_primitives::DirectoryListingPolicy::Disabled
        );
        assert_eq!(policy.symlinks, eggserve_primitives::SymlinkPolicy::Denied);
        assert_eq!(policy.dotfiles, eggserve_primitives::DotfilePolicy::Denied);
    }
}

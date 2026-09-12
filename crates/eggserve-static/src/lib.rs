//! Hardened static-file serving built on the mature EggServe resolver.
//!
//! Resolution is descriptor/handle-relative under safe defaults. The service
//! keeps the opened file capability through planning and response construction;
//! it never checks a path and reopens it by pathname.

use std::path::Path;
use std::sync::Arc;

use eggserve_primitives::{
    BodyPlan, BodySource, ReadOnlyMethod, Request, Response, ResponseBody,
    ResponseConstructionError, StaticPolicy, StatusCode,
};
use eggserve_server::{Service, ServiceError, ServiceFuture};

mod fs;
mod mime;
mod path;
mod planner;
mod secure_root;

pub use path::{DotfilePolicy as PathDotfilePolicy, PathPolicy, PathRejection};
pub use planner::{
    evaluate_conditional_headers, evaluate_if_match, evaluate_if_none_match, evaluate_if_range,
    evaluate_range_header, generate_etag, plan_directory_listing, plan_file_response,
    plan_file_response_with_preconditions, plan_file_response_with_preconditions_and_metadata,
};
pub use secure_root::{
    resolve_and_plan, ResolveAndPlanError, ResolvedDirectory, ResolvedFile, ResolvedResource,
    ResourceDeniedReason, SecureRoot,
};

/// Static service configuration.
#[derive(Debug, Clone)]
pub struct StaticServiceBuilder {
    root: std::path::PathBuf,
    policy: StaticPolicy,
    default_content_type: String,
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

    pub fn build(self) -> Result<StaticService, ServiceError> {
        let root = SecureRoot::new(self.root, self.policy).map_err(|error| {
            ServiceError::internal(format!("failed to initialize static root: {error}"))
        })?;
        Ok(StaticService {
            root: Arc::new(root),
            default_content_type: self.default_content_type,
        })
    }
}

/// Hardened static-file service implementing the direct server service trait.
#[derive(Clone)]
pub struct StaticService {
    root: Arc<SecureRoot>,
    #[allow(dead_code)]
    default_content_type: String,
}

impl StaticService {
    pub fn builder(root: impl AsRef<Path>) -> StaticServiceBuilder {
        StaticServiceBuilder {
            root: root.as_ref().to_path_buf(),
            policy: StaticPolicy::safe_default(),
            default_content_type: "application/octet-stream".to_owned(),
        }
    }

    pub fn root(&self) -> &SecureRoot {
        &self.root
    }

    async fn respond(&self, request: Request) -> Result<Response, ServiceError> {
        let head = request.head();
        if !head.permits_static_resolution() {
            return Err(ServiceError::rejected(405));
        }
        let method = if head.is_head() {
            ReadOnlyMethod::Head
        } else {
            ReadOnlyMethod::Get
        };
        let resource = self
            .root
            .resolve_uri(head.target().path())
            .map_err(|_| ServiceError::rejected(400))?;

        match resource {
            ResolvedResource::File(file) => self.file_response(file, method, &request),
            ResolvedResource::Directory(directory) => {
                let index = directory.resolve_child("index.html", &self.root);
                match index {
                    ResolvedResource::File(file) => self.file_response(file, method, &request),
                    ResolvedResource::Directory(_) => Err(ServiceError::rejected(403)),
                    ResolvedResource::NotFound => self.directory_response(directory, method),
                    ResolvedResource::Denied(_) | ResolvedResource::IoError(_) => {
                        Err(ServiceError::rejected(403))
                    }
                }
            }
            ResolvedResource::NotFound => Err(ServiceError::rejected(404)),
            ResolvedResource::Denied(_) => Err(ServiceError::rejected(403)),
            ResolvedResource::IoError(_) => Err(ServiceError::rejected(404)),
        }
    }

    fn file_response(
        &self,
        file: ResolvedFile,
        method: ReadOnlyMethod,
        request: &Request,
    ) -> Result<Response, ServiceError> {
        let header = |name: &str| {
            request
                .head()
                .headers()
                .get_first(name)
                .and_then(|value| value.to_str().ok())
        };
        let detected_content_type = file.content_type();
        let content_type = if detected_content_type == "application/octet-stream" {
            self.default_content_type.as_str()
        } else {
            detected_content_type
        };
        let plan = file.plan_response_with_content_type(
            method,
            header("if-match"),
            header("if-unmodified-since"),
            header("if-none-match"),
            header("if-modified-since"),
            header("range"),
            header("if-range"),
            content_type,
        );
        let source = file
            .into_body(&plan)
            .map_err(|error| ServiceError::internal(error.to_string()))?;
        response_from_plan(plan, source)
    }

    fn directory_response(
        &self,
        directory: ResolvedDirectory,
        method: ReadOnlyMethod,
    ) -> Result<Response, ServiceError> {
        if self.root.policy().directory_listing
            != eggserve_primitives::DirectoryListingPolicy::Enabled
        {
            return Err(ServiceError::rejected(403));
        }
        let entries = directory
            .list(&self.root, 4096)
            .map_err(|_| ServiceError::rejected(404))?;
        let mut body =
            String::from(r#"<!doctype html><meta charset="utf-8"><title>Index</title><ul>"#);
        for (name, is_dir) in entries {
            body.push_str("<li>");
            if is_dir {
                body.push_str("<strong>");
            }
            for ch in name.chars() {
                match ch {
                    '&' => body.push_str("&amp;"),
                    '<' => body.push_str("&lt;"),
                    '>' => body.push_str("&gt;"),
                    '"' => body.push_str("&quot;"),
                    _ => body.push(ch),
                }
            }
            if is_dir {
                body.push_str("</strong>/");
            }
            body.push_str("</li>");
        }
        body.push_str("</ul>");
        let mut plan = plan_directory_listing(body.len(), matches!(method, ReadOnlyMethod::Head));
        if matches!(method, ReadOnlyMethod::Get) {
            plan.body = BodyPlan::FullBytes(body.into_bytes());
        }
        response_from_plan(plan, BodySource::Empty)
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
) -> Result<Response, ServiceError> {
    let status = StatusCode::new(plan.status.as_u16())
        .map_err(|_| ServiceError::internal("static planner returned invalid status"))?;
    let body = match plan.body {
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
    builder
        .body(body)
        .map_err(|error: ResponseConstructionError| ServiceError::internal(error.to_string()))
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

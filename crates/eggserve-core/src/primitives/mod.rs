//! Public primitive types for embedding eggserve-core.
//!
//! This module is the **intended public boundary** for Rust consumers that want
//! to embed eggserve's hardened path validation and policy enforcement without
//! pulling in the full HTTP service layer.
//!
//! # Invariants
//!
//! Every type exported here enforces safety invariants at construction time:
//!
//! - [`ConfinedPath`] is only representable after passing through the full
//!   validation pipeline: origin-form parsing, percent decoding, path
//!   normalization, and component validation. No unchecked path can exist.
//! - [`StaticPolicy`] defaults deny all optional behaviors (directory listing,
//!   symlinks, dotfiles). Callers must explicitly opt in.
//! - [`PathRejection`] is the single error type for path validation failures.
//!   Every variant maps to a specific security check.
//!
//! # Two `DotfilePolicy` types
//!
//! There are two distinct `DotfilePolicy` enums that serve different layers:
//!
//! - [`DotfilePolicy`] (from [`crate::policy`]) — controls whether dotfiles are
//!   **served** in the final response. Part of [`StaticPolicy`].
//! - [`PathDotfilePolicy`] (from [`crate::path`]) — controls whether dotfile
//!   paths are **accepted** during [`ConfinedPath`] parsing. Part of
//!   [`PathPolicy`].
//!
//! In practice, if the path-level policy denies dotfiles, the request is
//! rejected before resolution and the static policy never sees it. Both must
//! agree for dotfiles to be served.

pub use crate::path::ConfinedPath;
pub use crate::path::DotfilePolicy as PathDotfilePolicy;
pub use crate::path::PathPolicy;
pub use crate::path::PathRejection;
pub use crate::policy::DirectoryListingPolicy;
pub use crate::policy::DotfilePolicy;
pub use crate::policy::StaticPolicy;
pub use crate::policy::SymlinkPolicy;

mod secure_root;
pub use secure_root::{
    ResolvedDirectory, ResolvedFile, ResolvedResource, ResourceDeniedReason, SecureRoot,
};

pub mod http;
pub mod planner;
pub mod response;

pub use http::{
    validate_method, validate_request_body, validate_request_target, ReadOnlyMethod,
    RequestValidationError,
};
pub use planner::{
    evaluate_conditional_headers, evaluate_if_none_match, evaluate_if_range, evaluate_range_header,
    generate_etag, plan_directory_listing, plan_file_response,
};
pub use response::{
    BodyPlan, ConditionalRequestOutcome, FileRange, HeaderMapPlan, RangeRequestOutcome,
    ResponseHeader, ResponseStatus, StaticResponsePlan,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_policy_default_denies_all() {
        let policy = StaticPolicy::default();
        assert_eq!(policy.directory_listing, DirectoryListingPolicy::Disabled);
        assert_eq!(policy.symlinks, SymlinkPolicy::Denied);
        assert_eq!(policy.dotfiles, DotfilePolicy::Denied);
    }

    #[test]
    fn static_policy_default_matches_safe_default() {
        let from_default = StaticPolicy::default();
        let from_safe = StaticPolicy::safe_default();
        assert_eq!(from_default.directory_listing, from_safe.directory_listing);
        assert_eq!(from_default.symlinks, from_safe.symlinks);
        assert_eq!(from_default.dotfiles, from_safe.dotfiles);
    }

    #[test]
    fn directory_listing_policy_default_is_disabled() {
        assert_eq!(
            DirectoryListingPolicy::default(),
            DirectoryListingPolicy::Disabled
        );
    }

    #[test]
    fn symlink_policy_default_is_denied() {
        assert_eq!(SymlinkPolicy::default(), SymlinkPolicy::Denied);
    }

    #[test]
    fn dotfile_policy_default_is_denied() {
        assert_eq!(DotfilePolicy::default(), DotfilePolicy::Denied);
    }

    #[test]
    fn path_dotfile_policy_default_is_denied() {
        assert_eq!(PathDotfilePolicy::default(), PathDotfilePolicy::Denied);
    }

    #[test]
    fn path_policy_default_denies_dotfiles_and_rejects_backslash() {
        let policy = PathPolicy::default();
        assert_eq!(policy.dotfiles, PathDotfilePolicy::Denied);
        assert!(policy.reject_backslash);
    }

    #[test]
    fn confined_path_parse_simple() {
        let path = ConfinedPath::parse("/foo/bar", &PathPolicy::default()).unwrap();
        assert_eq!(path.as_str(), "/foo/bar");
        assert_eq!(path.components(), &["foo", "bar"]);
    }

    #[test]
    fn confined_path_rejects_dotfile_with_default_policy() {
        let result = ConfinedPath::parse("/.env", &PathPolicy::default());
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), PathRejection::DotfileDenied);
    }

    #[test]
    fn confined_path_allows_dotfile_with_allow_policy() {
        let policy = PathPolicy {
            dotfiles: PathDotfilePolicy::Allow,
            ..PathPolicy::default()
        };
        let path = ConfinedPath::parse("/.env", &policy).unwrap();
        assert_eq!(path.as_str(), "/.env");
    }

    #[test]
    fn path_rejection_is_error() {
        let err: &dyn std::error::Error = &PathRejection::Empty;
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn two_dotfile_policies_are_distinct_types() {
        let static_policy = DotfilePolicy::Denied;
        let path_policy = PathDotfilePolicy::Denied;
        assert_eq!(static_policy, DotfilePolicy::Denied);
        assert_eq!(path_policy, PathDotfilePolicy::Denied);
    }
}

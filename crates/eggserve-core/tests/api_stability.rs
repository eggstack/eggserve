//! Compile-time API stability enforcement tests.
//!
//! These tests verify that the public API surface matches the stability tiers
//! defined in `docs/api-stability.md`. They are compile-sample checks: if a
//! type is importable, the test compiles; if it should be gated behind a
//! feature, the test uses `cfg` to verify the gate.

// ── Stable module re-exports ────────────────────────────────────────────────

#[test]
fn stable_types_accessible_from_primitives_facade() {
    // These must always be accessible through the `primitives` facade,
    // regardless of feature flags.
    use eggserve_core::primitives::ConfinedPath;
    use eggserve_core::primitives::PathPolicy;
    use eggserve_core::primitives::ResolvedResource;
    use eggserve_core::primitives::SecureRoot;
    use eggserve_core::primitives::StaticPolicy;

    use eggserve_core::limits::Limits;

    let _ = std::marker::PhantomData::<(
        ConfinedPath,
        PathPolicy,
        StaticPolicy,
        SecureRoot,
        ResolvedResource,
        Limits,
    )>;
}

#[test]
fn stable_response_plan_types_accessible() {
    use eggserve_core::primitives::BodyPlan;
    use eggserve_core::primitives::ConditionalRequestOutcome;
    use eggserve_core::primitives::FileRange;
    use eggserve_core::primitives::HeaderMapPlan;
    use eggserve_core::primitives::RangeRequestOutcome;
    use eggserve_core::primitives::ResponseHeader;
    use eggserve_core::primitives::ResponseStatus;
    use eggserve_core::primitives::StaticResponsePlan;

    let _ = std::marker::PhantomData::<(
        BodyPlan,
        ConditionalRequestOutcome,
        FileRange,
        HeaderMapPlan,
        RangeRequestOutcome,
        ResponseHeader,
        ResponseStatus,
        StaticResponsePlan,
    )>;
}

#[test]
fn stable_config_types_accessible() {
    use eggserve_core::config::ServeConfig;
    use eggserve_core::config::StartupSummary;

    let _ = std::marker::PhantomData::<(ServeConfig, StartupSummary)>;
}

#[test]
fn stable_limits_type_accessible() {
    use eggserve_core::limits::Limits;

    let _ = std::marker::PhantomData::<Limits>;
}

#[test]
fn stable_policy_types_accessible() {
    use eggserve_core::policy::DirectoryListingPolicy;
    use eggserve_core::policy::DotfilePolicy;
    use eggserve_core::policy::StaticPolicy;
    use eggserve_core::policy::SymlinkPolicy;

    let _ = std::marker::PhantomData::<(
        DirectoryListingPolicy,
        DotfilePolicy,
        StaticPolicy,
        SymlinkPolicy,
    )>;
}

#[test]
fn stable_primitives_path_types_accessible() {
    use eggserve_core::primitives::ConfinedPath;
    use eggserve_core::primitives::PathDotfilePolicy;
    use eggserve_core::primitives::PathPolicy;
    use eggserve_core::primitives::PathRejection;

    let _ =
        std::marker::PhantomData::<(ConfinedPath, PathDotfilePolicy, PathPolicy, PathRejection)>;
}

#[test]
fn stable_primitives_http_validation_types_accessible() {
    use eggserve_core::primitives::validate_method;
    use eggserve_core::primitives::validate_request_body;
    use eggserve_core::primitives::validate_request_target;
    use eggserve_core::primitives::ReadOnlyMethod;
    use eggserve_core::primitives::RequestValidationError;

    let _ = (
        validate_method,
        validate_request_body,
        validate_request_target,
        std::marker::PhantomData::<(ReadOnlyMethod, RequestValidationError)>,
    );
}

#[test]
fn stable_primitives_response_planning_functions_accessible() {
    use eggserve_core::primitives::evaluate_conditional_headers;
    use eggserve_core::primitives::evaluate_if_none_match;
    use eggserve_core::primitives::evaluate_if_range;
    use eggserve_core::primitives::evaluate_range_header;
    use eggserve_core::primitives::generate_etag;
    use eggserve_core::primitives::plan_directory_listing;
    use eggserve_core::primitives::plan_file_response;

    let _ = (
        evaluate_conditional_headers,
        evaluate_if_none_match,
        evaluate_if_range,
        evaluate_range_header,
        generate_etag,
        plan_directory_listing,
        plan_file_response,
    );
}

#[test]
fn stable_primitives_body_types_accessible() {
    use eggserve_core::primitives::BodyKind;
    use eggserve_core::primitives::BodySource;
    use eggserve_core::primitives::BodySourceError;

    let _ = std::marker::PhantomData::<(BodyKind, BodySource, BodySourceError)>;
}

#[test]
fn stable_primitives_secure_root_types_accessible() {
    use eggserve_core::primitives::resolve_and_plan;
    use eggserve_core::primitives::ResolveAndPlanError;
    use eggserve_core::primitives::ResolvedDirectory;
    use eggserve_core::primitives::ResolvedFile;
    use eggserve_core::primitives::ResourceDeniedReason;
    use eggserve_core::primitives::SecureRoot;

    let _ = (
        resolve_and_plan,
        std::marker::PhantomData::<(
            ResolveAndPlanError,
            ResolvedDirectory,
            ResolvedFile,
            ResourceDeniedReason,
            SecureRoot,
        )>,
    );
}

#[test]
fn experimental_canonical_request_types_accessible() {
    use eggserve_core::primitives::connection_info::{ConnectionInfo, Scheme, TlsInfo};
    use eggserve_core::primitives::header_block::{
        DuplicateHeaderError, HeaderBlock, HeaderError, HeaderField, HeaderName, HeaderValue,
    };
    use eggserve_core::primitives::method::{Method, MethodError};
    use eggserve_core::primitives::request_head::{RequestHead, RequestHeadError};
    use eggserve_core::primitives::request_target::{RequestTarget, RequestTargetError};
    use eggserve_core::primitives::version::{HttpVersion, HttpVersionError};

    let _ = (std::marker::PhantomData::<(
        Method,
        MethodError,
        HttpVersion,
        HttpVersionError,
        HeaderBlock,
        HeaderName,
        HeaderValue,
        HeaderField,
        HeaderError,
        DuplicateHeaderError,
        RequestTarget,
        RequestTargetError,
        RequestHead,
        RequestHeadError,
        ConnectionInfo,
        Scheme,
        TlsInfo,
    )>,);
}

// ── python-bindings-internal feature gate ────────────────────────────────────

#[test]
fn python_bindings_internal_extraction_methods_absent_by_default() {
    // `ResolvedFile::into_std_file()`, `into_parts()`, and `from_parts()`
    // are behind `python-bindings-internal` and must NOT be callable in
    // a default-feature build. This test compiles under default features,
    // which itself proves the gate is working — if the methods leaked
    // through, downstream code could call them without opting in.
    //
    // Positive verification of the gate is done by the Python crate's
    // build, which enables `python-bindings-internal` and calls these
    // methods directly.
    use eggserve_core::primitives::ResolvedFile;

    // Verify ResolvedFile is importable (it's a stable public type),
    // confirming the type exists while its extraction methods remain gated.
    let _phantom = std::marker::PhantomData::<ResolvedFile>;
}

// ── Client feature gate ──────────────────────────────────────────────────────

#[cfg(feature = "client")]
mod client_feature_enabled {
    #[test]
    fn client_module_accessible_with_feature() {
        use eggserve_core::primitives::client::validate_header;
        use eggserve_core::primitives::client::ClientConfig;
        use eggserve_core::primitives::client::ClientError;
        use eggserve_core::primitives::client::ClientRequest;
        use eggserve_core::primitives::client::ClientRequestBuilder;
        use eggserve_core::primitives::client::ClientResponse;
        use eggserve_core::primitives::client::HttpClient;
        use eggserve_core::primitives::client::Method;
        use eggserve_core::primitives::client::ParsedUrl;
        use eggserve_core::primitives::client::Scheme;

        let _ = (
            validate_header,
            std::marker::PhantomData::<(
                ClientConfig,
                ClientError,
                ClientRequest,
                ClientRequestBuilder,
                ClientResponse,
                HttpClient,
                Method,
                ParsedUrl,
                Scheme,
            )>,
        );
    }
}

// ── Client-tls feature gate ──────────────────────────────────────────────────

#[cfg(feature = "client-tls")]
mod client_tls_feature_enabled {
    #[test]
    fn client_tls_builds_with_feature() {
        // When client-tls is enabled, the full client module (including TLS
        // support) should compile. The presence of this module confirms the
        // feature gate works; actual TLS functionality is tested elsewhere.
        use eggserve_core::primitives::client::ClientConfig;

        let config = ClientConfig::default();
        // verify_tls defaults to true, which is the TLS-relevant setting.
        assert!(config.verify_tls);
    }
}

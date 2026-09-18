//! Static-confinement authority conformance (Plan 219).
//!
//! Proves that the `eggserve_core::primitives` static/path surface resolves to
//! the single `eggserve-static` implementation authority: core facade types
//! are the static types (assignability in both directions), core planner
//! functions are the static function items (identical fn pointers), and
//! confined resolution through the facade serves the same bytes as resolution
//! through the authority. A resolved file stays an opened capability end to
//! end — the facade never reconstructs a path and reopens it.

use eggserve_core::primitives::{
    resolve_and_plan, ConfinedPath, PathDotfilePolicy, PathPolicy, PathRejection,
    ResolveAndPlanError, ResolvedResource, ResourceDeniedReason, SecureRoot,
};

// ── Type identity: facades are the authority types ──────────────────────────

#[test]
fn secure_root_facade_is_the_static_authority() {
    fn accept_core(_: eggserve_core::primitives::SecureRoot) {}
    fn accept_static(_: eggserve_static::SecureRoot) {}
    let tmp = tempfile::TempDir::new().unwrap();
    let policy = eggserve_core::policy::StaticPolicy::safe_default();
    let via_core = SecureRoot::new(tmp.path(), policy.clone()).unwrap();
    // Same value is accepted as both the facade and the authority type.
    accept_core(via_core.clone());
    accept_static(via_core.clone());
    let via_static = eggserve_static::SecureRoot::new(tmp.path(), policy).unwrap();
    accept_core(via_static.clone());
    accept_static(via_static);
}

#[test]
fn confined_path_facade_is_the_static_authority() {
    fn accept_core(_: ConfinedPath) {}
    fn accept_static(_: eggserve_static::ConfinedPath) {}
    let policy = PathPolicy::default();
    let via_core = ConfinedPath::parse("/foo/bar", &policy).unwrap();
    accept_core(via_core.clone());
    accept_static(via_core);
    let via_static = eggserve_static::ConfinedPath::parse("/foo/bar", &policy).unwrap();
    accept_core(via_static.clone());
    accept_static(via_static);
}

#[test]
fn path_policy_vocabulary_is_shared() {
    // Both `DotfilePolicy` spellings name the static authority's enums.
    let core_policy = PathPolicy::default();
    let static_policy = eggserve_static::PathPolicy::default();
    assert_eq!(core_policy.dotfiles, static_policy.dotfiles);
    assert_eq!(core_policy.reject_backslash, static_policy.reject_backslash);
    assert_eq!(
        PathDotfilePolicy::Denied,
        eggserve_static::PathDotfilePolicy::Denied
    );
    // Rejection variants compare across the facade boundary.
    let rejection: PathRejection = PathRejection::DotfileDenied;
    assert_eq!(rejection, eggserve_static::PathRejection::DotfileDenied);
    let denied: ResourceDeniedReason = ResourceDeniedReason::SymlinkDenied;
    assert!(matches!(
        denied,
        eggserve_static::ResourceDeniedReason::SymlinkDenied
    ));
}

// ── Function identity: one planner implementation ───────────────────────────

#[test]
fn planner_functions_are_the_static_authority() {
    // Coercing both paths to the same fn-pointer type and comparing addresses
    // proves there is one implementation, not two matching copies.
    let core_plan = eggserve_core::primitives::planner::plan_file_response_with_preconditions
        as fn(
            eggserve_core::primitives::http::ReadOnlyMethod,
            &std::fs::Metadata,
            &str,
            Option<&str>,
            Option<&str>,
            Option<&str>,
            Option<&str>,
            Option<&str>,
            Option<&str>,
        ) -> eggserve_core::primitives::response::StaticResponsePlan;
    let static_plan = eggserve_static::plan_file_response_with_preconditions
        as fn(
            eggserve_core::primitives::http::ReadOnlyMethod,
            &std::fs::Metadata,
            &str,
            Option<&str>,
            Option<&str>,
            Option<&str>,
            Option<&str>,
            Option<&str>,
            Option<&str>,
        ) -> eggserve_core::primitives::response::StaticResponsePlan;
    assert_eq!(core_plan as usize, static_plan as usize);

    let core_decode =
        eggserve_core::primitives::percent_decode as fn(&str) -> Result<String, PathRejection>;
    let static_decode = eggserve_static::path::decode::percent_decode
        as fn(&str) -> Result<String, eggserve_static::PathRejection>;
    assert_eq!(core_decode as usize, static_decode as usize);
}

// ── Behavioral parity through the facade ────────────────────────────────────

fn fixture_root() -> (tempfile::TempDir, SecureRoot) {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("hello.txt"), b"hello").unwrap();
    std::fs::create_dir(tmp.path().join("subdir")).unwrap();
    std::fs::write(tmp.path().join("subdir").join("file.txt"), b"sub").unwrap();
    let policy = eggserve_core::policy::StaticPolicy::safe_default();
    let root = SecureRoot::new(tmp.path(), policy).unwrap();
    (tmp, root)
}

#[test]
fn facade_and_authority_agree_on_resolution_outcomes() {
    let (_tmp, root) = fixture_root();
    let static_root =
        eggserve_static::SecureRoot::new(root.root_path(), root.policy().clone()).unwrap();

    for uri in ["/hello.txt", "/subdir/file.txt", "/missing", "/../escape"] {
        let via_facade = root.resolve_uri(uri);
        let via_authority = static_root.resolve_uri(uri);
        match (via_facade, via_authority) {
            (Ok(a), Ok(b)) => assert_eq!(
                std::mem::discriminant(&a),
                std::mem::discriminant(&b),
                "outcome mismatch for {uri}"
            ),
            (Err(a), Err(b)) => assert_eq!(a, b, "rejection mismatch for {uri}"),
            (a, b) => panic!("ok/err mismatch for {uri}: {a:?} vs {b:?}"),
        }
    }
}

#[test]
fn facade_serves_opened_capability_bytes() {
    use eggserve_core::primitives::http::ReadOnlyMethod;

    let (_tmp, root) = fixture_root();
    let policy = PathPolicy::default();
    let confined = ConfinedPath::parse("/hello.txt", &policy).unwrap();
    let (plan, mut body) = resolve_and_plan(
        &root,
        &confined,
        ReadOnlyMethod::Get,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();
    assert_eq!(plan.status.as_u16(), 200);
    assert_eq!(body.len(), 5);
    assert_eq!(body.read_all().unwrap(), b"hello");
}

#[test]
fn facade_reports_denied_and_missing_distinctly() {
    let (_tmp, root) = fixture_root();
    let policy = PathPolicy::default();
    let missing = ConfinedPath::parse("/missing", &policy).unwrap();
    assert!(matches!(root.resolve(&missing), ResolvedResource::NotFound));
    let escape = ConfinedPath::from_path_component("/../escape", &policy);
    match escape {
        Err(rejection) => assert!(matches!(
            rejection,
            PathRejection::ParentComponent | PathRejection::RootEscapeDenied
        )),
        Ok(confined) => assert!(matches!(
            root.resolve(&confined),
            ResolvedResource::Denied(_) | ResolvedResource::NotFound
        )),
    }
    // `resolve_and_plan` preserves the missing-resource taxonomy (never
    // downgraded to a generic I/O error).
    let missing_plan = resolve_and_plan(
        &root,
        &missing,
        eggserve_core::primitives::http::ReadOnlyMethod::Get,
        None,
        None,
        None,
        None,
        None,
        None,
    );
    assert!(matches!(missing_plan, Err(ResolveAndPlanError::NotFound)));
}

// ── Capability bridge (Plan 219 §4, static-owned feature) ───────────────────

/// The Python bridge moves the already-opened handle (`into_parts` →
/// `from_parts`) without reconstructing a path or reopening. The bridge
/// constructors live on the static authority behind
/// `python-bindings-internal`, forwarded by the core facade feature.
#[cfg(feature = "python-bindings-internal")]
#[test]
fn capability_bridge_moves_the_opened_handle() {
    use eggserve_core::primitives::http::ReadOnlyMethod;

    let (_tmp, root) = fixture_root();
    let policy = PathPolicy::default();
    let confined = ConfinedPath::parse("/hello.txt", &policy).unwrap();
    let file = root.resolve(&confined).into_file().unwrap();
    let content_type = file.content_type().to_owned();
    let components = file.safe_relative_components().to_vec();
    let (handle, metadata) = file.into_parts();

    let rebuilt = eggserve_core::primitives::ResolvedFile::from_parts(handle, metadata, components);
    assert_eq!(rebuilt.content_type(), content_type);
    let plan = eggserve_core::primitives::planner::plan_file_response(
        ReadOnlyMethod::Get,
        rebuilt.metadata(),
        rebuilt.content_type(),
        None,
        None,
        None,
        None,
    );
    let mut body = rebuilt.into_body(&plan).unwrap();
    assert_eq!(body.read_all().unwrap(), b"hello");
}

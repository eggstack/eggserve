//! Shared request-target/confinement handoff corpus (Plan 184).

use eggserve_core::primitives::{ConfinedPath, PathPolicy, RequestTarget};

#[test]
fn canonical_target_and_confinement_share_http_classification() {
    let policy = PathPolicy::default();
    for raw in ["/", "/a?", "/a//b", "/foo#bar", "/a?b#frag"] {
        let target = RequestTarget::parse(raw).unwrap();
        assert!(ConfinedPath::parse(raw, &policy).is_ok(), "{raw:?}");
        assert!(target.path().starts_with('/'));
    }

    for raw in [
        "//",
        "///",
        "*",
        "http://example.test/a",
        "example.test:443",
    ] {
        assert!(
            RequestTarget::parse(raw).is_err(),
            "canonical accepted {raw:?}"
        );
        assert!(
            ConfinedPath::parse(raw, &policy).is_err(),
            "confinement accepted {raw:?}"
        );
    }
}

#[test]
fn path_security_remains_after_target_classification() {
    let policy = PathPolicy::default();
    for raw in [
        "/%2fetc/passwd",
        "/%ZZ",
        "/../etc/passwd",
        "/%2e%2e/etc/passwd",
        "/foo\0bar",
        "/foo\nbar",
    ] {
        let target = RequestTarget::parse(raw);
        if let Ok(target) = target {
            assert!(
                ConfinedPath::from_path_component(target.path(), &policy).is_err(),
                "path layer accepted {raw:?}"
            );
        } else {
            assert!(ConfinedPath::parse(raw, &policy).is_err());
        }
    }
}

#[test]
fn confinement_limit_is_distinct_from_runtime_target_limit() {
    let raw = format!("/{}", "a".repeat(8192));
    assert!(RequestTarget::parse(&raw).is_ok());
    assert!(ConfinedPath::parse(&raw, &PathPolicy::default()).is_err());
}

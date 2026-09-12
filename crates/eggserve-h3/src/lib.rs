//! Experimental HTTP/3 and QUIC dependency boundary for EggServe.
//!
//! This package deliberately owns the coordinated Quinn/H3/H3-Quinn version
//! set. It is a transport dependency boundary, not a second application
//! service model: the compatibility runtime keeps canonical request, response,
//! policy, timeout, and lifecycle semantics in EggServe's shared code.
//!
//! The re-exports are intentionally narrow in purpose. They let the
//! experimental EggServe adapter use the transport stack without making
//! `eggserve-core` or `eggserve-server` direct consumers of those packages.
//! Applications should not use these types as a stable EggServe API.

#[doc(hidden)]
pub use h3;
#[doc(hidden)]
pub use h3_quinn;
#[doc(hidden)]
pub use quinn;

/// Versions of the coordinated H3/QUIC compatibility set used by this crate.
pub const H3_VERSION: &str = "0.0.8";
pub const H3_QUINN_VERSION: &str = "0.0.10";
pub const QUINN_VERSION: &str = "0.11.11";

/// Returns the dependency set used for qualification and diagnostics.
pub const fn dependency_versions() -> (&'static str, &'static str, &'static str) {
    (H3_VERSION, H3_QUINN_VERSION, QUINN_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinated_dependency_versions_are_explicit() {
        assert_eq!(dependency_versions(), ("0.0.8", "0.0.10", "0.11.11"));
    }
}

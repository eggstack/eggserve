//! Hardened static-serving primitives for eggserve.
//!
//! # Public API status (alpha)
//!
//! The public surface is intentionally conservative during the alpha period.
//! Modules and types are divided into three buckets:
//!
//! - **Stable-ish (semver-considered)**: [`config`], [`limits`], [`policy`].
//!   Field shapes here may still evolve before 1.0, but breaking changes will
//!   be accompanied by a major version bump and a migration note.
//! - **Experimental**: [`service`]. The HTTP handler signature is exposed for
//!   integration users that want to embed `handle_request` in their own
//!   accept loop, but the body type (`BoxBodyInner`) and async surface are
//!   not stable. Breaking changes may occur in minor releases.
//! - **Internal**: [`fs`], [`path`], [`response`], MIME detection, and the
//!   error taxonomy. These are not part of the public API and are not
//!   re-exported. External callers should not depend on them.
//!
//! # Primitives facade
//!
//! The [`primitives`] module is the **intended public boundary** for Rust
//! consumers that want to embed eggserve's hardened path validation and policy
//! enforcement without pulling in the full HTTP service layer. It re-exports
//! the core types with invariant-focused documentation.
//!
//! Before 1.0, every public type or function in this crate is best-effort
//! and may change without a major version bump. See
//! `docs/release-criteria.md` for the 1.0 freeze plan.

pub mod config;
pub(crate) mod error;
pub(crate) mod fs;
pub mod limits;
pub(crate) mod mime;
pub(crate) mod path;
pub mod policy;
pub mod primitives;
pub(crate) mod response;
pub mod server;
pub mod service;

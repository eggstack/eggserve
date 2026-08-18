//! Hardened static-serving primitives for eggserve.
//!
//! # Public API status (alpha)
//!
//! The public surface is intentionally conservative during the alpha period.
//! Modules and types are divided into three buckets:
//!
//! - **Semver-considered (pre-1.0)**: [`config`], [`limits`], [`policy`], and
//!   [`primitives`]. These are the intended public facades, but a minor
//!   release may still make breaking changes before 1.0.
//! - **Experimental**: [`server`]. The runtime and service boundary is
//!   exposed for Rust embedders and may change independently before 1.0.
//! - **Internal**: [`fs`], [`path`], [`response`], MIME detection, and the
//!   error taxonomy. These are not part of the public API and are not
//!   re-exported. External callers should not depend on them.
//!
//! Start with [`primitives`] when using the library without starting a
//! server. Use [`server`] for the experimental transport-owning runtime; the
//! repository's `static_server` and `custom_service` examples show the two
//! supported embedding paths.
//!
//! # Primitives facade
//!
//! The [`primitives`] module is the **intended public boundary** for Rust
//! consumers that want to embed eggserve's hardened path validation and policy
//! enforcement without pulling in the full HTTP service layer. It re-exports
//! the core types with invariant-focused documentation.
//!
//! Before 1.0, every public type or function in this crate may change without
//! a major version bump. See [docs/release-process.md](docs/release-process.md)
//! for the manual release procedure.

pub mod config;
pub(crate) mod fs;
pub mod limits;
pub(crate) mod mime;
pub mod ops;
pub(crate) mod path;
pub mod policy;
pub mod primitives;
pub(crate) mod response;
pub mod server;
#[cfg(feature = "tls")]
pub mod tls;

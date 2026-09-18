//! Compatibility facade (Plan 219: implementation authority moved to `eggserve-static`).
//!
//! Secure-root resolution, resolved-file/directory capabilities, MIME
//! selection, and the `resolve_and_plan` helper are implemented once in
//! [`eggserve_static`]. This module re-exports that implementation so existing
//! `eggserve_core::primitives::{SecureRoot, ConfinedPath, ...}` paths keep
//! working through the 0.x line. Security fixes land once, in the static
//! authority.
//!
//! A resolved file remains an opened capability: the re-exported
//! [`ResolvedFile`] carries the handle opened during confined resolution and
//! never reconstructs an absolute path to reopen it.

pub use eggserve_static::{
    resolve_and_plan, ResolveAndPlanError, ResolvedDirectory, ResolvedFile, ResolvedResource,
    ResourceDeniedReason, SecureRoot,
};

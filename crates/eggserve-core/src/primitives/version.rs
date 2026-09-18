//! Compatibility re-export (Plan 217: version moved to direct crates).
//!
//! The canonical version is owned by
//! [`eggserve_primitives::version`]; this module re-exports that
//! implementation so existing `eggserve_core::primitives::version::`
//! paths keep working. There is one version authority, not two.
//!
//! Hyper conversion (`TryFrom<hyper::Version>`) lived here for the 0.1
//! inbound adapter; it moves to the server transport boundary
//! (`eggserve-server` connection pipeline) so the canonical crate stays
//! Hyper-free. Use the runtime pipeline for Hyper interop.

pub use eggserve_primitives::version::*;

//! Compatibility re-export (Plan 216: authority moved to direct crates).
//!
//! The duplicate-preserving header block is owned by
//! [`eggserve_primitives::header_block`]; this module re-exports that
//! implementation so existing `eggserve_core::primitives::header_block::`
//! paths keep working. There is one validation authority, not two — the
//! tunnel handshake validator and the H1 classifier both run on these
//! shared types.

pub use eggserve_primitives::header_block::*;

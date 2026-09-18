//! Compatibility re-export (Plan 217: request bodies moved to direct crates).
//!
//! The canonical one-shot body is owned by
//! [`eggserve_primitives::request_body`]; this module re-exports that
//! implementation so existing `eggserve_core::primitives::request_body::`
//! paths keep working. There is one body/lifecycle authority, not two.
//! The public runtime-adapter API (`WireTrailerSlot`/`new_wire_slot`,
//! `IncomingError`, `from_incoming*`) lives in the direct crate for the
//! connection driver.

pub use eggserve_primitives::request_body::*;

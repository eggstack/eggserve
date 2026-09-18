//! Compatibility re-export (Plan 217: response streams moved to direct crates).
//!
//! The canonical stream vocabulary is owned by
//! [`eggserve_primitives::response_stream`]; this module re-exports that
//! implementation so existing
//! `eggserve_core::primitives::response_stream::` paths keep working.
//! There is one stream authority, not two. The public runtime-adapter API
//! (`TrailerFuture`/`ByteStream`/`into_parts`) lives in the direct crate
//! for the connection driver.

pub use eggserve_primitives::response_stream::*;

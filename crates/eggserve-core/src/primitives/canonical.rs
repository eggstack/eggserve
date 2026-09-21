//! Compatibility re-export (Plan 217: canonical responses moved to direct crates).
//!
//! The canonical response vocabulary (status, headers, body, normalization)
//! is owned by [`eggserve_primitives::canonical`]; this module re-exports
//! that implementation so existing `eggserve_core::primitives::canonical::`
//! paths keep working. There is one normalization authority, not two.
//!
//! Hyper conversion (`to_hyper_response` + file-stream overloads) is owned
//! by [`eggserve_server::adapters`]; the compatibility paths below delegate
//! to that single authority (same response type, no second framing
//! implementation). `runtime_error_with_policy` is canonical (direct) and
//! re-exported for the pipeline.

pub use eggserve_primitives::canonical::*;
pub use eggserve_server::adapters::to_hyper_response;

// H2-gated file-stream overload: the H2 pipeline calls it through the
// compatibility path, so the re-export exists exactly when H2 compiles.
#[cfg(feature = "http2")]
pub(crate) use eggserve_server::adapters::to_hyper_response_with_file_stream_semaphore_and_chunk_size;

/// Compatibility submodule preserving the 0.1
/// `canonical::adapters::to_hyper_response` path.
///
/// Thin delegation to the direct authority; no second conversion lives here.
pub mod adapters {
    pub use eggserve_server::adapters::{
        to_hyper_response, to_hyper_response_with_file_stream_semaphore,
        to_hyper_response_with_file_stream_semaphore_and_chunk_size,
        to_hyper_response_without_origin_date,
    };
}

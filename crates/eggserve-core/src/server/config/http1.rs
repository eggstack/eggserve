//! HTTP/1 parser projection (Plan 206 Track E).
//!
//! Owns the internal [`Http1Config`] projection from compatibility fields
//! on [`super::runtime::RuntimeConfig`]. No second defaults table: values
//! project from the Plan 179 shared authority.

/// HTTP/1-only parser and framing settings projected from the compatibility
/// fields on [`RuntimeConfig`]. Keeping this internal projection lets future
/// protocol configs own their knobs without duplicating Plan 179 defaults or
/// changing every existing `RuntimeConfig` literal before the 0.2 transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Http1Config {
    pub(crate) max_buf_size: usize,
    pub(crate) max_headers: usize,
}

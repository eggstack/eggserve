//! Compatibility re-export (Plan 217: request head moved to direct crates).
//!
//! The canonical head is owned by
//! [`eggserve_primitives::request_head`]; this module re-exports that
//! implementation so existing `eggserve_core::primitives::request_head::`
//! paths keep working. There is one head authority, not two.
//!
//! The 0.1 Hyper inbound adapter (`RequestHead::try_from_hyper`) moves to
//! the server transport boundary so the canonical type stays Hyper-free.
//! Runtime pipelines convert via their own validated adapter; tests should
//! construct heads via `RequestHead::new`/`new_with_authority`.

pub use eggserve_primitives::request_head::*;

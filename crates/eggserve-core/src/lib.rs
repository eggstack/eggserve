//! Hardened static-serving primitives for eggserve.

pub mod config;
pub(crate) mod error;
pub mod fs;
pub mod limits;
pub(crate) mod mime;
pub(crate) mod path;
pub mod policy;
pub(crate) mod response;
pub use response::BoxBodyInner;
pub mod service;
pub mod telemetry;

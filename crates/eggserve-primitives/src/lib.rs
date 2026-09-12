//! Dependency-light canonical HTTP contracts for EggServe consumers.
//!
//! This crate deliberately contains no transport, async-runtime, TLS, QUIC,
//! or filesystem dependency. It is the leaf layer for application-facing
//! metadata and request/response values. The compatibility `eggserve-core`
//! crate continues to expose its historical, richer `primitives` module for
//! the 0.1 series.

pub mod http;
pub mod limits;
pub mod policy;
pub mod proxy;
pub mod request;
pub mod response;

pub use http::{Header, HeaderBlock, HeaderError, HttpVersion, Method, MethodError};
pub use limits::Limits;
pub use policy::{ErrorPolicy, RequestPolicy};
pub use proxy::{IpPrefix, ProxyProvenance, TrustedProxyConfig};
pub use request::{Request, RequestBody, RequestHead, RequestTarget};
pub use response::{BodyLength, Response, ResponseBody, ResponseError, StatusCode};

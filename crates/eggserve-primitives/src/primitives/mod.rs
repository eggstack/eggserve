//! The canonical application-facing model.

pub mod authority;
pub mod body;
pub mod canonical;
pub mod connection_info;
pub mod header_block;
pub mod http;
pub mod incomplete_body_policy;
pub mod interim;
pub mod limits;
pub mod method;
pub mod policy;
pub mod proxy;
pub mod request;
pub mod request_body;
pub mod request_body_error;
pub mod request_body_policy;
pub mod request_context;
pub mod request_head;
pub mod request_lifecycle;
pub mod request_target;
pub mod response;
pub mod response_stream;
pub mod trailers;
pub mod version;

pub use authority::{Authority, AuthorityError};
pub use body::{BodyKind, BodySource, BodySourceError};
pub use canonical::{
    is_hop_by_hop_header, normalize_metadata, normalize_response, BodyLength, NormalizeRequest,
    Response, ResponseBody, ResponseBuilder, ResponseConstructionError, ResponseStream,
    ResponseStreamError, StatusCode,
};
pub use connection_info::{ConnectionInfo, Scheme, SocketEndpoints, TlsInfo};
pub use header_block::{
    DuplicateHeaderError, HeaderBlock, HeaderError, HeaderField, HeaderName, HeaderValue,
    HeaderValueTextError,
};
pub use http::{
    validate_method, validate_request_body, validate_request_target, ReadOnlyMethod,
    RequestValidationError,
};
pub use incomplete_body_policy::IncompleteBodyPolicy;
pub use interim::{ExpectDecision, InterimDisposition, InterimError, InterimLimits, InterimSender};
pub use limits::{Limits, LimitsError};
pub use method::{Method, MethodError};
pub use policy::{
    DirectoryListingPolicy, DotfilePolicy, ErrorRepresentationPolicy, StaticMetadataPolicy,
    StaticPolicy, SymlinkPolicy,
};
pub use proxy::{
    derive_forwarded_effective, parse_proxy_v1_line, parse_proxy_v2_header, ForwardedConfig,
    ForwardedEffective, ForwardedRejection, IpPrefix, ProxyEndpoints, ProxyParseError,
    ProxyProtocolConfig, ProxySourceKind, TrustedProxyConfig, TrustedProxyConfigError,
};
pub use request::Request;
pub use request_body::{BodyState, RequestBody};
pub use request_body_error::RequestBodyError;
pub use request_body_policy::RequestBodyPolicy;
pub use request_context::RequestContext;
pub use request_head::RequestHead;
pub use request_lifecycle::{RequestCancellationReason, RequestLifecycle};
pub use request_target::{RequestTarget, RequestTargetError};
pub use response::{
    BodyPlan, ConditionalRequestOutcome, FileRange, HeaderMapPlan, RangeRequestOutcome,
    ResponseHeader, ResponseStatus, StaticResponsePlan,
};
pub use response_stream::MAX_RESPONSE_STREAM_CHUNK_BYTES;
pub use trailers::{
    is_forbidden_trailer_field, trailer_block_bytes, validate_trailers, TrailerLimits,
    TrailerValidationError, Trailers,
};
pub use version::{HttpVersion, HttpVersionError};

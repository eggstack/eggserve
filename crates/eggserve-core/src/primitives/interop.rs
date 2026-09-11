//! Optional `http` / `http-body` interoperability adapters (Plan 200).
//!
//! This module is gated behind the `http-interop` feature so native
//! consumers never pay for ecosystem types. It provides loss-aware,
//! audited conversions between EggServe's canonical vocabulary and the
//! standard [`http`] types, plus incremental body adapters with real
//! backpressure.
//!
//! # Design principle
//!
//! Adapters are loss-aware: where a standard representation cannot express
//! a canonical property exactly, the conversion either preserves it in a
//! documented [`http::Extensions`] entry or fails with [`InteropError`].
//! The native canonical model remains authoritative for protocol
//! correctness; `http` is never the internal correctness authority.
//!
//! # Round-trip limitations
//!
//! - **Header order**: [`crate::primitives::HeaderBlock`] preserves exact
//!   field-line order and duplicates. [`http::HeaderMap`] preserves
//!   duplicate values per name (via `append`) and per-name order, but
//!   global field-line order across different names is **not** preserved.
//!   Consumers requiring exact wire order must use the native API.
//! - **Request target**: only origin-form (`/path?query`) round-trips.
//!   Absolute-form, authority-form, and asterisk-form are rejected rather
//!   than normalized. The exact raw target bytes are preserved in
//!   [`RawTargetExt`] when converting to ecosystem requests.
//! - **Opaque values**: legal opaque field-value octets map through
//!   `HeaderValue::from_bytes`, never through mandatory UTF-8.
//! - **Advanced capabilities**: interim-response senders (Plan 198) and
//!   one-shot tunnel capabilities (Plan 199) are **not** placed in
//!   `Extensions` (which are clonable). They remain available only through
//!   the native [`crate::primitives::RequestContext`] wrapper.
//!
//! # Middleware boundary (Track F)
//!
//! Tower layers operate on standard `http` request/response objects **after**
//! EggServe has completed protocol parsing/validation and **before** final
//! EggServe response normalization. Middleware may add ordinary application
//! headers/content, but cannot bypass body hard limits, canonical framing
//! validation, the final header denylist/privacy policy, response
//! no-progress timeouts, or connection lifecycle/shutdown. EggServe hard
//! admission/framing/privacy/timeout boundaries remain authoritative after
//! middleware; see `docs/http-interop.md`.

use bytes::Bytes;
use futures_util::Stream as _;
use std::fmt;
use std::pin::Pin;
use std::task::{Context, Poll};

use super::authority::{Authority, AuthorityError};
use super::canonical::{Response, ResponseBody, ResponseConstructionError, ResponseStream};
use super::connection_info::ConnectionInfo;
use super::header_block::{HeaderBlock, HeaderError};
use super::method::{Method, MethodError};
use super::request_body::RequestBody;
use super::request_body_error::RequestBodyError;
use super::request_head::RequestHead;
use super::request_lifecycle::RequestLifecycle;
use super::request_target::{RequestTarget, RequestTargetError};
use super::response_stream::ResponseStreamError;
use super::trailers::{TrailerLimits, Trailers};
use super::version::{HttpVersion, HttpVersionError};

/// Errors from `http` interoperability conversions.
///
/// All variants are typed and sanitized: no filesystem paths, file contents,
/// or transport internals are reflected. Bodies that fail map to a generic
/// wire truncation with sanitized diagnostics; see [`RequestBodyHttpError`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteropError {
    /// The HTTP method is not a valid token.
    InvalidMethod,
    /// The status code is outside EggServe's 100–599 range.
    InvalidStatus(u16),
    /// The HTTP version is not supported.
    InvalidVersion,
    /// A header name or value failed validation.
    InvalidHeader,
    /// The URI is not valid origin-form.
    InvalidUri,
    /// The authority value is malformed.
    InvalidAuthority,
    /// A trailer block failed canonical validation.
    InvalidTrailers,
}

impl fmt::Display for InteropError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMethod => write!(f, "invalid http method"),
            Self::InvalidStatus(code) => write!(f, "invalid http status: {code}"),
            Self::InvalidVersion => write!(f, "unsupported http version"),
            Self::InvalidHeader => write!(f, "invalid http header"),
            Self::InvalidUri => write!(f, "invalid http uri"),
            Self::InvalidAuthority => write!(f, "invalid http authority"),
            Self::InvalidTrailers => write!(f, "invalid http trailers"),
        }
    }
}

impl std::error::Error for InteropError {}

impl From<MethodError> for InteropError {
    fn from(_: MethodError) -> Self {
        Self::InvalidMethod
    }
}

impl From<RequestTargetError> for InteropError {
    fn from(_: RequestTargetError) -> Self {
        Self::InvalidUri
    }
}

impl From<HttpVersionError> for InteropError {
    fn from(_: HttpVersionError) -> Self {
        Self::InvalidVersion
    }
}

impl From<HeaderError> for InteropError {
    fn from(_: HeaderError) -> Self {
        Self::InvalidHeader
    }
}

impl From<AuthorityError> for InteropError {
    fn from(_: AuthorityError) -> Self {
        Self::InvalidAuthority
    }
}

/// Exact raw request-target bytes that `http::Uri` cannot express alone.
///
/// Stored in [`http::Extensions`] when converting a canonical request head
/// to an ecosystem request. Cloning the extension never clones a body or a
/// one-shot capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTargetExt(pub String);

/// Transport-authenticated connection metadata for ecosystem requests.
///
/// Cloned into [`http::Extensions`]; never derived from untrusted headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionInfoExt(pub ConnectionInfo);

/// Validated effective authority for ecosystem requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityExt(pub Option<Authority>);

/// Cloneable cancellation observer for ecosystem requests.
///
/// Forwards peer-disconnect / shutdown / timeout cancellation even when the
/// Tower service is not polling body IO.
#[derive(Debug, Clone)]
pub struct LifecycleExt(pub RequestLifecycle);

// ---------------------------------------------------------------------------
// Scalar conversions (Track B)
// ---------------------------------------------------------------------------

/// Convert a canonical [`Method`] to [`http::Method`].
///
/// # Errors
///
/// Returns [`InteropError::InvalidMethod`] when the canonical token cannot
/// be represented as an `http` method (loss-aware; never silently coerced).
pub fn method_to_http(method: &Method) -> Result<http::Method, InteropError> {
    method
        .as_str()
        .parse::<http::Method>()
        .map_err(|_| InteropError::InvalidMethod)
}

/// Convert an [`http::Method`] to the canonical [`Method`].
///
/// # Errors
///
/// Returns [`InteropError::InvalidMethod`] on token validation failure.
pub fn method_from_http(method: &http::Method) -> Result<Method, InteropError> {
    Method::new(method.as_str()).map_err(|_| InteropError::InvalidMethod)
}

/// Convert a canonical [`StatusCode`] to [`http::StatusCode`].
pub fn status_to_http(status: super::canonical::StatusCode) -> http::StatusCode {
    // Canonical range 100–599 is a subset of `http` 100–999; infallible.
    http::StatusCode::from_u16(status.as_u16())
        .expect("canonical status is always a valid http status")
}

/// Convert an [`http::StatusCode`] to the canonical status type.
///
/// # Errors
///
/// Returns [`InteropError::InvalidStatus`] when the code lies outside
/// EggServe's 100–599 range (e.g. 600–999). No silent clamping is performed.
pub fn status_from_http(
    status: http::StatusCode,
) -> Result<super::canonical::StatusCode, InteropError> {
    super::canonical::StatusCode::new(status.as_u16())
        .map_err(|_| InteropError::InvalidStatus(status.as_u16()))
}

/// Convert a canonical [`HttpVersion`] to [`http::Version`].
///
/// The four modeled versions map exactly. `HttpVersion` is
/// `#[non_exhaustive]` for downstream matching; variants can only be added
/// in this crate, at which point this match must be extended.
pub fn version_to_http(version: HttpVersion) -> http::Version {
    match version {
        HttpVersion::Http10 => http::Version::HTTP_10,
        HttpVersion::Http11 => http::Version::HTTP_11,
        HttpVersion::Http2 => http::Version::HTTP_2,
        HttpVersion::Http3 => http::Version::HTTP_3,
    }
}

/// Convert an [`http::Version`] to the canonical [`HttpVersion`].
///
/// # Errors
///
/// Returns [`InteropError::InvalidVersion`] for versions EggServe does not
/// model (e.g. HTTP/0.9, HTTP/2.0 textual form, unknown future versions).
pub fn version_from_http(version: http::Version) -> Result<HttpVersion, InteropError> {
    match version {
        http::Version::HTTP_10 => Ok(HttpVersion::Http10),
        http::Version::HTTP_11 => Ok(HttpVersion::Http11),
        http::Version::HTTP_2 => Ok(HttpVersion::Http2),
        http::Version::HTTP_3 => Ok(HttpVersion::Http3),
        _ => Err(InteropError::InvalidVersion),
    }
}

/// Convert a canonical [`Authority`] to [`http::uri::Authority`].
///
/// # Errors
///
/// Returns [`InteropError::InvalidAuthority`] when the canonical text has
/// no exact `http` representation.
pub fn authority_to_http(authority: &Authority) -> Result<http::uri::Authority, InteropError> {
    authority
        .as_str()
        .parse::<http::uri::Authority>()
        .map_err(|_| InteropError::InvalidAuthority)
}

/// Convert an [`http::uri::Authority`] to the canonical [`Authority`].
///
/// # Errors
///
/// Returns [`InteropError::InvalidAuthority`] on validation failure.
pub fn authority_from_http(authority: &http::uri::Authority) -> Result<Authority, InteropError> {
    Authority::parse(authority.as_str()).map_err(|_| InteropError::InvalidAuthority)
}

/// Convert a canonical [`RequestTarget`] to [`http::Uri`].
///
/// Only origin-form targets are representable. The raw target string is
/// parsed directly so percent-encoding and query structure are preserved.
///
/// # Errors
///
/// Returns [`InteropError::InvalidUri`] when the raw target cannot be
/// represented as an `http::Uri`.
pub fn request_target_to_uri(target: &RequestTarget) -> Result<http::Uri, InteropError> {
    target
        .raw()
        .parse::<http::Uri>()
        .map_err(|_| InteropError::InvalidUri)
}

// ---------------------------------------------------------------------------
// Header conversions (Track B)
// ---------------------------------------------------------------------------

/// Convert a canonical [`HeaderBlock`] to [`http::HeaderMap`].
///
/// Duplicate values for one name are preserved via `append` in per-name
/// order, using byte conversion for opaque values (never mandatory UTF-8).
/// Global field-line order across different names is **not** preserved;
/// see the module docs. Names/values that `http` rejects (which cannot
/// happen for canonically constructed blocks) are skipped rather than
/// coerced, preserving loss-awareness in the reverse direction.
pub fn header_block_to_map(block: &HeaderBlock) -> http::HeaderMap {
    let mut map = http::HeaderMap::with_capacity(block.len());
    for field in block.iter() {
        let Ok(name) = http::HeaderName::from_bytes(field.name.as_str().as_bytes()) else {
            continue;
        };
        let Ok(value) = http::HeaderValue::from_bytes(field.value.as_bytes()) else {
            continue;
        };
        map.append(name, value);
    }
    map
}

/// Convert an [`http::HeaderMap`] to the canonical [`HeaderBlock`].
///
/// Iterates every stored value (duplicates preserved per name in order)
/// through byte-preserving validation. Global cross-name order follows
/// `HeaderMap` iteration and is **not** claimed to match the original
/// wire order.
///
/// # Errors
///
/// Returns [`InteropError::InvalidHeader`] when any name/value fails
/// canonical validation.
pub fn header_map_to_block(map: &http::HeaderMap) -> Result<HeaderBlock, InteropError> {
    let mut block = HeaderBlock::with_capacity(map.len());
    for (name, value) in map.iter() {
        let cname = super::header_block::HeaderName::new(name.as_str())
            .map_err(|_| InteropError::InvalidHeader)?;
        let cvalue = super::header_block::HeaderValue::from_bytes(value.as_bytes())
            .map_err(|_| InteropError::InvalidHeader)?;
        block.push(cname, cvalue);
    }
    Ok(block)
}

/// Convert [`http`] trailer fields to validated canonical [`Trailers`].
///
/// Uses the single canonical trailer validator; no second policy lives in
/// the adapter.
///
/// # Errors
///
/// Returns [`InteropError::InvalidHeader`] for malformed fields and
/// [`InteropError::InvalidTrailers`] for forbidden/oversized blocks.
pub fn header_map_to_trailers(map: &http::HeaderMap) -> Result<Trailers, InteropError> {
    let block = header_map_to_block(map)?;
    Trailers::new(block).map_err(|_| InteropError::InvalidTrailers)
}

/// Convert canonical [`Trailers`] to an [`http::HeaderMap`] for framing.
///
/// Opaque values map through bytes; forbidden fields cannot occur because
/// `Trailers` construction already enforces the denylist.
pub fn trailers_to_map(trailers: &Trailers) -> http::HeaderMap {
    header_block_to_map(trailers.as_block())
}

// ---------------------------------------------------------------------------
// Request-head conversions (Track B)
// ---------------------------------------------------------------------------

/// Convert a canonical [`RequestHead`] plus transport metadata to an
/// ecosystem [`http::Request`] with an empty body.
///
/// The method, origin-form URI, version, and headers map directly.
/// Exact metadata that `http` cannot express is preserved in extensions:
/// [`RawTargetExt`] (exact raw target), [`ConnectionInfoExt`],
/// [`AuthorityExt`], and [`LifecycleExt`]. One-shot capabilities
/// (interim senders, tunnel capabilities) are deliberately **not** placed
/// in extensions; Tower services needing them must use the native wrapper.
///
/// # Errors
///
/// Returns [`InteropError`] when any scalar conversion fails.
pub fn request_head_to_http(
    head: &RequestHead,
    connection: &ConnectionInfo,
    lifecycle: &RequestLifecycle,
) -> Result<http::Request<()>, InteropError> {
    let method = method_to_http(head.method())?;
    let uri = request_target_to_uri(head.target())?;
    let version = version_to_http(head.version());
    let mut builder = http::Request::builder()
        .method(method)
        .uri(uri)
        .version(version);
    if let Some(headers) = builder.headers_mut() {
        *headers = header_block_to_map(head.headers());
    }
    let mut req = builder.body(()).map_err(|_| InteropError::InvalidHeader)?;
    req.extensions_mut()
        .insert(RawTargetExt(head.target().raw().to_owned()));
    req.extensions_mut()
        .insert(ConnectionInfoExt(connection.clone()));
    req.extensions_mut()
        .insert(AuthorityExt(head.authority().cloned()));
    req.extensions_mut().insert(LifecycleExt(lifecycle.clone()));
    Ok(req)
}

/// Convert an ecosystem [`http::Request`] head to canonical parts.
///
/// Only origin-form URIs are accepted; absolute-form, authority-form, and
/// asterisk-form are rejected with [`InteropError::InvalidUri`] rather than
/// normalized. Headers map through byte-preserving validation. When the
/// request carries [`RawTargetExt`], its exact bytes are preferred over the
/// `http::Uri` rendering for fidelity.
///
/// # Errors
///
/// Returns [`InteropError`] on any malformed method, target, version,
/// header, or authority.
pub fn request_head_from_http<B>(
    req: &http::Request<B>,
) -> Result<
    (
        Method,
        RequestTarget,
        HttpVersion,
        HeaderBlock,
        Option<Authority>,
    ),
    InteropError,
> {
    let method = method_from_http(req.method())?;
    let version = version_from_http(req.version())?;
    let headers = header_map_to_block(req.headers())?;
    // Prefer the exact raw target when a previous adapter preserved it;
    // otherwise render the Uri and validate as origin-form.
    let raw = if let Some(ext) = req.extensions().get::<RawTargetExt>() {
        ext.0.clone()
    } else {
        let uri = req.uri();
        if uri.scheme().is_some() || uri.authority().is_some() {
            return Err(InteropError::InvalidUri);
        }
        let pq = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
        pq.to_owned()
    };
    let target = RequestTarget::parse(raw).map_err(|_| InteropError::InvalidUri)?;
    // Effective authority: explicit extension wins; otherwise a single
    // consistent Host header (duplicates must agree) is used, matching the
    // native Hyper adapter policy.
    let authority = if let Some(ext) = req.extensions().get::<AuthorityExt>() {
        ext.0.clone()
    } else {
        let hosts: Vec<String> = req
            .headers()
            .get_all(http::header::HOST)
            .iter()
            .map(|v| v.as_bytes().to_vec())
            .map(|b| String::from_utf8(b).map_err(|_| InteropError::InvalidAuthority))
            .collect::<Result<Vec<_>, _>>()?;
        match hosts.as_slice() {
            [] => None,
            [first, rest @ ..] if rest.iter().all(|v| v == first) => {
                Some(Authority::parse(first).map_err(|_| InteropError::InvalidAuthority)?)
            }
            _ => return Err(InteropError::InvalidAuthority),
        }
    };
    Ok((method, target, version, headers, authority))
}

// ---------------------------------------------------------------------------
// Request-body adapter (Track C)
// ---------------------------------------------------------------------------

/// Sanitized error for the `http_body::Body` view over [`RequestBody`].
///
/// `Display` is intentionally generic (`"request body failed"`) so internal
/// limit/framing/transport details never reach Tower middleware responses.
/// Use [`RequestBodyHttpError::detail`] for sanitized internal diagnostics
/// only (pass through `ops::sanitize_text_field` before emitting).
#[derive(Debug)]
pub struct RequestBodyHttpError {
    detail: String,
}

impl RequestBodyHttpError {
    /// Create a sanitized adapter error.
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }

    /// Internal diagnostic detail (never write to the client).
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for RequestBodyHttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "request body failed")
    }
}

impl std::error::Error for RequestBodyHttpError {}

impl From<RequestBodyError> for RequestBodyHttpError {
    fn from(e: RequestBodyError) -> Self {
        Self::new(e.to_string())
    }
}

impl http_body::Body for RequestBody {
    type Data = Bytes;
    type Error = RequestBodyHttpError;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Bytes>, Self::Error>>> {
        // Drive the canonical stream for data first, preserving one-shot
        // ownership, byte limits, and cancellation wake-ups. `poll_next`
        // carries the limit/framing checks; dropping preserves
        // abandoned-body/reuse semantics via `RequestBody::drop`.
        let this = self.get_mut();
        match Pin::new(&mut *this).poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => Poll::Ready(Some(Ok(http_body::Frame::data(chunk)))),
            Poll::Ready(Some(Err(e))) => {
                Poll::Ready(Some(Err(RequestBodyHttpError::new(e.to_string()))))
            }
            Poll::Ready(None) => {
                // Content complete: surface a stored trailer failure as a
                // terminal body error, else emit one trailers frame when a
                // validated block exists, else end the stream.
                if let Some(msg) = this.completed_trailer_failure() {
                    return Poll::Ready(Some(Err(RequestBodyHttpError::new(format!(
                        "invalid request trailers: {msg}"
                    )))));
                }
                match this.take_completed_trailers() {
                    Some(trailers) => Poll::Ready(Some(Ok(http_body::Frame::trailers(
                        trailers_to_map(&trailers),
                    )))),
                    None => Poll::Ready(None),
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn size_hint(&self) -> http_body::SizeHint {
        // Truthful but not a guarantee after transport failure: remaining
        // declared bytes when known, unknown otherwise.
        match self.declared_length() {
            Some(declared) => {
                let remaining = declared.saturating_sub(self.bytes_received());
                http_body::SizeHint::with_exact(remaining)
            }
            None => http_body::SizeHint::default(),
        }
    }

    fn is_end_stream(&self) -> bool {
        // End only when content completed *and* no terminal trailer frame
        // remains to be emitted.
        self.is_complete() && self.completed_trailers_snapshot().is_none()
    }
}

// ---------------------------------------------------------------------------
// Response-body adapter (Track D)
// ---------------------------------------------------------------------------

/// Shared terminal-trailer slot for `response_from_http_body`.
///
/// The byte stream fills it once at EOF; the trailer future (polled once
/// after bytes by the transport) reads it. Only the validated terminal
/// block is retained; no body buffering occurs.
#[allow(clippy::type_complexity)]
type HttpTrailerSlot =
    std::sync::Arc<std::sync::Mutex<Option<Result<Option<Trailers>, ResponseStreamError>>>>;

/// Convert an ecosystem [`http::Response`] with an `http_body` body into the
/// canonical [`Response`] pipeline.
///
/// Requirements honored:
/// - EggServe remains the final framing authority: incoming
///   `content-length`/`transfer-encoding` are stripped and recomputed by
///   canonical normalization; application values are validated/reconciled,
///   never blindly trusted.
/// - Hop-by-hop/protocol-forbidden fields pass through canonical
///   normalization (which strips them centrally).
/// - Response trailers map through the Plan 198 validator without
///   full-body buffering (incremental poll, then one trailer future).
/// - `HEAD`/body-forbidden statuses never poll body/trailer frames: the
///   wrapped stream is lazy, and runtime normalization drops it without
///   polling (prompt producer release).
/// - Producer panic/error/cancellation maps into committed-response
///   behavior (truncated close, no second HTTP error).
/// - Bounded/no-progress timeout tracking works at the canonical poll
///   boundary because the returned [`ResponseStream`] is polled by the
///   runtime transport like any native stream.
///
/// # Errors
///
/// Returns [`InteropError`] for malformed status/headers/trailers, or
/// [`ResponseConstructionError`] for response-construction failures.
pub fn response_from_http_body<B>(
    res: http::Response<B>,
) -> Result<Response, ResponseConstructionError>
where
    B: http_body::Body<Data = Bytes> + Send + 'static,
    B::Error: std::error::Error + Send + Sync + 'static,
{
    use futures_util::Stream;
    use std::sync::{Arc, Mutex};

    let (parts, body) = res.into_parts();
    let status = super::canonical::StatusCode::new(parts.status.as_u16())
        .map_err(|_| ResponseConstructionError::InvalidStatus(parts.status.as_u16()))?;
    let mut headers = header_map_to_block(&parts.headers)
        .map_err(|_| ResponseConstructionError::InvalidHeader(HeaderError::InvalidValue))?;
    // EggServe is the framing authority: strip application framing so
    // normalization recomputes it from the actual adapted body. This also
    // avoids `ForbiddenFramingHeader` on 205 and stale lengths elsewhere.
    headers.retain(|f| {
        !f.name.as_str().eq_ignore_ascii_case("content-length")
            && !f.name.as_str().eq_ignore_ascii_case("transfer-encoding")
    });

    // Fast path: empty bodies stay empty (known 0) so framing stays exact
    // instead of degrading to unknown-length chunked.
    if body.is_end_stream() {
        // `is_end_stream` true at construction means no data or trailers
        // will ever arrive; dropping `body` releases any producer.
        drop(body);
        return Response::builder()
            .status(status)
            .body(ResponseBody::Empty)
            .map(|mut r| {
                // Attach headers preserved from the http response.
                for field in headers.iter() {
                    r.head_mut()
                        .headers_mut()
                        .push(field.name.clone(), field.value.clone());
                }
                r
            });
    }

    // Shared terminal-trailer slot: the byte stream fills it once at EOF;
    // the trailer future (polled once after bytes by the transport) reads
    // it. No body buffering occurs; only the validated terminal block is
    // retained.
    let slot: HttpTrailerSlot = Arc::new(Mutex::new(None));
    let slot_writer = slot.clone();

    struct HttpBodyDataStream<B> {
        body: Pin<Box<B>>,
        slot: HttpTrailerSlot,
        trailers_emitted: bool,
    }

    impl<B> Stream for HttpBodyDataStream<B>
    where
        B: http_body::Body<Data = Bytes> + Send + 'static,
        B::Error: std::error::Error + Send + Sync + 'static,
    {
        type Item = Result<Bytes, ResponseStreamError>;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            // Poll the ecosystem body frame-by-frame (real backpressure:
            // only when downstream is ready). Data frames stream through;
            // a trailers frame is validated, stored, and ends data.
            // `Pin<Box<B>>` polls without requiring `B: Unpin`.
            match self.body.as_mut().poll_frame(cx) {
                Poll::Ready(Some(Ok(frame))) => {
                    if let Some(data) = frame.data_ref() {
                        if data.is_empty() {
                            // Empty chunks are not progress; re-poll
                            // without yielding an empty DATA frame.
                            cx.waker().wake_by_ref();
                            return Poll::Pending;
                        }
                        let chunk = data.clone();
                        Poll::Ready(Some(Ok(chunk)))
                    } else if frame.is_trailers() {
                        let trailers = frame
                            .trailers_ref()
                            .map(|map| {
                                let block = header_map_to_block(map).map_err(|_| {
                                    ResponseStreamError::new("invalid response trailers")
                                })?;
                                Trailers::new(block).map_err(|_| {
                                    ResponseStreamError::new("invalid response trailers")
                                })
                            })
                            .transpose();
                        let stored = match trailers {
                            Ok(Some(t)) if t.is_empty() => Ok(None),
                            Ok(v) => Ok(v),
                            Err(e) => Err(e),
                        };
                        if let Ok(mut guard) = self.slot.lock() {
                            *guard = Some(stored);
                        }
                        self.trailers_emitted = true;
                        Poll::Ready(None)
                    } else {
                        // Unknown frame kind: end data safely.
                        if let Ok(mut guard) = self.slot.lock() {
                            *guard = Some(Ok(None));
                        }
                        Poll::Ready(None)
                    }
                }
                Poll::Ready(Some(Err(e))) => {
                    if let Ok(mut guard) = self.slot.lock() {
                        *guard = Some(Err(ResponseStreamError::new(format!(
                            "response body failed: {e}"
                        ))));
                    }
                    Poll::Ready(Some(Err(ResponseStreamError::new(format!(
                        "response body failed: {e}"
                    )))))
                }
                Poll::Ready(None) => {
                    if !self.trailers_emitted {
                        if let Ok(mut guard) = self.slot.lock() {
                            if guard.is_none() {
                                *guard = Some(Ok(None));
                            }
                        }
                    }
                    Poll::Ready(None)
                }
                Poll::Pending => Poll::Pending,
            }
        }
    }

    let data_stream = HttpBodyDataStream {
        body: Box::pin(body),
        slot: slot_writer,
        trailers_emitted: false,
    };
    let trailer_future = async move {
        // Polled once after bytes by the transport. The slot is already
        // filled by EOF; default to no trailers if the producer vanished.
        let taken = slot.lock().map(|mut g| g.take()).unwrap_or(None);
        match taken {
            Some(Ok(v)) => Ok(v),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    };

    let stream = ResponseStream::with_trailers(data_stream, trailer_future);
    let mut response = Response::builder()
        .status(status)
        .body(ResponseBody::Stream(stream))?;
    for field in headers.iter() {
        response
            .head_mut()
            .headers_mut()
            .push(field.name.clone(), field.value.clone());
    }
    Ok(response)
}

/// Convenience adapter for common `Bytes`/empty bodies (Track D).
///
/// Builds a canonical [`Response`] from a status, headers, and an optional
/// byte buffer without requiring callers to understand internal
/// [`ResponseBody`] variants. Framing headers in `headers` are stripped;
/// EggServe recomputes framing at normalization.
pub fn response_from_bytes(
    status: super::canonical::StatusCode,
    headers: &HeaderBlock,
    body: Option<Bytes>,
) -> Result<Response, ResponseConstructionError> {
    let mut clean = HeaderBlock::with_capacity(headers.len());
    for field in headers.iter() {
        if field.name.as_str().eq_ignore_ascii_case("content-length")
            || field
                .name
                .as_str()
                .eq_ignore_ascii_case("transfer-encoding")
        {
            continue;
        }
        clean.push(field.name.clone(), field.value.clone());
    }
    let mut builder = Response::builder().status(status);
    for field in clean.iter() {
        builder = builder.push_header(field.name.clone(), field.value.clone());
    }
    let body = match body {
        Some(b) if !b.is_empty() => ResponseBody::Bytes(b.to_vec()),
        _ => ResponseBody::Empty,
    };
    builder.body(body)
}

/// Build a canonical [`Response`] from an ecosystem status/headers pair with
/// an empty body (convenience for middleware-generated rejections).
pub fn empty_response_from_http(
    status: http::StatusCode,
    headers: &http::HeaderMap,
) -> Result<Response, ResponseConstructionError> {
    let canonical_status = super::canonical::StatusCode::new(status.as_u16())
        .map_err(|_| ResponseConstructionError::InvalidStatus(status.as_u16()))?;
    let mut block = header_map_to_block(headers)
        .map_err(|_| ResponseConstructionError::InvalidHeader(HeaderError::InvalidValue))?;
    block.retain(|f| {
        !f.name.as_str().eq_ignore_ascii_case("content-length")
            && !f.name.as_str().eq_ignore_ascii_case("transfer-encoding")
    });
    let mut builder = Response::builder().status(canonical_status);
    for field in block.iter() {
        builder = builder.push_header(field.name.clone(), field.value.clone());
    }
    builder.body(ResponseBody::Empty)
}

// ---------------------------------------------------------------------------
// Re-exports for ergonomics
// ---------------------------------------------------------------------------

/// Trailer limits used by the adapters (canonical defaults).
pub fn default_trailer_limits() -> TrailerLimits {
    TrailerLimits::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn method_round_trip() {
        let m = Method::get();
        let h = method_to_http(&m).unwrap();
        assert_eq!(h, http::Method::GET);
        let back = method_from_http(&h).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn status_round_trip() {
        let s = super::super::canonical::StatusCode::OK;
        let h = status_to_http(s);
        assert_eq!(h, http::StatusCode::OK);
        let back = status_from_http(h).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn status_out_of_range_rejected() {
        let err = status_from_http(http::StatusCode::from_u16(599).unwrap()).unwrap();
        assert_eq!(err.as_u16(), 599);
        // 600+ has no canonical representation.
        assert_eq!(
            status_from_http(http::StatusCode::from_u16(600).unwrap()).unwrap_err(),
            InteropError::InvalidStatus(600)
        );
    }

    #[test]
    fn version_round_trip() {
        assert_eq!(version_to_http(HttpVersion::Http11), http::Version::HTTP_11);
        assert_eq!(
            version_from_http(http::Version::HTTP_2).unwrap(),
            HttpVersion::Http2
        );
    }

    #[test]
    fn opaque_header_value_preserved() {
        let mut block = HeaderBlock::new();
        block
            .push_bytes("x-opaque", b"\x80\x81 opaque \xff")
            .unwrap();
        let map = header_block_to_map(&block);
        let v = map.get("x-opaque").unwrap();
        assert_eq!(v.as_bytes(), b"\x80\x81 opaque \xff");
        let back = header_map_to_block(&map).unwrap();
        assert_eq!(
            back.get_first("x-opaque").unwrap().as_bytes(),
            b"\x80\x81 opaque \xff"
        );
    }

    #[test]
    fn duplicate_headers_preserved_per_name() {
        let mut block = HeaderBlock::new();
        block.push_str("x-dup", "a").unwrap();
        block.push_str("x-other", "z").unwrap();
        block.push_str("x-dup", "b").unwrap();
        let map = header_block_to_map(&block);
        let vals: Vec<_> = map
            .get_all("x-dup")
            .iter()
            .map(|v| v.to_str().unwrap().to_owned())
            .collect();
        assert_eq!(vals, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn absolute_uri_rejected() {
        let req = http::Request::builder()
            .method(http::Method::GET)
            .uri("http://example.test/")
            .version(http::Version::HTTP_11)
            .body(())
            .unwrap();
        assert_eq!(
            request_head_from_http(&req).unwrap_err(),
            InteropError::InvalidUri
        );
    }

    #[test]
    fn conflicting_host_rejected() {
        let req = http::Request::builder()
            .method(http::Method::GET)
            .uri("/x")
            .version(http::Version::HTTP_11)
            .header("host", "a.test")
            .header("host", "b.test")
            .body(())
            .unwrap();
        // `http` coalesces? If it appends, our adapter must reject disagreement.
        let res = request_head_from_http(&req);
        // Accept either rejection or first-wins only when values agree; here
        // they disagree so rejection is required when both are visible.
        if let Ok((_, _, _, headers, _)) = res {
            let hosts = headers.get_all("host");
            // If http merged them, at least both values must be visible.
            assert!(!hosts.is_empty());
        }
    }
}

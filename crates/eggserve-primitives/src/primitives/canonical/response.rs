//! Canonical response construction and normalization (Plan 206 Track C).
//!
//! Owns [`Response`], [`ResponseBuilder`], [`NormalizeRequest`],
//! [`normalize_response`]/[`normalize_metadata`] (the single
//! normalization authority), and the crate-internal
//! [`runtime_error_with_policy`] representation table shared by
//! H1/H2/H3. All producers converge here; `adapters` owns wire conversion.

use std::fmt;

use super::super::header_block::{HeaderBlock, HeaderName, HeaderValue};
use super::headers::ResponseHead;
use super::response_body::{BodyLength, ResponseBody};
use super::status::{ResponseConstructionError, StatusCode};

/// A canonical HTTP response.
///
/// Combines a [`ResponseHead`] (status + headers) with a [`ResponseBody`].
/// The body is one-shot: consuming the response via [`normalize_response`] or
/// transport conversion consumes the body.
///
/// Normalization is idempotent: [`normalize_response`] sets an internal flag
/// and a second call is a no-op. This lets the static service normalize
/// eagerly while the connection pipeline normalizes every service response
/// without double-suppression losing a HEAD length or inventing
/// `Content-Length` for unknown-length streams. Mutating via
/// [`Response::head_mut`] or [`Response::take_body`] clears the flag.
///
/// # Construction
///
/// Use [`Response::builder()`] for validated construction, or convert from
/// existing types via `From`/`Into`.
pub struct Response {
    head: ResponseHead,
    pub(super) body: Option<ResponseBody>,
    normalized: bool,
}

impl Response {
    /// Create a new response builder.
    pub fn builder() -> ResponseBuilder {
        ResponseBuilder {
            status: None,
            headers: HeaderBlock::new(),
        }
    }

    /// Returns a reference to the response head.
    pub fn head(&self) -> &ResponseHead {
        &self.head
    }

    /// Returns a mutable reference to the response head.
    ///
    /// This invalidates prior normalization.
    pub fn head_mut(&mut self) -> &mut ResponseHead {
        self.normalized = false;
        &mut self.head
    }

    /// Returns the status code.
    pub fn status(&self) -> StatusCode {
        self.head.status()
    }

    /// Returns a reference to the headers.
    pub fn headers(&self) -> &HeaderBlock {
        self.head.headers()
    }

    /// Returns true if normalize has been applied since last mutation.
    pub fn is_normalized(&self) -> bool {
        self.normalized
    }

    /// Tunnel handoff is owned by the transport runtime, so canonical
    /// responses do not carry a transport token.
    pub fn is_tunnel(&self) -> bool {
        false
    }

    /// Take the body out of the response, leaving an empty body.
    ///
    /// Returns `None` if the body was already consumed. Invalidates prior
    /// normalization.
    pub fn take_body(&mut self) -> Option<ResponseBody> {
        self.normalized = false;
        self.body.take()
    }

    /// Returns a reference to the body, if present.
    pub fn body(&self) -> Option<&ResponseBody> {
        self.body.as_ref()
    }

    /// Strip terminal response trailers, if any (H1 policy suppression).
    ///
    /// Drops the trailer producer without polling (deterministic release).
    /// Used when the request did not indicate trailer willingness (`TE:
    /// trailers`) or the version cannot carry trailers (HTTP/1.0). Invalidates
    /// prior normalization so framing is recomputed without trailers.
    pub fn strip_response_trailers(&mut self) {
        if let Some(ResponseBody::Stream(stream)) = self.body.as_mut() {
            // Drop without polling: `take_trailer_future` + drop.
            let _ = stream.take_trailer_future();
        }
        self.normalized = false;
    }

    /// Returns `true` when the response carries a terminal trailer source.
    pub fn has_response_trailers(&self) -> bool {
        matches!(self.body.as_ref(), Some(ResponseBody::Stream(s)) if s.has_trailers())
    }
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Response")
            .field("head", &self.head)
            .field("body", &self.body)
            .field("normalized", &self.normalized)
            .finish()
    }
}

/// Builder for constructing a [`Response`] with validated headers.
///
/// # Example
///
/// ```no_run
/// use eggserve_primitives::{Response, ResponseBody, StatusCode};
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let response = Response::builder()
///     .status(StatusCode::OK)
///     .header("content-type", "text/plain")?
///     .body(ResponseBody::Bytes(b"ok".to_vec()))?;
/// # let _ = response;
/// # Ok(())
/// # }
/// ```
pub struct ResponseBuilder {
    status: Option<StatusCode>,
    headers: HeaderBlock,
}

impl ResponseBuilder {
    /// Set the response status code.
    pub fn status(mut self, status: StatusCode) -> Self {
        self.status = Some(status);
        self
    }

    /// Add a validated header field.
    ///
    /// # Errors
    ///
    /// Returns an error if the header name or value is invalid.
    pub fn push_header(mut self, name: HeaderName, value: HeaderValue) -> Self {
        self.headers.push(name, value);
        self
    }

    /// Add a header from string slices, validating name and value.
    ///
    /// # Errors
    ///
    /// Returns an error if the header name or value is invalid (empty name,
    /// CR/LF/NUL in value, name exceeding 256 bytes).
    pub fn header(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, ResponseConstructionError> {
        let name = HeaderName::new(name)?;
        let value = HeaderValue::new(value)?;
        self.headers.push(name, value);
        Ok(self)
    }

    /// Build the response with the given body.
    ///
    /// # Errors
    ///
    /// Returns an error if no status code was set.
    pub fn body(self, body: ResponseBody) -> Result<Response, ResponseConstructionError> {
        let status = self
            .status
            .ok_or(ResponseConstructionError::InvalidStatus(0))?;
        Ok(Response {
            head: ResponseHead::new(status, self.headers),
            body: Some(body),
            normalized: false,
        })
    }

    /// Build the response with an empty body.
    pub fn empty(self) -> Result<Response, ResponseConstructionError> {
        self.body(ResponseBody::Empty)
    }
}

/// A normalization request describing the context for response normalization.
pub struct NormalizeRequest {
    /// Whether the original request was a HEAD request.
    pub is_head: bool,
}

impl NormalizeRequest {
    /// Create a new normalization request.
    pub fn new(is_head: bool) -> Self {
        Self { is_head }
    }
}

/// Normalize a response immediately before transport conversion.
///
/// This function applies the following rules:
///
/// 1. **HEAD suppression**: HEAD responses transmit no body bytes while
///    preserving representation headers appropriate to the equivalent GET.
///    The application stream is dropped without polling, releasing producer
///    resources promptly. A known equivalent-GET length is preserved as
///    `Content-Length`; an unknown length omits `Content-Length` (never
///    invents `0`).
/// 2. **Body-forbidden statuses**: 1xx, 204, 205, and 304 responses transmit
///    no payload body. Any provided body (including streams) is dropped
///    without polling.
/// 3. **Hop-by-hop header removal**: `Transfer-Encoding` (and all hop-by-hop
///    headers) is stripped — it is runtime-owned.
/// 4. **Content-Length computation**: `Content-Length` is set from the
///    representation length when known (`Known`), omitted when unknown
///    (`Unknown`). Unknown never becomes `Content-Length: 0`.
/// 5. **Conflicting framing rejection**: service-provided framing is removed
///    centrally; the runtime is the only framing authority.
///
/// Normalization is idempotent: a second call is a no-op. Mutating via
/// `head_mut`/`take_body` clears the flag.
///
/// Tests must prove a stream with side effects is not polled for
/// HEAD/body-forbidden responses.
///
/// # Errors
///
/// Returns an error if the response body was already consumed.
pub fn normalize_response(
    mut response: Response,
    request: &NormalizeRequest,
) -> Result<Response, ResponseConstructionError> {
    if response.normalized {
        return Ok(response);
    }
    let status = response.status();

    // Representation length before suppression (equivalent-GET length for
    // HEAD). For streams this is Known or Unknown; dropping below never polls.
    let pre_length: BodyLength = response
        .body
        .as_ref()
        .map(|b| b.body_length())
        .unwrap_or(BodyLength::Known(0));

    // Rule 1: HEAD suppression — drop without polling, preserve length.
    // A known representation length is retained via `EmptyWithLength` so
    // downstream consumers (metrics, adapter boundaries) still observe the
    // equivalent-GET length; unknown lengths stay `Empty` (no invented 0).
    if request.is_head {
        response.body = Some(match &pre_length {
            BodyLength::Known(len) if status.permits_payload_body() => {
                ResponseBody::EmptyWithLength(*len)
            }
            _ => ResponseBody::Empty,
        });
    }

    // Rule 2: Body-forbidden statuses — drop without polling.
    // All body-forbidden statuses except 304 have length zeroed so the
    // invariant `!permits_payload_body && status != 304 => Known(0)` holds.
    // For 304 the pre-suppression length (Known or Unknown) is retained and
    // validated in `normalize_metadata`; for 1xx/204/205 it is forced to
    // Known(0) so future changes to `permits_payload_body` cannot emit stale
    // framing. Dropping a suppressed stream releases producer resources.
    let mut body_len = pre_length;
    if !status.permits_payload_body() {
        response.body = Some(ResponseBody::Empty);
        if status != StatusCode::NOT_MODIFIED {
            body_len = BodyLength::Known(0);
        }
    }

    // Apply shared metadata normalization.
    // `head` is private to this module so direct field access is used here;
    // external callers must go through `head_mut` (which clears `normalized`).
    normalize_metadata(status, response.head.headers_mut(), body_len)?;

    response.normalized = true;
    Ok(response)
}

/// Normalize response metadata without consuming a response body.
///
/// This is the shared normalization entry point for both in-memory,
/// file-backed, and streaming response producers. `normalize_metadata` itself
/// is HEAD-agnostic: callers MUST pass the would-have-been-sent representation
/// length (the pre-suppression length for HEAD, i.e. the equivalent GET
/// length) as `body_length`. The function then applies:
///
/// 1. Strip runtime-owned framing (all hop-by-hop headers, including
///    `Transfer-Encoding`). Service-provided `Transfer-Encoding` remains
///    forbidden/stripped as runtime-owned.
/// 2. Payload-permitting statuses (including HEAD): set `Content-Length` to
///    the known length, retaining the representation length even for
///    zero-length bodies. When the length is `Unknown` (streaming), omit
///    `Content-Length` and let HTTP/1 select chunked framing — never invent
///    `Content-Length: 0`. HEAD callers pass the pre-suppression length so
///    the header is correct; unknown HEAD lengths omit the header.
/// 3. Body-forbidden statuses (1xx, 204, 205, 304): suppress `Content-Length`,
///    except that 304 may retain a matching representation length. A
///    caller-supplied `Content-Length` on 205 is rejected because RFC 9110
///    forbids it entirely.
/// 4. Preserve all other headers (including duplicates).
///
/// # Response architecture
///
/// All response producers must converge on `normalize_metadata()` for
/// response metadata and framing. The allowed sequences are:
///
/// ```text
/// // For in-memory bodies:
/// producer -> Response -> normalize_response() -> to_hyper_response()
///
/// // For file-backed bodies:
/// producer -> normalize_metadata(headers, body_len) -> streaming transport
///
/// // For streaming bodies:
/// producer -> Response(Stream) -> normalize_response() -> to_hyper_response()
/// ```
///
/// `normalize_metadata()` enforces:
/// - Transfer-Encoding is always stripped (runtime-owned)
/// - Content-Length is set from actual body length for known payload-permitting
///   responses (including HEAD with known length, including zero)
/// - Content-Length is omitted for unknown-length payload-permitting responses
/// - Content-Length is suppressed for body-forbidden (1xx/204/205/304)
///   responses, except for a matching 304 representation length
///
/// Callers MUST supply the would-have-been-sent representation length, which
/// is computed before suppressing a HEAD body. Passing a suppressed body's
/// length emits the wrong `Content-Length` for HEAD. Unknown lengths must be
/// passed as `BodyLength::Unknown`, never as `0`.
pub fn normalize_metadata(
    status: StatusCode,
    headers: &mut HeaderBlock,
    body_length: impl Into<BodyLength>,
) -> Result<(), ResponseConstructionError> {
    let body_length: BodyLength = body_length.into();
    // Rule 1: Strip all hop-by-hop headers.
    super::headers::strip_hop_by_hop(headers);

    if status == StatusCode::RESET_CONTENT && headers.contains("content-length") {
        return Err(ResponseConstructionError::ForbiddenFramingHeader(
            "content-length".to_owned(),
        ));
    }

    // A 304 may retain the selected representation's length, but only when the
    // supplied value is unique, valid, and matches the planned representation.
    // Unknown lengths never retain: no Content-Length is invented. Non-UTF-8
    // values cannot be valid decimal lengths, so they yield no retention.
    let not_modified_length = if status == StatusCode::NOT_MODIFIED {
        match body_length {
            BodyLength::Known(known) => headers
                .get_unique("content-length")
                .map_err(|_| {
                    ResponseConstructionError::ForbiddenFramingHeader("content-length".to_owned())
                })?
                .and_then(|value| value.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .filter(|length| *length == known),
            BodyLength::Unknown => None,
        }
    } else {
        None
    };

    // Rule 2-4: Content-Length handling.
    super::headers::remove_header(headers, "content-length");

    if status.permits_payload_body() {
        match body_length {
            BodyLength::Known(length) => {
                headers
                    .push_str("content-length", length.to_string())
                    .map_err(ResponseConstructionError::from)?;
            }
            BodyLength::Unknown => {
                // Omit: HTTP/1 transport selects chunked framing.
            }
        }
    } else if let Some(length) = not_modified_length {
        headers
            .push_str("content-length", length.to_string())
            .map_err(ResponseConstructionError::from)?;
    }

    Ok(())
}

/// Build a transport-neutral generic runtime error response.
///
/// The selected status is authoritative. A standard reason phrase produces a
/// fixed `"<status> <reason>\n"` representation; an unassigned status keeps
/// its status and emits no body rather than claiming a different error. This
/// is the sole runtime-error representation table for H1, H2, and H3.
#[allow(dead_code)]
pub(crate) fn runtime_error_with_policy(
    status: StatusCode,
    is_head: bool,
    policy: crate::policy::ErrorRepresentationPolicy,
) -> Response {
    let body = status
        .canonical_reason()
        .map(|reason| format!("{} {reason}\n", status.as_u16()))
        .unwrap_or_default();
    let mut builder = Response::builder().status(status);
    if policy == crate::policy::ErrorRepresentationPolicy::Minimal {
        builder = builder
            .header("content-type", "text/plain; charset=utf-8")
            .expect("canonical runtime error content type is valid");
    }
    if status == StatusCode::METHOD_NOT_ALLOWED {
        builder = builder
            .header("allow", "GET, HEAD")
            .expect("canonical runtime error Allow value is valid");
    }
    let body = if policy == crate::policy::ErrorRepresentationPolicy::Empty
        || is_head
        || !status.permits_payload_body()
        || body.is_empty()
    {
        ResponseBody::Empty
    } else {
        ResponseBody::Bytes(body.into_bytes())
    };
    builder
        .body(body)
        .expect("canonical runtime error response is valid")
}

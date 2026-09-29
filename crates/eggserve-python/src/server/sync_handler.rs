//! Python sync callback handler (Plan 206 Track B).
//!
//! Owns `PythonCallbackService` and the single Python-response-to-canonical
//! conversion (`convert_python_response_to_canonical`,
//! `extract_python_response_body`). Shared canonical/Python helpers live
//! here; the Python-side `AsyncServer` shim reuses the same bridge via
//! `asyncio.to_thread` with no duplicated Rust conversion logic.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;

use pyo3::prelude::*;
use tokio::sync::mpsc;
use tokio::sync::Semaphore;

use bytes::Bytes;
use eggserve_primitives::body::BodySource;
use eggserve_primitives::canonical::{
    normalize_response, NormalizeRequest, Response as CanonicalResponse, ResponseBody,
    ResponseStream, ResponseStreamError, StatusCode as CanonicalStatusCode,
};
use eggserve_primitives::header_block::{HeaderName, HeaderValue};
use eggserve_primitives::request_body::RequestBody;
use eggserve_primitives::request_body_policy::RequestBodyPolicy;
use eggserve_primitives::request_context::RequestContext;
use eggserve_primitives::request_head::RequestHead;
// Plan 221: static/path/filesystem authority lives once in `eggserve-static`
// (Plan 219); the compatibility `eggserve_core::primitives` facade re-exports
// it. The bridge names the leaf directly. `StaticPolicy` stays
// primitives-owned.
use eggserve_server::service::{Service, ServiceError};

use super::body_bridge::{PyRequestBody, PythonReceiverStream, PYTHON_STREAM_CHANNEL_BOUND, spawn_python_stream_producer};
use super::request_bridge::PyRequest;
use super::response_bridge::{PyResponse, PyResponseBody};

pub(super) struct PythonCallbackService {
    pub(super) handler: Arc<std::sync::Mutex<Option<Py<PyAny>>>>,
    pub(super) callback_semaphore: Arc<Semaphore>,
    pub(super) body_policy: RequestBodyPolicy,
}

impl PythonCallbackService {
    fn call_python_callback(
        handler: &Arc<std::sync::Mutex<Option<Py<PyAny>>>>,
        py_request: PyRequest,
    ) -> Result<CanonicalResponse, ServiceError> {
        Python::attach(|py| {
            let handler_gil = handler
                .lock()
                .map_err(|_| ServiceError::internal("handler lock poisoned"))?;
            let handler_py = handler_gil
                .as_ref()
                .ok_or_else(|| ServiceError::internal("handler already consumed"))?
                .clone_ref(py);
            drop(handler_gil);

            let is_head = py_request.method == "HEAD";
            let py_req_obj = py_request
                .into_pyobject(py)
                .map_err(|e| ServiceError::internal(format!("failed to create request: {e}")))?;

            let result = handler_py.bind(py).call1((py_req_obj,)).map_err(|err| {
                // Log the exception type only; exception text may carry
                // untrusted request data and must not reach logs.
                // NOTE (B85): intentional privacy — all exceptions become a
                // generic 500 with only the type name logged. An opt-in debug
                // mode preserving the message/chain needs a new API (out of
                // scope here).
                let type_name = err
                    .value(py)
                    .get_type()
                    .name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| "<unknown>".to_string());
                eggserve_server::ops::Logger::global().emit(eggserve_server::ops::Event::new(
                    eggserve_server::ops::Severity::Error,
                    eggserve_server::ops::EventKind::ServiceError,
                    format!("Python handler raised an exception ({type_name})"),
                ));
                ServiceError::internal("handler raised an exception")
            })?;

            if result
                .hasattr("__await__")
                .map_err(|_| ServiceError::internal("Python handler response inspection failed"))?
            {
                return Err(ServiceError::internal(
                    "handler returned a coroutine; async handlers are not supported",
                ));
            }

            convert_python_response_to_canonical(py, &result, is_head)
        })
    }

    fn build_py_request(
        head: RequestHead,
        body: RequestBody,
        body_policy: RequestBodyPolicy,
        context: RequestContext,
        tunnel: Option<eggserve_server::tunnel::TunnelCapability>,
    ) -> PyRequest {
        use eggserve_primitives::connection_info::Scheme;

        let connection = context.connection().clone();
        let method_str = head.method().as_str().to_string();
        let target = head.target().clone();
        let http_version = head.version().to_string();

        // Non-socket transports expose no fabricated addresses: map absent
        // endpoints to None rather than placeholder values.
        let remote_addr = connection.remote_addr.map(|a| a.to_string());
        let local_addr = connection.local_addr.map(|a| a.to_string());
        let remote_address = connection
            .remote_addr
            .map(|a| (a.ip().to_string(), a.port()));
        let local_address = connection
            .local_addr
            .map(|a| (a.ip().to_string(), a.port()));
        let scheme = Some(match connection.scheme {
            Scheme::Http => "http".to_string(),
            Scheme::Https => "https".to_string(),
        });

        let (py_body, has_body) = if body_policy.is_reject() {
            (None, false)
        } else {
            // Expose the body only when there is actual content to read.
            // Empty bodies (Content-Length: 0 or no Content-Length with no
            // Transfer-Encoding) are treated as bodyless for the Python
            // handler, regardless of method.
            let has_content = body.declared_length().is_some_and(|len| len > 0)
                || head.headers().contains("transfer-encoding")
                || body.bytes_received() > 0;
            if has_content {
                let declared_length = body.declared_length();
                let py_body = PyRequestBody {
                    inner: Arc::new(std::sync::Mutex::new(Some(body))),
                    handle: tokio::runtime::Handle::current(),
                    declared_length,
                    final_bytes_received: Arc::new(AtomicU64::new(0)),
                    final_complete: Arc::new(AtomicBool::new(false)),
                    incremental: Arc::new(AtomicBool::new(false)),
                };
                (Some(py_body), true)
            } else {
                (None, false)
            }
        };

        let authority = head.authority().map(|a| a.as_str().to_owned());
        let tls_protocol_version = connection
            .tls
            .as_ref()
            .and_then(|t| t.protocol_version.clone());
        let tls_server_name = connection.tls.as_ref().and_then(|t| t.server_name.clone());
        let tls_alpn = connection.tls.as_ref().and_then(|t| t.alpn.clone());
        let client_authenticated = connection
            .tls
            .as_ref()
            .is_some_and(|t| t.client_authenticated);
        let peer_certificates_present = connection
            .tls
            .as_ref()
            .is_some_and(|t| t.peer_certificates_present);
        let proxy_source = connection.proxy_source.map(|a| a.to_string());
        let proxy_destination = connection.proxy_destination.map(|a| a.to_string());
        // Clone lifecycle/interim handles (Arc-backed, cheap).
        let lifecycle = Some(context.lifecycle_clone());
        let interim = context.interim().cloned();
        // Plan 217: single service contract — capability arrives via
        // `Service::call_with_tunnel`, intent stays on the context for
        // routing (inspectable via the capability's `request()` when present).
        // Store the live capability (if any) in a Python-owned
        // one-shot slot; `take_tunnel()` takes from here. This preserves
        // one-shot semantics and keeps `PyRequest` Sync.
        let tunnel_slot: Option<
            Arc<std::sync::Mutex<Option<eggserve_server::tunnel::TunnelCapability>>>,
        > = tunnel.map(|taken| Arc::new(std::sync::Mutex::new(Some(taken))));
        let handle = tokio::runtime::Handle::try_current().ok();

        PyRequest {
            method: method_str,
            header_block: head.headers().clone(),
            headers: std::sync::OnceLock::new(),
            target,
            remote_addr,
            remote_address,
            local_addr,
            local_address,
            scheme,
            http_version,
            body: if has_body { py_body } else { None },
            effective_addr: connection
                .effective_client_addr()
                .map(|addr| addr.to_string()),
            effective_address: connection
                .effective_client_addr()
                .map(|addr| (addr.ip().to_string(), addr.port())),
            effective_scheme: Some(
                connection.effective_scheme_value().as_str().to_owned(),
            ),
            effective_authority: connection
                .effective_authority_value()
                .map(|authority| authority.as_str().to_owned()),
            proxy_provenance: connection
                .proxy_provenance
                .map(|kind| kind.as_str().to_owned()),
            forwarded_provenance: connection
                .forwarded_provenance
                .map(|kind| kind.as_str().to_owned()),
            authority,
            tls_protocol_version,
            tls_server_name,
            tls_alpn,
            client_authenticated,
            peer_certificates_present,
            proxy_source,
            proxy_destination,
            handle,
            lifecycle,
            interim,
            tunnel_slot,
            tunnel_handshake: Arc::new(std::sync::Mutex::new(None)),
        }
    }
}

pub(super) fn convert_python_response_to_canonical<'py>(
    _py: Python<'py>,
    obj: &Bound<'py, PyAny>,
    is_head: bool,
) -> Result<CanonicalResponse, ServiceError> {
    let status: u16 = obj
        .getattr("status")
        .map_err(|_| ServiceError::internal("Python handler response status is missing"))?
        .extract()
        .map_err(|_| ServiceError::internal("Python handler response status is invalid"))?;
    let code = CanonicalStatusCode::new(status)
        .map_err(|_| ServiceError::internal("Python handler response status is outside 100-599"))?;

    let mut headers: Vec<(String, String)> = obj
        .getattr("headers")
        .map_err(|_| ServiceError::internal("Python handler response headers are missing"))?
        .extract()
        .or_else(|_| {
            // Native Response exposes a dict; structural responses may
            // provide an ordered list of header pairs. Extract as an ordered
            // vector first so dict insertion order survives (a `HashMap`
            // fallback would randomize wire order); only then try the
            // unordered map form.
            obj.getattr("headers")
                .and_then(|v| v.extract::<Vec<(String, String)>>())
                .or_else(|_| {
                    obj.getattr("headers").and_then(|v| {
                        v.extract::<HashMap<String, String>>()
                            .map(|map| map.into_iter().collect())
                    })
                })
        })
        .map_err(|_| ServiceError::internal("Python handler response headers are invalid"))?;

    if let Ok(py_resp) = obj.extract::<pyo3::Bound<'py, PyResponse>>() {
        headers.extend(py_resp.borrow().extra_headers.iter().cloned());
    }

    // Validate every header into temporary canonical values before constructing
    // a response. This keeps a later body or framing failure from exposing a
    // partially validated response.
    let mut validated_headers = Vec::with_capacity(headers.len());
    for (name, value) in headers {
        if eggserve_primitives::canonical::is_hop_by_hop_header(&name) {
            return Err(ServiceError::internal(
                "Python handler response header validation failed",
            ));
        }
        // Empty field-values are legal per RFC 9110 and the canonical
        // `HeaderValue` (post-OWS); only transport-refused bytes fail below.
        let n = HeaderName::new(name.as_str()).map_err(|_| {
            ServiceError::internal("Python handler response header validation failed")
        })?;
        let v = HeaderValue::new(value.as_str()).map_err(|_| {
            ServiceError::internal("Python handler response header validation failed")
        })?;
        let content_length = if name.eq_ignore_ascii_case("content-length") {
            // Strict RFC 9110 `Content-Length = 1*DIGIT`: strip only SP/HTAB
            // OWS (not Unicode whitespace) and reject empty, `+`-prefixed,
            // or non-digit values. `u64::from_str` would accept `"+5"`.
            let trimmed = value.trim_matches([' ', '\t']);
            let valid = !trimmed.is_empty()
                && trimmed.bytes().all(|b| b.is_ascii_digit());
            if !valid {
                return Err(ServiceError::internal(
                    "Python handler response length validation failed",
                ));
            }
            Some(trimmed.parse::<u64>().map_err(|_| {
                ServiceError::internal("Python handler response length validation failed")
            })?)
        } else {
            None
        };
        validated_headers.push((n, v, content_length));
    }

    let declared_lengths: Vec<u64> = validated_headers
        .iter()
        .filter_map(|(_, _, declared)| *declared)
        .collect();
    // Duplicate `Content-Length` fields must agree on the wire via a single
    // value; two identical fields are still a duplicate framing anomaly.
    if declared_lengths.len() > 1 {
        return Err(ServiceError::internal(
            "Python handler response length validation failed",
        ));
    }
    let representation_length = declared_lengths.into_iter().next();
    let body = extract_python_response_body(obj, code, is_head, representation_length)?;

    // Framing-authoritative check: `body_length()` (Unknown for unknown
    // streams), never `len()` (which returns 0 for unknown streams and would
    // let `Content-Length: 0` slip through on an unknown-length stream).
    // Unknown-length bodies must be chunked, never `Content-Length`.
    match body.body_length() {
        eggserve_primitives::canonical::BodyLength::Known(known) => {
            if let Some(declared) = representation_length {
                if declared != known {
                    return Err(ServiceError::internal(
                        "Python handler response length validation failed",
                    ));
                }
            }
        }
        eggserve_primitives::canonical::BodyLength::Unknown => {
            if representation_length.is_some() {
                return Err(ServiceError::internal(
                    "Python handler response length validation failed",
                ));
            }
        }
    }

    let mut response = CanonicalResponse::builder()
        .status(code)
        .body(body)
        .map_err(|_| ServiceError::internal("Python handler response construction failed"))?;

    for (n, v, _) in validated_headers {
        response.head_mut().headers_mut().push(n, v);
    }

    let norm_req = NormalizeRequest::new(is_head);
    normalize_response(response, &norm_req)
        .map_err(|_| ServiceError::internal("Python handler response normalization failed"))
}

pub(super) fn extract_python_response_body<'py>(
    obj: &Bound<'py, PyAny>,
    status: CanonicalStatusCode,
    is_head: bool,
    representation_length: Option<u64>,
) -> Result<ResponseBody, ServiceError> {
    if let Ok(py_resp) = obj.extract::<pyo3::Bound<'py, PyResponse>>() {
        let response = py_resp.borrow();
        let mut body = response.body.lock().map_err(|_| {
            ServiceError::internal("Python handler response body conversion failed")
        })?;
        return match std::mem::replace(&mut *body, PyResponseBody::Consumed) {
            PyResponseBody::Consumed => Err(ServiceError::internal(
                "Python handler response body conversion failed",
            )),
            PyResponseBody::Empty => {
                if is_head {
                    if let Some(length) = representation_length {
                        Ok(ResponseBody::EmptyWithLength(length))
                    } else {
                        Ok(ResponseBody::Empty)
                    }
                } else {
                    Ok(ResponseBody::Empty)
                }
            }
            PyResponseBody::Bytes(data) => {
                if is_head {
                    if let Some(length) = representation_length {
                        // Validate against the discarded representation: the
                        // later `EmptyWithLength` length check would pass
                        // vacuously, so a mismatch must fail here.
                        if data.len() as u64 != length {
                            return Err(ServiceError::internal(
                                "Python handler response length validation failed",
                            ));
                        }
                        Ok(ResponseBody::EmptyWithLength(length))
                    } else {
                        Ok(ResponseBody::Bytes(data))
                    }
                } else {
                    Ok(ResponseBody::Bytes(data))
                }
            }
            PyResponseBody::BodySource(source) => match source {
                BodySource::Empty => {
                    if is_head {
                        if let Some(length) = representation_length {
                            Ok(ResponseBody::EmptyWithLength(length))
                        } else {
                            Ok(ResponseBody::Empty)
                        }
                    } else {
                        Ok(ResponseBody::Empty)
                    }
                }
                BodySource::Bytes(data) => {
                    if is_head {
                        if let Some(length) = representation_length {
                            // Same vacuous-check guard as `Bytes` above.
                            if data.len() as u64 != length {
                                return Err(ServiceError::internal(
                                    "Python handler response length validation failed",
                                ));
                            }
                            Ok(ResponseBody::EmptyWithLength(length))
                        } else {
                            Ok(ResponseBody::Bytes(data))
                        }
                    } else {
                        Ok(ResponseBody::Bytes(data))
                    }
                }
                file @ BodySource::FileFull { .. } | file @ BodySource::FileRange { .. } => {
                    Ok(ResponseBody::File(file))
                }
            },
            PyResponseBody::Stream {
                iterable,
                content_length,
            } => {
                // HEAD and body-forbidden statuses must not advance the
                // iterator: drop the iterable (releasing Python references
                // promptly) and retain only framing-relevant length.
                // `normalize_response` drops streams without polling, but
                // spawning the producer would still pull one item before
                // observing the drop, so suppress here.
                let suppress_forbidden = !status.permits_payload_body();
                if is_head || suppress_forbidden {
                    drop(iterable);
                    // 304 may retain a matching representation length;
                    // 1xx/204/205 are forced to zero by normalization.
                    // HEAD with known length preserves it; HEAD unknown
                    // omits Content-Length via an empty unknown stream
                    // (Empty would invent `Content-Length: 0`).
                    if status == CanonicalStatusCode::NOT_MODIFIED {
                        if let Some(length) = content_length.or(representation_length) {
                            Ok(ResponseBody::EmptyWithLength(length))
                        } else {
                            let empty =
                                ResponseStream::new(futures_util::stream::empty::<
                                    Result<Bytes, ResponseStreamError>,
                                >());
                            Ok(ResponseBody::Stream(empty))
                        }
                    } else if is_head {
                        if let Some(length) = content_length.or(representation_length) {
                            // Known HEAD length: preserve for framing. When
                            // only the header supplied the length for an
                            // unknown stream, the header is authoritative
                            // (validation below already compared it against
                            // the would-be body only for non-suppressed
                            // paths; here the iterator never ran so accept
                            // the declared header length).
                            Ok(ResponseBody::EmptyWithLength(length))
                        } else {
                            // Unknown HEAD length: omit Content-Length.
                            let empty =
                                ResponseStream::new(futures_util::stream::empty::<
                                    Result<Bytes, ResponseStreamError>,
                                >());
                            Ok(ResponseBody::Stream(empty))
                        }
                    } else {
                        Ok(ResponseBody::Empty)
                    }
                } else {
                    let (sender, receiver) =
                        mpsc::channel::<Result<Bytes, ResponseStreamError>>(
                            PYTHON_STREAM_CHANNEL_BOUND,
                        );
                    spawn_python_stream_producer(iterable, sender);
                    let adapter = PythonReceiverStream {
                        rx: std::sync::Mutex::new(receiver),
                    };
                    let stream = match content_length {
                        Some(len) => ResponseStream::with_known_length(adapter, len),
                        None => ResponseStream::new(adapter),
                    };
                    Ok(ResponseBody::Stream(stream))
                }
            }
            PyResponseBody::StreamWithTrailers {
                iterable,
                content_length,
                trailers,
            } => {
                // Same suppression as `Stream`, plus trailer suppression for
                // HEAD/body-forbidden (never emit trailers without polling).
                let suppress_forbidden = !status.permits_payload_body();
                if is_head || suppress_forbidden {
                    drop(iterable);
                    drop(trailers);
                    if status == CanonicalStatusCode::NOT_MODIFIED {
                        if let Some(length) = content_length.or(representation_length) {
                            Ok(ResponseBody::EmptyWithLength(length))
                        } else {
                            let empty =
                                ResponseStream::new(futures_util::stream::empty::<
                                    Result<Bytes, ResponseStreamError>,
                                >());
                            Ok(ResponseBody::Stream(empty))
                        }
                    } else if is_head {
                        if let Some(length) = content_length.or(representation_length) {
                            Ok(ResponseBody::EmptyWithLength(length))
                        } else {
                            let empty =
                                ResponseStream::new(futures_util::stream::empty::<
                                    Result<Bytes, ResponseStreamError>,
                                >());
                            Ok(ResponseBody::Stream(empty))
                        }
                    } else {
                        Ok(ResponseBody::Empty)
                    }
                } else {
                    // Build the canonical trailer block (validated at
                    // construction, re-validated here defensively; failures
                    // are internal (500) with no detail leak).
                    use eggserve_primitives::header_block::HeaderBlock;
                    use eggserve_primitives::trailers::{TrailerLimits, Trailers};
                    let mut block = HeaderBlock::new();
                    for (name, value) in &trailers {
                        block.push_str(name, value).map_err(|_| {
                            ServiceError::internal(
                                "Python handler response trailer validation failed",
                            )
                        })?;
                    }
                    let trailers = Trailers::with_limits(block, &TrailerLimits::default())
                        .map_err(|_| {
                            ServiceError::internal(
                                "Python handler response trailer validation failed",
                            )
                        })?;
                    let (sender, receiver) =
                        mpsc::channel::<Result<Bytes, ResponseStreamError>>(
                            PYTHON_STREAM_CHANNEL_BOUND,
                        );
                    spawn_python_stream_producer(iterable, sender);
                    let adapter = PythonReceiverStream {
                        rx: std::sync::Mutex::new(receiver),
                    };
                    let stream = match content_length {
                        Some(len) => ResponseStream::with_known_length_and_trailers(
                            adapter,
                            len,
                            futures_util::future::ready(Ok(Some(trailers))),
                        ),
                        None => ResponseStream::with_trailers(
                            adapter,
                            futures_util::future::ready(Ok(Some(trailers))),
                        ),
                    };
                    Ok(ResponseBody::Stream(stream))
                }
            }
        };
    }

    let body = obj
        .getattr("body")
        .map_err(|_| ServiceError::internal("Python handler response body is missing"))?;
    if let Ok(data) = body.extract::<Vec<u8>>() {
        if is_head {
            if let Some(length) = representation_length {
                // Structural (stdlib-facade) HEAD responses trust the declared
                // length: `BaseHTTPRequestHandler.send_error` (and HEAD
                // handlers generally) withhold the body (`wfile` empty) while
                // declaring the GET-equivalent `Content-Length`. Validating
                // `data` here would break that stdlib pattern; the native
                // `PyResponse` paths above validate instead.
                return Ok(ResponseBody::EmptyWithLength(length));
            }
        }
        return Ok(ResponseBody::Bytes(data));
    }

    let kind: String = body
        .getattr("kind")
        .map_err(|_| ServiceError::internal("Python handler response body is unsupported"))?
        .extract()
        .map_err(|_| ServiceError::internal("Python handler response body kind is invalid"))?;
    match kind.as_str() {
        "empty" => Ok(ResponseBody::Empty),
        "bytes" => {
            let data = body
                .call_method0("read_all")
                .map_err(|_| {
                    ServiceError::internal("Python handler response body conversion failed")
                })?
                .extract::<Vec<u8>>()
                .map_err(|_| {
                    ServiceError::internal("Python handler response body conversion failed")
                })?;
            if is_head {
                if let Some(length) = representation_length {
                    // Same stdlib-facade trust as the `Vec<u8>` branch above:
                    // the declared length is authoritative for HEAD.
                    Ok(ResponseBody::EmptyWithLength(length))
                } else {
                    Ok(ResponseBody::Bytes(data))
                }
            } else {
                Ok(ResponseBody::Bytes(data))
            }
        }
        _ => Err(ServiceError::internal(
            "Python handler response body kind is unsupported",
        )),
    }
}

impl Service for PythonCallbackService {
    fn request_body_policy(
        &self,
        _head: &eggserve_primitives::request_head::RequestHead,
    ) -> RequestBodyPolicy {
        self.body_policy
    }

    fn call(
        &self,
        request: eggserve_primitives::request::Request,
    ) -> Pin<
        Box<dyn std::future::Future<Output = Result<CanonicalResponse, ServiceError>> + Send + '_>,
    > {
        self.call_with_tunnel(request, None)
    }

    fn call_with_tunnel(
        &self,
        request: eggserve_primitives::request::Request,
        tunnel: Option<eggserve_server::tunnel::TunnelCapability>,
    ) -> Pin<
        Box<dyn std::future::Future<Output = Result<CanonicalResponse, ServiceError>> + Send + '_>,
    > {
        let handler = self.handler.clone();
        let callback_semaphore = self.callback_semaphore.clone();
        let body_policy = self.body_policy;

        Box::pin(async move {
            // Fail fast under pile-up (parity with the Rust admission
            // kernel): never queue unbounded `Request+Body+Context` behind
            // `acquire().await`; shed with 503 instead.
            // NOTE (B55): this permit (plus the outer service permit, held
            // across `Service::call`) is held for the whole Python callback,
            // including blocking `RequestBody.read()` network waits on the
            // callback thread. Slow uploads therefore contend with handlers
            // under `max_python_callbacks`; size that bound for upload
            // concurrency (separate upload/download budgets need a new
            // admission API, out of scope here).
            let callback_permit = match callback_semaphore.try_acquire_owned() {
                Ok(permit) => permit,
                Err(_) => {
                    return Err(ServiceError::rejected(
                        503,
                        "service unavailable: callback saturated",
                    ));
                }
            };

            // Plan 204: use the full context so async-capable handlers observe
            // lifecycle/interim/tunnel ownership. Sync behavior for existing
            // getters is unchanged (additive fields only).
            // Plan 217: capability arrives via the service parameter, not via
            // a context slot.
            let (head, body, context) = request.into_parts_with_context();
            let py_request = Self::build_py_request(head, body, body_policy, context, tunnel);
            // Share the handshake slot so `accept_tunnel` (which runs on the
            // blocking handler thread) can publish the runtime-owned
            // handshake `Response` (with `TunnelAcceptance`) for use here.
            let handshake_slot = py_request.tunnel_handshake.clone();

            let outcome = tokio::task::spawn_blocking(move || {
                let _callback_permit = callback_permit;
                Self::call_python_callback(&handler, py_request)
            })
            .await
                .map_err(|e| ServiceError::internal(format!("callback task failed: {e}")))?;

            // Plan 204 tunnel handoff: if the handler accepted a tunnel, the
            // stored handshake (with transport upgrade + duplex handler)
            // wins over the converted `Response`. This preserves the
            // runtime-owned acceptance across the Python boundary without
            // exposing raw sockets. Denial stays ordinary HTTP.
            if let Ok(mut slot) = handshake_slot.lock() {
                if let Some(handshake) = slot.take() {
                    return Ok(handshake);
                }
            }
            outcome
        })
    }
}

// ---------------------------------------------------------------------------
// Python Server — delegates to Rust runtime
// ---------------------------------------------------------------------------

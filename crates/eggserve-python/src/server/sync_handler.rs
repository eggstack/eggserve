//! Python sync callback handler (Plan 206 Track B).
//!
//! Owns `PythonCallbackService` and the single Python-response-to-canonical
//! conversion (`convert_python_response_to_canonical`,
//! `extract_python_response_body`). Shared canonical/Python helpers live
//! here; the Python-side `AsyncServer` shim reuses the same bridge via
//! `asyncio.to_thread` with no duplicated Rust conversion logic.

#![allow(unused_imports)]
use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyIterator};
use tokio::sync::mpsc;
use tokio::sync::Semaphore;

use bytes::Bytes;
use eggserve_core::policy;
use eggserve_core::primitives::body::BodySource;
use eggserve_core::primitives::canonical::{
    normalize_response, NormalizeRequest, Response as CanonicalResponse, ResponseBody,
    ResponseStream, ResponseStreamError, StatusCode as CanonicalStatusCode,
};
use eggserve_core::primitives::header_block::{HeaderName, HeaderValue};
use eggserve_core::primitives::http::ReadOnlyMethod;
use eggserve_core::primitives::request_body::RequestBody;
use eggserve_core::primitives::request_body_error::RequestBodyError as RustBodyError;
use eggserve_core::primitives::request_body_policy::RequestBodyPolicy;
use eggserve_core::primitives::request_context::RequestContext;
use eggserve_core::primitives::request_head::RequestHead;
use eggserve_core::primitives::{
    resolve_and_plan, ConfinedPath, PathDotfilePolicy, PathPolicy, PathRejection,
    ResolveAndPlanError, SecureRoot, StaticPolicy,
};
use eggserve_core::server::config::RuntimeConfig;
use eggserve_core::server::errors::ShutdownResult;
use eggserve_core::server::lifecycle::LifecycleState;
use eggserve_core::server::service::{Service, ServiceError};
use eggserve_core::server::{Server, ServerHandle};

use super::*;
#[allow(unused_imports)]
use super::body_bridge::{PyRequestBody, PythonReceiverStream, PYTHON_STREAM_CHANNEL_BOUND, spawn_python_stream_producer};
#[allow(unused_imports)]
use super::errors::{RawBodyError, raw_body_error_to_pyerr};
#[allow(unused_imports)]
use super::request_bridge::PyRequest;
#[allow(unused_imports)]
use super::response_bridge::{PyResponse, PyResponseBody};
#[allow(unused_imports)]
use super::tunnel_bridge::{PyTunnelCapability, PyTunnelRequest};

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
        Python::with_gil(|py| {
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
                let type_name = err
                    .value(py)
                    .get_type()
                    .name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| "<unknown>".to_string());
                eggserve_core::ops::Logger::global().emit(eggserve_core::ops::Event::new(
                    eggserve_core::ops::Severity::Error,
                    eggserve_core::ops::EventKind::ServiceError,
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
    ) -> PyRequest {
        use eggserve_core::primitives::connection_info::Scheme;

        let connection = context.connection().clone();
        let method_str = head.method().as_str().to_string();
        let target = head.target().path().to_string();
        let query = head.target().query().unwrap_or("").to_string();
        // Text-only facade (Plan 173 Track D): stdlib-shaped `headers` expose
        // `str`. Opaque (non-UTF-8) field values are omitted rather than
        // lossily coerced; Rust canonical primitives remain byte-correct.
        let header_items: Vec<(String, String)> = head
            .headers()
            .iter()
            .filter_map(|f| {
                f.value
                    .to_str()
                    .ok()
                    .map(|v| (f.name.to_string(), v.to_owned()))
            })
            .collect();
        let mut headers = HashMap::new();
        for (name, value) in &header_items {
            headers
                .entry(name.to_ascii_lowercase())
                .or_insert_with(|| value.clone());
        }
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
                };
                (Some(py_body), true)
            } else {
                (None, false)
            }
        };

        // Plan 204 byte-fidelity views (additive; text facade above unchanged).
        let raw_target_bytes = head.target().raw_bytes().to_vec();
        let path_bytes = head.target().path_bytes().to_vec();
        let query_bytes = head.target().query_bytes().map(|q| q.to_vec());
        let header_items_bytes: Vec<(Vec<u8>, Vec<u8>)> = head
            .headers()
            .iter()
            .map(|f| {
                (
                    f.name.as_str().as_bytes().to_vec(),
                    f.value.as_bytes().to_vec(),
                )
            })
            .collect();
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
        // Clone capability handles (Arc-backed, cheap; tunnel slot shared so
        // taking via one clone removes for all — one-shot, no duplication).
        let lifecycle = Some(context.lifecycle_clone());
        let interim = context.interim().cloned();
        // Re-create the shared tunnel slot view: `RequestContext` owns
        // `Arc<Mutex<Option<TunnelCapability>>>` privately; expose one-shot
        // takes via a new slot that mirrors presence. Presence is checked
        // via `tunnel_request()` (metadata clone, no ownership); the actual
        // capability is taken via `context.take_tunnel()` on demand in
        // `take_tunnel()` below (which locks the context's slot). To keep
        // `PyRequest` Sync with a plain Mutex slot, store a fresh slot that
        // is populated lazily? Simpler: store `None` here and resolve
        // presence via a cloned context? `RequestContext` is Clone + Sync
        // (Arc-backed) so store it directly for tunnel takes.
        //
        // To avoid storing the full context (which holds a non-Sync
        // `TunnelCapability` inside its Mutex slot), store only the
        // tunnel-request metadata presence + a shared take handle created
        // here. The runtime context's slot is not directly reachable after
        // `into_parts_with_context` moves it; instead `build_py_request`
        // receives the owned context, so take the capability slot ownership
        // by wrapping the context itself in an Arc<Mutex<Option<...>>>?
        //
        // Pragmatic additive path: store the tunnel-request metadata for
        // routing decisions and resolve the live capability via a shared
        // `Arc<Mutex<Option<TunnelCapability>>>` created from
        // `context.take_tunnel()` eagerly (taking now, holding for Python).
        // If no capability, slot holds None (ordinary HTTP). `take_tunnel()`
        // then takes from this Python-owned slot (one-shot). This preserves
        // one-shot semantics (runtime slot already drained once here) and
        // keeps `PyRequest` Sync (Mutex<Option<TunnelCapability>> is Sync
        // when the capability is Send).
        let tunnel_request = context.tunnel_request();
        let tunnel_slot: Option<
            Arc<std::sync::Mutex<Option<eggserve_core::primitives::tunnel::TunnelCapability>>>,
        > = if tunnel_request.is_some() {
            let taken = context.take_tunnel();
            Some(Arc::new(std::sync::Mutex::new(taken)))
        } else {
            None
        };
        let handle = tokio::runtime::Handle::try_current().ok();

        PyRequest {
            method: method_str,
            path: target,
            query,
            headers,
            header_items,
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
            raw_target_bytes,
            path_bytes,
            query_bytes,
            header_items_bytes,
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
            // provide an ordered list of header pairs.
            obj.getattr("headers")
                .and_then(|v| v.extract::<HashMap<String, String>>())
                .map(|map| map.into_iter().collect())
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
        if eggserve_core::primitives::canonical::is_hop_by_hop_header(&name) {
            return Err(ServiceError::internal(
                "Python handler response header validation failed",
            ));
        }
        if value.trim().is_empty() {
            return Err(ServiceError::internal(
                "Python handler response header validation failed",
            ));
        }
        let n = HeaderName::new(name.as_str()).map_err(|_| {
            ServiceError::internal("Python handler response header validation failed")
        })?;
        let v = HeaderValue::new(value.as_str()).map_err(|_| {
            ServiceError::internal("Python handler response header validation failed")
        })?;
        let content_length = if name.eq_ignore_ascii_case("content-length") {
            Some(value.parse::<u64>().map_err(|_| {
                ServiceError::internal("Python handler response length validation failed")
            })?)
        } else {
            None
        };
        validated_headers.push((n, v, content_length));
    }

    let representation_length = validated_headers
        .iter()
        .find_map(|(_, _, declared)| *declared);
    let body = extract_python_response_body(obj, code, is_head, representation_length)?;

    let body_len = body.len();
    for (_, _, declared) in &validated_headers {
        if let Some(declared) = declared {
            if *declared != body_len {
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
                    use eggserve_core::primitives::header_block::HeaderBlock;
                    use eggserve_core::primitives::trailers::{TrailerLimits, Trailers};
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
        _head: &eggserve_core::primitives::request_head::RequestHead,
    ) -> RequestBodyPolicy {
        self.body_policy
    }

    fn call(
        &self,
        request: eggserve_core::primitives::request::Request,
    ) -> Pin<
        Box<dyn std::future::Future<Output = Result<CanonicalResponse, ServiceError>> + Send + '_>,
    > {
        let handler = self.handler.clone();
        let callback_semaphore = self.callback_semaphore.clone();
        let body_policy = self.body_policy;

        Box::pin(async move {
            let callback_permit = callback_semaphore
                .acquire_owned()
                .await
                .map_err(|_| ServiceError::internal("callback semaphore closed"))?;

            // Plan 204: use the full context so async-capable handlers observe
            // lifecycle/interim/tunnel ownership. Sync behavior for existing
            // getters is unchanged (additive fields only).
            let (head, body, context) = request.into_parts_with_context();
            let py_request = Self::build_py_request(head, body, body_policy, context);
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


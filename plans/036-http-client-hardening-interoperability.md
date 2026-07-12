# Phase 36 — HTTP Client Hardening and Interoperability

## Goal

Harden the experimental low-level HTTP client so its documented contract is accurate, interoperable, and safe under malformed, slow, TLS, IPv4, and IPv6 conditions.

The client remains deliberately narrow: buffered responses, one connection per request, no redirects, cookies, proxies, retries, decompression, pooling, streaming response API, HTTP/2, or HTTP/3.

## Track A — Contract consolidation

Update the release contract and client docs to define:
- supported schemes (`http`, optional `https`);
- URL grammar and rejected constructs;
- one connection per request;
- connect timeout versus request timeout;
- response-body size cap semantics;
- TLS verification defaults;
- `verify_tls=False` risk;
- request body buffering;
- response body buffering;
- duplicate-header preservation;
- error taxonomy.

Mark all client APIs experimental in Rust and Python docs.

## Track B — Local HTTP interoperability harness

Build reusable local test servers that can emit exact HTTP/1.1 responses for:
- fixed `Content-Length`;
- chunked transfer encoding;
- connection-close-delimited bodies;
- empty bodies;
- duplicate headers;
- malformed status line;
- malformed headers;
- premature EOF;
- incorrect `Content-Length`;
- delayed headers;
- delayed body chunks;
- oversized bodies.

No external network dependency is permitted in CI.

## Track C — Timeout and size-limit semantics

Prove:
- connect timeout applies only to connection establishment;
- request timeout covers handshake, send, response headers, and full body collection;
- stalled body collection times out;
- max response bytes are enforced without unbounded allocation;
- zero/invalid values are rejected at construction;
- timeout and size-limit errors map consistently in Rust and Python.

Decide whether body-limit overflow returns a dedicated error before or after reading the violating chunk, and document it.

## Track D — TLS correctness

Using a local TLS server and generated test CA/certificates, cover:
- trusted certificate succeeds;
- untrusted certificate fails;
- hostname mismatch fails;
- expired/not-yet-valid certificate fails where practical;
- `verify_tls=False` succeeds only when explicitly configured;
- SNI uses the hostname;
- IP literal behavior is documented and tested;
- HTTPS fails before TCP connect when `client-tls` is disabled;
- HTTP never enters TLS code paths.

Avoid network-based public CA tests.

## Track E — URL and authority hardening

Test and correct:
- IPv4 and IPv6 literals;
- bracketed IPv6 authorities;
- default and explicit ports;
- empty host/port;
- query with and without slash;
- fragment stripping;
- userinfo rejection;
- unsupported schemes;
- Unicode/IDN rejection or explicit support;
- percent-encoded path preservation;
- controls, whitespace, and CR/LF rejection;
- Host header generation.

Prefer a well-defined narrow parser over permissive accidental behavior.

## Track F — Request construction and headers

Ensure:
- method validation is explicit;
- request-target is origin-form;
- Host is generated correctly;
- duplicate request headers are represented without collapse if supported;
- forbidden framing combinations are rejected;
- user-supplied Host/Content-Length behavior is documented;
- GET/HEAD body policy matches the release contract;
- invalid header names/values fail before network I/O.

## Track G — Response parsing and error mapping

Verify handling of:
- informational responses if Hyper exposes them;
- 204, 304, and HEAD bodies;
- duplicate response headers;
- premature EOF;
- malformed chunked bodies;
- protocol errors;
- body-limit errors;
- TLS errors;
- timeout errors;
- DNS/connect failures.

Python must expose meaningful EggServe exception classes rather than collapsing everything into `ValueError`.

## Track H — Feature matrix

CI must cover:
- default features without client symbols;
- `client` HTTP-only;
- `client-tls` HTTP and HTTPS;
- Python wheel client imports;
- installed-wheel local HTTP smoke test;
- installed-wheel local HTTPS smoke test where the wheel includes TLS support.

## Likely files

- `crates/eggserve-core/src/primitives/client/*`
- client integration tests
- `crates/eggserve-python/src/client.rs`
- Python client tests
- client architecture/docs
- CI workflow

## Acceptance criteria

- HTTPS is never plaintext and verification is on by default.
- Disabled TLS rejects HTTPS before network activity.
- Timeout coverage matches documentation.
- Premature EOF and malformed responses are errors.
- IPv6 authority and Host handling are correct.
- Duplicate-header behavior is explicit.
- Python/Rust errors are coherent.
- All tests use local deterministic servers.
- No higher-level client features are added.

# Plan 223 — Consolidate outbound HTTP CONNECT wire mechanics between eggfetch and eggress

## Purpose

Remove duplicated outbound HTTP/1 CONNECT handshake code between `eggfetch-http-connect` and `eggress-protocol-http` without making eggserve depend on an HTTP client or proxy stack.

This is a cross-repo maintenance plan; eggserve itself should remain unaffected at runtime.

## Repositories

- `eggstack/eggfetch`
- `eggstack/eggress`
- `eggstack/eggserve` only as planning/architecture coordination

## Goals

- One small neutral implementation of caller-owned-stream outbound HTTP CONNECT mechanics.
- Preserve egress-specific inbound CONNECT server parsing/forwarding.
- Preserve eggfetch-specific proxy selection, dialing, timeout/retry, and TLS policy.
- Avoid product-to-product dependency layering.

## Preferred shape

Promote/rename the current `eggfetch-http-connect` concept into a neutral crate such as `eggnet-http-connect` if cross-repo naming matters.

The primitive should own only:
- CONNECT authority formatting,
- request-head encoding,
- optional Proxy-Authorization encoding,
- bounded response-head parsing,
- response status extraction,
- preservation of read-ahead bytes,
- neutral structured errors.

Caller owns:
- socket dialing,
- DNS,
- TLS,
- timeout wrapping,
- retry,
- status policy beyond parse/result classification if desired,
- proxy routing,
- tunnel lifecycle.

## Non-goals

- Do not share egress inbound CONNECT authentication/server parser unless a later review proves identical semantics.
- Do not move ordinary HTTP forwarding into the neutral crate.
- Do not make eggserve depend on the CONNECT crate.
- Do not combine H2/H3 CONNECT with the H1 wire primitive.

## Work

### 1. Compare behavior

Diff current eggfetch and eggress outbound implementations for:
- IPv6 authority rendering,
- empty/invalid host rejection,
- credential control-character rejection,
- header/count/byte limits,
- status-line parsing,
- obs-text handling,
- read-ahead preservation,
- 2xx acceptance policy,
- 407/403/502/504 mapping.

Resolve semantic differences explicitly before deleting either implementation.

### 2. Neutral API

Keep the API stream-generic over caller-owned `AsyncRead + AsyncWrite` where practical.

Do not expose eggfetch or eggress target-address types. Use a neutral target/authority representation or caller-supplied validated authority.

### 3. Eggfetch migration

Keep its current proxy orchestration and use the neutral wire primitive for encode/read.

### 4. Eggress migration

Replace only outbound `http_connect` wire mechanics. Retain:
- inbound `handle_connect`,
- proxy auth policy,
- target abstractions,
- forwarding,
- relay,
- H2 CONNECT pool,
- H3 CONNECT behavior.

### 5. Dependency footprint

The neutral crate should remain extremely small. Avoid pulling `http`, Hyper, URL, rustls, or a framework if the current byte-level implementation does not need them.

## Security invariants

- No credential bytes appear in errors/logs.
- Limits are enforced during read, not after allocation.
- Bytes read beyond the CONNECT head are never discarded.
- Authority encoding cannot permit CR/LF/request-line injection.
- IPv6 authority uses brackets.
- Timeouts remain caller-owned so the primitive cannot silently widen policy.

## Tests

- fragmented response head,
- response head + tunneled bytes in one read,
- oversized head,
- too many headers,
- malformed status,
- obs-text header value,
- IPv4/domain/IPv6 authority,
- credential injection attempts,
- non-2xx statuses,
- cancellation/timeout via caller wrapper,
- parity tests in both products.

## Acceptance criteria

- Eggfetch and eggress no longer maintain separate outbound H1 CONNECT encoders/parsers.
- Neutral crate has no product, TLS, DNS, socket, retry, or routing dependency.
- Eggress inbound CONNECT remains locally owned.
- Eggserve dependency graph is unchanged.

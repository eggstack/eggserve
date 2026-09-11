# Plan 202 — Trusted Proxy Metadata and HAProxy PROXY Protocol

## Status

**IMPLEMENTED / CLOSED.**

Prerequisites: Plan 197 connection/request context (settled); Plan 201 listener architecture (settled, developed in parallel and closed first).

## Closure record

Implemented as specified with a conservative single-trusted-hop scope:

- Track A: `ConnectionInfo` preserves raw peer/local and adds provenance-tagged `proxy_source`/`proxy_destination`/`proxy_provenance` + `effective_client`/`effective_scheme`/`effective_authority`/`forwarded_provenance` with `effective_client_addr()`/`effective_scheme_value()`/`has_trusted_proxy_metadata()`; `ConnectionContext` carries the PROXY layer via `with_proxy_endpoints()` into per-request `ConnectionInfo`.
- Track B: `TrustedProxyConfig` with explicit `IpPrefix` peers/CIDRs (no DNS, no implicit loopback), explicit `trust_unix`, validated ranges; defaults trust nothing.
- Track C: `primitives::proxy` pure bounded parsers (v1 107B strict, v2 16+≤1024B with signature/version/command/family/protocol/length validation, `LOCAL`/`UNKNOWN`/`UNSPEC`/UNIX truthful absence, TLVs ignored bounded) plus `server::proxy::read_proxy_preamble` timeout-protected read with `PrefixedIo` leftover replay; accept order `TCP → PROXY → TLS → HTTP`; disabled interprets bytes normally; malformed/untrusted closes before TLS/HTTP.
- Track D: `Forwarded` (RFC 7239 `for=`/`proto=`/`host=` with quoted values, IPv4/IPv6/ports, `unknown`/obfuscated) and legacy `X-Forwarded-*` single-hop rightmost-wins policy with 4 KiB/16-element budgets, conflict fail-closed, canonical Host/target never rewritten.
- Track E: transport TLS remains direct-`https` source; trusted proxy asserts external `https` only via header policy into `effective_scheme`; `effective_authority` never rewrites the request.
- Track F: `proxy_protocol_accepted`/`rejected` + `forwarded_metadata_accepted`/`rejected` events/counters with sanitized `peer`/`source`/`effective`/`category` (no chains/TLVs).
- Track G: native `RequestContext`/`ConnectionInfo` carry effective fields; Tower via `ConnectionInfoExt` (already includes effective); Python `lowlevel` exposes `trusted_proxies`/`trust_unix_local`/`proxy_protocol`/`forwarded_*` config plus `effective_*`/`*_provenance` getters with `remote_addr` unchanged; sync facade never changes `client_address`.

Qualification: `crates/eggserve-core/tests/trusted_proxy.rs` (36 tests with TLS: peer policy, v1/v2 parsing incl. fragmented/timeout/oversized/family matrix/LOCAL/UNKNOWN, header hop/conflict/bounds/IPv4/IPv6/ports, spoofed-untrusted, trusted standard/legacy, conflict fail-closed, PROXY v1/v2 trusted/untrusted/disabled/UNKNOWN/malformed, TLS-after-PROXY ordering, H1/H2 parity via shared pipeline, H3 out-of-scope, fuzz ceilings, no-reverse-proxying boundary). H3 datagram proxying remains out of scope; multi-hop beyond one trusted hop remains untrusted by documentation.

## Purpose

Provide a secure way for downstream application servers to obtain original-client and original-scheme/authority metadata when EggServe is deployed behind a trusted reverse proxy or load balancer.

The central rule is: forwarding metadata is untrusted by default. EggServe must never reinterpret `Forwarded`, `X-Forwarded-*`, or a PROXY protocol preamble merely because it is present. Trust must be configured against the immediate peer/listener boundary.

EggServe does not become a reverse proxy in this plan.

## Threat model

Attackers may connect directly to EggServe and send spoofed forwarding headers. Attackers may send bytes beginning with a PROXY v1/v2 signature to a listener that is not configured for PROXY protocol. A misconfigured trusted proxy may append rather than replace untrusted chains. Parsing ambiguity, oversized preambles, and address-family confusion must not produce trusted connection facts.

Security-sensitive downstream features—secure-cookie decisions, URL construction, logging/audit identity, rate limits, IP allowlists—may rely on this metadata, so ambiguous input must fail closed or remain untrusted.

## Track A — Separate peer facts from effective client facts

Extend `ConnectionInfo` or Plan 197's request context to distinguish:

- transport peer/local endpoint observed by the socket;
- optional trusted proxy-reported source/destination endpoint;
- optional trusted forwarded scheme/authority/client identity derived under configured policy;
- provenance/trust source for each effective value.

Do not overwrite the raw peer address. Applications must be able to audit both immediate peer and effective client address.

A conceptual model:

```rust
ConnectionInfo {
    peer_addr,
    local_addr,
    effective_client,
    effective_scheme,
    effective_authority,
    proxy_provenance,
    ...
}
```

Exact field placement depends on Plan 197. Values derived from ordinary untrusted headers must not populate trusted fields.

## Track B — Trusted peer policy

Add explicit configuration identifying which immediate peers/listeners may supply trusted metadata.

Support a small, auditable representation such as exact IPs/CIDRs and/or a listener-level `trusted_proxy` flag for private prebound transports. Avoid DNS-name trust rules in the first implementation because resolution/rebinding semantics create unnecessary ambiguity.

Requirements:

- default: no peer is trusted;
- loopback is not automatically trusted merely because it is loopback;
- trust decision uses the actual immediate transport peer before processing forwarded metadata;
- Unix-domain listeners may have an explicit local-trust mode if required, not implicit global trust;
- configuration validation rejects nonsensical networks/ranges.

If CIDR parsing requires a dependency, choose a narrow maintained crate or implement only the minimal correct address-prefix matching needed; do not build a networking utility framework.

## Track C — HAProxy PROXY protocol v1/v2 transport preamble

Add an optional listener/transport mode that parses PROXY protocol before TLS or HTTP when enabled.

Protocol order must be explicit:

```text
TCP accept -> PROXY preamble (optional) -> TLS handshake (optional) -> HTTP
```

This ordering is required for common TLS-terminating-or-passing load balancer deployments where the PROXY header precedes TLS bytes.

Requirements:

- disabled listeners interpret bytes normally; no magic auto-detection;
- enabled listener accepts only from trusted immediate peers;
- strict small preamble size and timeout before parsing;
- v1 line length bounded and syntax strict;
- v2 signature/version/command/family/protocol/length validated before allocation;
- LOCAL command semantics handled without inventing client identity;
- UNKNOWN/UNSPEC cases preserve truthful absence;
- TLVs are ignored safely by default and bounded even when not interpreted;
- malformed/oversized preamble closes before TLS/HTTP parsing and never reaches a service;
- PROXY source/destination never replaces raw peer/local metadata; it populates a provenance-tagged effective layer.

Do not expose arbitrary PROXY v2 TLV bytes to applications until a concrete consumer justifies a typed, bounded API.

## Track D — `Forwarded` and `X-Forwarded-*` policy

Provide an optional header-derived effective metadata policy for deployments where the trusted proxy terminates HTTP/TLS and forwards ordinary HTTP.

Start conservatively. Support the standardized `Forwarded` header and, if product compatibility warrants it, common `X-Forwarded-For`, `X-Forwarded-Proto`, and `X-Forwarded-Host` through separate explicit configuration.

Required decisions:

- whether the trusted proxy is expected to replace or append chains;
- how many trusted proxy hops are allowed;
- parsing from the right/left based on documented deployment convention;
- validation of IP literals, quoted values, host/port, and scheme tokens;
- conflict behavior when standardized and legacy headers disagree;
- maximum field bytes/elements before service work;
- treatment of `unknown`/obfuscated `Forwarded` identifiers.

Prefer a policy that walks a chain only across explicitly trusted proxy hops. If correctness cannot be guaranteed for arbitrary multi-proxy chains in the first version, support the common single trusted hop first and document it.

Never remove the original forwarding headers from the canonical request unless response/application policy explicitly asks; trusted derived metadata is separate from raw application-visible input.

## Track E — Scheme and authority semantics

Transport-authenticated TLS state remains the source of direct-connection `https`. A trusted proxy may assert original `https` only under the explicit proxy policy.

Similarly, the canonical request's protocol authority/Host remains the actual HTTP message authority. A trusted `Forwarded host=`/`X-Forwarded-Host` may populate an `original_authority`/effective external authority field but must not silently rewrite the canonical request target/Host before routing unless the downstream application chooses to use it.

This prevents hidden security behavior changes in native services.

## Track F — Observability/privacy

Log enough provenance to debug deployments without leaking unnecessary header contents:

- immediate peer;
- whether trusted proxy metadata was accepted;
- source kind (`proxy_v1`, `proxy_v2`, `forwarded`, legacy forwarded);
- sanitized/effective client endpoint according to existing logging privacy policy;
- rejection category for malformed/untrusted preambles.

Do not log full untrusted forwarding chains or arbitrary TLVs by default.

## Track G — Python/Rust exposure

Expose trusted derived metadata through the native request context. Plan 200 may place safe read-only copies into `http::Extensions`; Plan 204 maps them to ASGI `client`, `scheme`, `server`/extensions as appropriate.

The synchronous Python compatibility facade should not silently change `client_address` based on forwarding headers. Any compatibility behavior change would require a separate explicit decision; low-level APIs may expose effective metadata independently.

## Security/adversarial tests

Required cases:

- direct attacker with spoofed `Forwarded`/`X-Forwarded-For` remains untrusted;
- untrusted peer sending valid PROXY v1/v2 is rejected when protocol is enabled for trusted peers only;
- listener with PROXY disabled treats signature bytes as non-HTTP/TLS input normally;
- fragmented preamble across reads;
- slow preamble timeout;
- max-length v1 and oversized v1;
- truncated/oversized v2 length;
- every address family/protocol combination supported or deterministically rejected;
- LOCAL/UNKNOWN semantics;
- duplicate/conflicting forwarded metadata;
- IPv4/IPv6 and ports;
- multi-hop trust boundary cases;
- TLS-after-PROXY ordering;
- H1/H2 over trusted proxy parity;
- no effect on H3 unless a separately specified datagram proxy mechanism is implemented (out of scope here).

Fuzz PROXY and forwarding-header parsers with hard allocation ceilings.

## Acceptance criteria

- [x] raw immediate peer/local metadata is always preserved;
- [x] default configuration trusts no proxy metadata;
- [x] trusted peer policy is explicit and validated before metadata adoption;
- [x] PROXY v1/v2 parsing is optional, bounded, timeout-protected, and occurs before TLS/HTTP;
- [x] malformed or untrusted PROXY input never reaches application services as trusted facts;
- [x] header-derived forwarded metadata has an explicit hop/conflict policy and hard size bounds;
- [x] canonical Host/request-target semantics are not silently rewritten by forwarded values;
- [x] native, Tower, and low-level Python consumers can access provenance-tagged effective metadata;
- [x] synchronous Python compatibility behavior does not silently change;
- [x] EggServe still performs no reverse proxying.

## Handoff

Plan 203 may attach authenticated TLS identity to the same connection/request context. Plan 207 must include spoofing/trust-boundary cases in the application-server qualification matrix.
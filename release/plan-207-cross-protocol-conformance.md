# Plan 207 — Cross-Protocol Application-Server Conformance (closure record)

**Status:** Implemented 2026-09-11. Routine deterministic subset passes on Linux x86_64; expensive interop/soak/browser evidence remains manual per Track J.

## What this plan proves

One normative inventory (`conformance/app_server_conformance.toml`, 55 scenarios, 47 routine) drives qualification. Each semantic scenario is defined once, then run through every applicable transport/consumer. Byte-identical framing is never asserted; canonical application-visible behavior plus protocol-correct lifecycle outcomes are.

## Capability matrix (Plan 207 decision)

| Feature | Native Rust | Tower/`http` adapters | Python low-level | H1 | H2 | H3 | Tier |
|---|---|---|---|---|---|---|---|
| Request metadata (method/target/query/authority/scheme/version) | yes | loss-documented (cross-name header order) | H1 byte-fidelity | yes | yes | yes | experimental `server`, stable `primitives` types |
| Duplicate/opaque headers | yes | opaque via `from_bytes` | sync/async bridges | yes | yes | yes | same as above |
| Peer/local endpoints | yes (`Some` TCP, `None` Unix/caller-owned) | via `ConnectionInfoExt` | `remote_addr` unchanged + `effective_*` getters | yes | yes | yes (QUIC addrs) | experimental |
| Trusted proxy / PROXY / Forwarded provenance | yes, fail-closed | via `ConnectionInfoExt` | config + getters | yes | yes (H1/H2 parity) | ignored (out of scope) | experimental |
| TLS/SNI/ALPN/mTLS metadata | yes | n/a | H1 `HTTPSServer` single-identity | yes (TLS) | yes (ALPN) | separate QUIC identity | experimental (Plan 203) |
| Request body (empty/fixed/chunked/no-length-DATA/incremental/`read_all`/trailers/abandon/over-limit) | yes | `RequestBody: http_body::Body` | `read`/`iter_chunks` + `AsyncRequest` | yes | yes (no chunked; DATA probe) | yes (bounded probe) | experimental |
| Response variants + trailers + interim 1xx | yes | `response_from_http_body` (framing stripped) | `Response.stream` / `AsyncResponse.stream` | yes | yes (connection fallback on stall) | yes (per-stream reset) | experimental |
| Early/full-duplex, lifecycle, shutdown, admission | yes | outer ceiling only | bounded tasks/queues | yes | yes (GOAWAY) | yes (GOAWAY) | experimental |
| Generic tunnels (H1 Upgrade/`CONNECT`, H2/H3 Extended `CONNECT`) | yes | n/a (stays native) | H1 `take_tunnel`/`Tunnel` | yes | yes | yes (plain + Extended; generic `:protocol` blocked by `h3` 0.0.8) | experimental |
| Multiplexed isolation | n/a (H1) | n/a | n/a (H1-only) | n/a | yes | yes | experimental |
| Python event-loop stress | n/a | n/a | bounded (routine) + manual soak | yes | skip (H1-only contract) | skip (H1-only contract) | experimental |

No tier promotion follows from this plan. H2/H3 remain experimental (Plans 191–195 blockers stand). Plan 208 decides promotion.

## Routine evidence (Linux, deterministic)

| Suite | Command | Result |
|---|---|---|
| Inventory schema | `python3 scripts/verify-conformance-matrix.py` | 51 static + 55 app-server (47 routine) pass |
| New cross-protocol subset | `cargo test -p eggserve-core --test cross_protocol_conformance` | 17 pass |
| Same subset, H2/TLS | `cargo test -p eggserve-core --features http2,tls --test cross_protocol_conformance` | 18 pass |
| Same subset, Tower | `cargo test -p eggserve-core --features tower --test cross_protocol_conformance` | 18 pass |
| Owning suites (no duplication) | `cargo test -p eggserve-core --test http2_runtime / http3_runtime / tunnel_upgrade / trailers_interim / trusted_proxy / tls_identity / app_server_consumer / application_service_contract / interop_http_tower (tower)` | pass (existing CI) |
| Python bridge + ASGI fixture | installed wheel job (`test_async_bridge.py`, `asgi_fixture.py`) | pass (Python CI job) |

New file `crates/eggserve-core/tests/cross_protocol_conformance.rs` covers: metadata parity (TCP/prebound/Unix/caller-owned), endpoint truthfulness, untrusted-Forwarded fail-closed, empty/fixed/chunked/over-limit/abandoned bodies, response variants, HEAD/204 no-poll, interim bounds, panic sanitization, framing-ambiguity rejection, oversized-target 414, admission saturation/recovery, tunnel denial + H1 echo, H2 sibling survival (`http2`), Tower parity (`tower`). H1 TLS, H2 TLS, H3, and `http-interop` streaming details are owned by their existing suites and referenced from the inventory rather than duplicated.

## Manual / release-only evidence (fail-closed)

- Two-client H2/H3 interop, h2spec, browsers, adversarial wire, impairment, cross-platform H2/H3 runtime, soak with memory-growth observation, slowloris rates, and same-machine perf sanity (`benchmarks/207-conformance/` when executed) are **not** routine gates. `scripts/qualify-http2.sh` / `scripts/qualify-http3.sh` record PASS/SKIP and fail closed under `EGGSERVE_REQUIRE_*` flags; release qualification must mark the affected tier blocked when mandatory evidence is unavailable.
- Python H2/H3 is an explicit capability skip (facade remains H1-only by design), not a gap.
- H3 over Unix stream is impossible by construction and never listed.

## Security properties re-checked

- Spoofed `Forwarded`/`X-Forwarded-*` and untrusted PROXY preambles cannot forge trusted identity (fail-closed, raw peer preserved; canonical Host/target never rewritten).
- Oversized targets/headers/trailers bounded before expensive work (414/431/400).
- Framing ambiguity rejected before service invocation; malformed service output cannot corrupt framing (runtime sole framing authority).
- Tunnel denial stays ordinary HTTP; malformed upgrade/Extended CONNECT never yields a capability.

## Files

- `conformance/app_server_conformance.toml` (normative inventory)
- `crates/eggserve-core/tests/cross_protocol_conformance.rs` (routine subset)
- `scripts/verify-conformance-matrix.py` (validates both inventories)
- This record

## Handoff

Plan 208 consumes this evidence plus Plans 197–206 to decide stable vs experimental tiers. Support status may differ between Rust runtime and Python consumer; state explicitly.

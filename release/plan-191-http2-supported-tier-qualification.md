# Plan 191 HTTP/2 Supported-Tier Promotion Qualification

Date: 2026-09-10
Candidate base: `04693ec14266284bc2927b224bfa99591eeb9de6`
Decision: **remain experimental — promotion blocked (no browser, no macOS/Windows runtime evidence)**

## Scope and inventory

Plan 191 attempted to promote the existing opt-in Rust `http2` feature from
experimental to supported, opt-in. No runtime source change was required: all
qualification passed against the candidate's implementation, and the only
repository change in this pass is test-harness hardening
(`scripts/qualify-http2.sh` fail-closed promotion gates), this closure record,
and a short blocker pointer in `architecture/http2.md`. No dependency was
added or upgraded; no Python, default-feature, or `server`-API surface changed.

| Item | Observed value |
|---|---|
| Rust stable / MSRV | 1.98.1 / 1.88 (`cargo +1.88 check` green) |
| Hyper / h2 / hyper-util | 1.11.1 / 0.4.19 / 0.1.20 |
| rustls / tokio-rustls / tokio | 0.23.41 / 0.26.4 / 1.52.3 |
| OS/arch | Linux x86_64 (Ubuntu 24.04) — only platform executed |
| Independent client family 1 | curl 8.5.0 with libnghttp2 1.59.0 |
| Independent client family 2 | python-h2 4.4.1 (pure-Python HPACK/H2, not libnghttp2) |
| Same-family frontends | nghttp 1.59.0, h2load 1.59.0 (libnghttp2; not counted as a second family) |
| Standards exerciser | h2spec 2.6.0 (RFC 7540/7541-era; classified against RFC 9113) |
| Unavailable | browsers, macOS/Windows runtime, Linux aarch64 runtime |
| Transports tested | TLS ALPN and cleartext prior knowledge; `h2c` Upgrade absent |
| Certificates | temporary self-signed loopback certs; verification enabled shape checked via ALPN + nghttp note |
| H2 limits | defaults (100 concurrent streams, 32 KiB header list, 16 KiB frame, 256 KiB stream window, 1 MiB connection window, 256 KiB per-stream send buffer, 1024 local / 20 peer reset states) |

Dependency triage (Track K): `cargo audit` and `cargo deny check` pass with no
advisories; `cargo tree` confirms the minimal graph gains no H2 dependency
without the feature. No Hyper/h2 advisory affecting SETTINGS, HPACK
accounting, flow control, reset retention, GOAWAY, cancellation, shutdown, or
unbounded allocation was open against the pinned versions at execution time.
Per the plan, nothing was upgraded merely because newer versions exist.

Standards matrix (RFC 9110 semantics, RFC 9113 transport, RFC 7541 HPACK,
RFC 7301 ALPN, TLS 1.3 via rustls): EggServe owns adaptation/policy/limits/
lifecycle/admission/response finalization; Hyper/h2 own frame parsing, HPACK,
stream state, windows, and resets under EggServe-configured bounds; ALPN
selection is rustls plus the EggServe selector. Every EggServe-owned boundary
below has direct evidence; pass-through behavior has version review plus the
adversarial sampling in Track C.

## Track B/H — independent-client matrix (two families)

Family 1 (curl/libnghttp2) via `scripts/qualify-http2.sh` baseline: TLS ALPN
selects `h2` with `--http2` and `http/1.1` with `--http1.1`; static GET,
HEAD-compatible framing, `content-length: 52` with no `connection`/
`transfer-encoding`, range 206 (`This`), conditional 304 via ETag, parallel
streams, cleartext prior-knowledge H2, and HTTP/1 `Upgrade: h2c` remaining
HTTP/1.1. `nghttp` (same family) negotiates `h2` and receives `:status: 200`
with `content-length: 52`.

Family 2 (python-h2 4.4.1) interop probes against the same build: TLS ALPN
negotiates `h2` with a 200 GET; cleartext prior-knowledge GET returns 200 with
the fixture body and no `connection`/`transfer-encoding`; an extended matrix
on one connection verified 4× multiplexed GET, HEAD (200, empty body,
`content-length: 52`), 404, POST → 405, single-range 206, conditional 304,
and unsatisfiable-range 416.

H1 fallback when the client offers only `http/1.1` is verified by curl on the
same TLS listener. Upgrade-based `h2c` is not accepted (cleartext Upgrade
request stays HTTP/1.1). Request cancellation/reset followed by sibling use is
covered by the deterministic Reject/sibling tests plus the live sibling checks
in Tracks E/F.

## Track C — h2spec and adversarial cases

h2spec 2.6.0, strict mode, cleartext prior knowledge:

- `hpack`: 8/8 pass.
- `http2` strict: 92–94/95 across reruns (94 once, 92 on the clean rerun).
- generic (all specs): 145/147.

Consistent deltas, classified against RFC 9113 rather than the exerciser's
RFC 7540 baseline:

1. `3.5.2 invalid connection preface` — server closes TCP without emitting
   GOAWAY (`unexpected EOF`). Ownership: delegated (Hyper/classifier preface
   handling). The connection is always terminated, no service invocation, no
   ambiguity with H1 parsing. Bounded and safe; GOAWAY-shape only.
2. `8.1 second HEADERS without END_STREAM` and `8.1.2.1.3 pseudo-header
   trailers` — server emits the bounded canonical error response (22-byte
   4xx HEADERS+DATA) followed by RST, instead of RST-only. Ownership:
   transport-scope trailer handling is Hyper/h2-owned; EggServe answers the
   delivered request with a bounded 4xx through the canonical pipeline.
   Sibling streams are unaffected and no unbounded state accrues. The exact
   frame (DATA vs RST first) races run to run (hence 92 vs 94), which is
   itself recorded as a determinism follow-up, not a silent pass.

Zero EggServe-owned unbounded/unsafe failures: every delta terminates the
stream or connection in a bounded way with no service confusion (the static
service rejects bodies/trailers by policy). The trailer-scope race is the one
item carried as follow-up work rather than a clean pass.

## Tracks D–G — multiplexing, headers, flow control, GOAWAY

- Multiplexing/admission: h2load at 2c×5m serves 200/200 2xx; at 4c×16m the
  server sheds via bounded 5xx (service admission) while staying responsive;
  a 100-connection burst sheds at the connection bound (200s, bounded 503s,
  handshake-time resets — the same shedding shape as H1 under an identical
  burst) and serves 200 immediately after. Counters/permits recover; no
  unbounded task/permit growth.
- Header/HPACK pressure: 50 small headers → 200; one 20 KiB header → 200; one
  40 KiB header → 431 pre-service with the next request on a fresh connection
  at 200 (stream-local scope, siblings usable). Header *count* (150 tiny
  headers → 200) is intentionally byte-bounded rather than count-bounded on
  H2: Hyper's H2 builder exposes a decoded-list byte ceiling (32 KiB), not a
  field-count knob, and EggServe's aggregate post-decode ceiling (32 KiB)
  bounds both. No HPACK implementation or fuzzer was added.
- Flow-control stall (F3): a python-h2 client that never acknowledges DATA
  receives exactly one connection window (65535 bytes) of a 2 MiB file, a
  sibling stream still receives its response HEADERS, and the server issues
  GOAWAY at the 5 s `response_write_timeout` (elapsed 5.0 s). Memory stays
  bounded by configured transport/application buffers; the server serves 200
  right after. Producer/poll-progress accounting with connection-fallback
  semantics holds as documented.
- GOAWAY/drain: GOAWAY observed live on the stall fallback; SIGTERM on an
  idle H2 server drains the listener (new connections refused, process
  exits); in-flight drain races remain covered by the deterministic
  lifecycle suite. `max_requests_per_connection` drain uses the H2 graceful
  path per the existing deterministic tests.

## Track J — load/resource characterization

h2load (nghttp2 1.59.0, TLS 1.3, `h2` ALPN) on loopback: 1000 requests over
10 connections × 10 streams complete in ~230 ms with zero timeouts/errors;
over-budget concurrency sheds as bounded 5xx with post-load 200s. No
pathological regression vs the H1 burst shape, no unbounded memory/task/
permit growth. Absolute rates are same-machine observations, not gates.

## Track I — platform runtime (blocker)

Actual H2 network qualification exists only for Linux x86_64. macOS (either
architecture), Windows x86-64, and Linux aarch64 received no runtime runs in
this pass — compile-only evidence is explicitly not counted. This alone keeps
H2 experimental per the plan's decision rule.

## Track L — harness

`scripts/qualify-http2.sh` now fails closed for promotion: it records
toolchain/client versions, counts implementation *families* (curl/nghttp
collapse to libnghttp2; python-h2 counts when importable), and
`EGGSERVE_REQUIRE_TWO_H2_CLIENTS=1`,
`EGGSERVE_REQUIRE_BROWSER_EVIDENCE=1` (via
`EGGSERVE_H2_BROWSER_EVIDENCE`), and `EGGSERVE_REQUIRE_PLATFORM_EVIDENCE=1`
(via `EGGSERVE_H2_PLATFORM_EVIDENCE`) each exit 2 when the mandatory evidence
is absent. Baseline (no gates) still passes; each strict gate was verified to
exit 2 on this host. Routine CI is unchanged (deterministic H2 tests only).

## Deterministic and supply-chain gates

Green on the candidate tree (re-run after the harness/docs change before the
final commit):

```text
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-python-release-metadata.py
cargo fmt --all -- --check
cargo +1.88 check --workspace --all-targets --features http3,tls
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo clippy -p eggserve-core --features http2,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http2,tls
cargo clippy -p eggserve-bin --features http2,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http2,tls
cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features tls
cargo clippy -p eggserve-core --features http3,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http3,tls
cargo clippy -p eggserve-bin --features http3,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http3,tls
cargo audit
cargo deny check
bash scripts/qualify-http2.sh
EGGSERVE_REQUIRE_TWO_H2_CLIENTS=1 bash scripts/qualify-http2.sh  # exit 2 here
```

## Remaining limitations and blockers

1. **Browser evidence: absent** (mandatory gate).
2. **Platform runtime: Linux x86_64 only** — macOS, Windows, Linux aarch64 absent (mandatory gate).
3. **Trailer-scope determinism follow-up**: the RST-vs-4xx race in strict
   h2spec trailer cases is bounded and delegated, but a future pass should
   either pin the expected Hyper behavior or add a deterministic EggServe-side
   trailer check with a regression test.
4. **Public stream-local wire-progress/reset hook**: still absent from the
   maintained Hyper API; the documented producer-progress plus
   connection-fallback contract is unchanged and now has live F3 evidence.

## Final tier

**Native HTTP/2 remains experimental (opt-in).** H1 remains the default and
minimal protocol; Python remains HTTP/1.1-shaped; H2 is not default-enabled.
Live documentation is not promoted; the only doc touch is the blocker pointer
in `architecture/http2.md`. A future promotion attempt reuses this record's
two-family, h2spec-classification, and F3 evidence and must close blockers
1–2 (plus 3 for a clean standards sheet).

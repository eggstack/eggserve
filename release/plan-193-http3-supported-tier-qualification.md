# Plan 193 HTTP/3 Supported-Tier Promotion Qualification

Date: 2026-09-10
Candidate base: `71dc693 Close Plan 192 HTTP/3 dependency readiness as BLOCKED`
Environment: Linux x86_64 (Ubuntu 24.04, kernel 6.8.0-139-generic), loopback qualification
Decision: **remain experimental — promotion blocked (entry prerequisite unmet plus missing mandatory evidence)**

## Scope

Plan 193 attempted to promote the existing opt-in Rust `http3` feature from
experimental to supported, opt-in, against the frozen Plan 192 candidate. No
runtime source change was required or made: the candidate already carries the
Plan 192 narrow hardening, and every deterministic gate passes against it. The
only repository changes in this pass are this closure record, the CLOSED status
pointer on the plan file, and short blocker pointers in the live documentation.
No dependency was added or upgraded; no Python, default-feature, or
`server`-API surface changed.

The entry prerequisite is itself unmet: Plan 192 closed `BLOCKED`, not `READY
FOR PLAN 193`. Per the plan's own decision rule ("If Plan 192 is `BLOCKED`, do
not execute this plan until its blocker is resolved"), a promotion campaign
cannot proceed. This pass therefore records the candidate freeze, re-checks the
named upstream blockers, probes every mandatory evidence class on this host,
and closes with H3 experimental — the same honest outcome shape as the Plan 191
H2 promotion attempt.

## Track A — Candidate freeze and evidence inventory

| Item | Observed value |
|---|---|
| Candidate commit | `71dc693` (current `main`, directly after the Plan 192 closure) |
| `h3` / `h3-quinn` / `quinn` | 0.0.8 / 0.0.10 / 0.11.11 — identical to the Plan 192 frozen candidate; no newer maintained release exists |
| `rustls` / `ring` / `rustls-pki-types` | 0.23.41 / 0.17.14 / 1.15.0 — unchanged |
| Rust stable / MSRV | 1.98.1 / 1.88 (`cargo +1.88 check --features http3,tls` green) |
| H3/QUIC feature flags | `http3` (implies `tls`); `h3`/`h3-quinn` with `default-features = false`; `quinn` with `runtime-tokio` + `rustls-ring` only |
| Certificate setup | temporary self-signed loopback certs via `openssl req -x509` (qualification script); verification-enabled shape unchanged |
| Independent H3 family 1 | none available on this host (curl 8.5.0 has no HTTP/3 support) |
| Independent H3 family 2 | none available (no nghttp3-client, quiche-client, or aioquic tooling) |
| Adversarial H3 client | none (`h3i` not installed) |
| Browser for Alt-Svc | none (no Chromium/Chrome/Firefox on this host) |
| Operating systems | Linux x86_64 only; no macOS/Windows runtime in this pass |
| Network impairment tooling | `tc` present but no H3 wire client to impair against; no impairment run performed |
| Configurations | defaults plus the script's same-port Alt-Svc and H1-fallback checks (`max_concurrent_bidi_streams = 100`, 32 KiB field section, 60 s QUIC idle, stateless retry off, Alt-Svc opt-in) |

Dependency drift check: `Cargo.lock` versions for `h3`, `h3-quinn`,
`quinn`, and `rustls` are byte-identical to the Plan 192 inventory. The
candidate under test is exactly the candidate Plan 192 froze.

## Upstream re-check (Plan 192 blockers carry over)

Re-checked 2026-09-10 against the `hyperium/h3` issue index and the local
lockfile:

1. **`hyperium/h3#338` (open, blocker)** — "FrameStream::poll_next / poll_data
   discards already-buffered bytes when QUIC connection error arrives in same
   recv batch" remains Open with no released fix; fix PR #339 remains
   unmerged. `h3` 0.0.8 is still the latest published release, so there is no
   upgrade candidate carrying the drain-before-error fix. EggServe's
   server-side HEADERS/DATA paths still traverse the affected frame layer (see
   the Plan 192 disposition). Still blocks promotion.
2. **`hyperium/h3#262` remainder (open, blocker)** — still open upstream; the
   three Plan 192 early-error fixes stand, and the three residual paths
   (503-admission with Stream-owned receive, Buffer `read_all` failure,
   post-service unconsumed Stream body) still drop receive without an explicit
   public-API abort. Still blocks promotion.
3. **Advisories** — `cargo audit` reports no vulnerabilities for the locked
   graph; `cargo deny check` passes (advisories, bans, licenses, sources). No
   new QUIC/TLS advisory affecting the configured subset.

No dependency or MSRV change was made during this pass, so no return to Plan
192 readiness was triggered beyond this re-check.

## Tracks B–C — Independent clients and browser (blocked)

- **B1/B2/B3 (direct semantic matrix, both families)**: not runnable. `curl
  -V` on this host lists no `HTTP3`; no second H3 implementation family is
  installed. Zero of the two required independent non-Quinn families
  interoperated with EggServe in this pass. The deterministic in-process
  Quinn/h3 suite (9/9, see below) remains regression evidence only and does
  not satisfy this gate.
- **Track C (browser Alt-Svc discovery/use/fallback)**: not runnable. No
  mainstream browser is installed, so the real discovery path (TCP Alt-Svc
  advertisement → cached H3 use → UDP-blackhole fallback → re-discovery) has
  no evidence. The script-level Alt-Svc unit checks still pass (same-port
  `h3=":<port>"; ma=86400`, port-0 resolution, denylist suppression,
  runtime-ownership), but those are not browser evidence.

## Tracks D–E — Adversarial frames and QPACK/header pressure (blocked)

- **Track D**: not runnable. `h3i` (or equivalent frame-level exerciser) is
  not installed, so no duplicate-control-stream, SETTINGS, frame-placement,
  pseudo-header, reset/stop-race, or GOAWAY-sequencing cases were exercised
  against this candidate.
- **Track E**: not runnable independently. Decoded field-section limits remain
  enforced pre-service in-process (`max_field_section_size` projected to both
  accept and decode), but compressed-field expansion near the ceiling,
  dynamic-state pressure, and control/QPACK-stream reset behavior have no
  adversarial-client evidence.

## Tracks F–I — Resources, flow control, bodies, shutdown (deterministic only)

In-process deterministic coverage is unchanged and green (see verification
below), including DATA-without-`Content-Length`, zero-length-plus-DATA,
bodyless dispatch, probe timeouts, sibling survival, lifecycle wake-up,
early-error stream scoping, and complete-response survival across peer close.
None of the following wire-level gates is met:

- **F (100-stream/concurrency pressure)**: no 100-simultaneous-stream run
  against an independent client; transport-vs-service budget separation has no
  live evidence in this pass.
- **G (stalled vs slow-progressing response)**: no real-credit-withholding
  client; per-stream no-progress timeout behavior has deterministic
  representation only.
- **H (body flow control/cancellation)**: no slow/stopped/reset-mid-body wire
  runs; stream-local cancellation scope has deterministic evidence only.
- **I (GOAWAY/drain races)**: idle/shutdown paths covered deterministically;
  racing new requests against GOAWAY under load with independent clients was
  not exercised.

## Tracks J–K — Network impairment and QUIC interop (blocked)

- **J1–J5**: no impairment runs. `tc` exists on the host, but with no H3 wire
  client there is nothing to impair; loss/latency/jitter/reordering/MTU/UDP-
  blackhole behavior of the EggServe H3 endpoint is unqualified. TCP fallback
  health without an H3 endpoint still passes at the script level, which is not
  the J5 UDP-blackhole-with-H3-configured case.
- **Track K**: no QUIC Interop Runner (or equivalent harness) integration was
  performed. Per the plan this does not standalone-block if independent-client
  plus impairment evidence exists — but that evidence does not exist either.

## Track L — Platform runtime (blocked)

Actual H3 traffic in this pass is TCP-fallback only (no direct H3 wire client
on any OS). Linux x86_64, macOS, Windows, and Linux aarch64 all lack H3 wire
runtime runs here — including Linux, because "runs actual UDP/QUIC/H3
traffic" is the bar and this host cannot generate it. Compile-only and
fallback-only evidence is explicitly not counted. This alone keeps H3
experimental per the plan's decision rule.

## Track M — Security/privacy review

No runtime source change in this pass, so the Plan 192 review stands: fresh
QUIC rustls config pinned to TLS 1.3 with exactly `h3` ALPN, application
0-RTT refused, no client auth, PEM handling unchanged, no transport secrets /
connection IDs / tokens / packets logged, Alt-Svc obeying the privacy
denylist, stateless retry explicit (default off). Canonical errors remain
generic. Advisory re-check (above) is clean; the two named upstream blockers
are the known-issue carryover, not hidden state.

## Track N — Performance and memory characterization

No H3 wire characterization was possible without a wire client; no new
characterization numbers are recorded here. The standing constraints are
unchanged: no unbounded growth observed deterministically, defaults remain
local/SBC-sized, and H3 cost stays isolated behind the `http3` feature (the
no-feature tree contains no `h3`/`h3-quinn`/`quinn`, enforced by
`scripts/qualify-http3.sh`).

## Track O — Supply chain, MSRV, and feature graph

Green on the execution tree:

- `cargo audit`: no vulnerabilities (locked graph).
- `cargo deny check`: advisories, bans, licenses, sources ok.
- `cargo +1.88 check --workspace --all-targets --features http3,tls`: green
  (workspace MSRV floor preserved).
- `cargo tree` (no-default-features): no `h3`/`h3-quinn`/`quinn`; minimal and
  default builds remain free of H3/QUIC dependencies.

The frozen Plan 192 dependency set is exactly what was tested here.

## Track P — Qualification harness and reproducibility

`scripts/qualify-http3.sh` is unchanged in this pass and was re-probed: the
baseline completes (TCP fallback + same-port Alt-Svc + H1-fallback checks),
reports `direct-h3-clients: <none>` with every evidence class as `SKIP`
(never `PASS`), and each fail-closed gate exits 2 as designed:

- `EGGSERVE_REQUIRE_H3_CLIENTS=1` → exit 2
- `EGGSERVE_REQUIRE_TWO_H3_CLIENTS=1` → exit 2
- `EGGSERVE_REQUIRE_ADVERSARIAL_H3=1` → exit 2
- `EGGSERVE_REQUIRE_H3_BROWSER=1` → exit 2
- `EGGSERVE_REQUIRE_H3_IMPAIRMENT=1` → exit 2
- `EGGSERVE_REQUIRE_H3_PLATFORM=1` (on Linux) → exit 2

Routine CI remains deterministic same-stack tests plus compile
representatives; external-client/browser/netem/multi-OS matrices stay manual
release qualification, as the plan requires.

## Deterministic and supply-chain gates

Green on the candidate tree (before and after the docs-only change, which
touches no source):

```text
python3 scripts/verify-conformance-matrix.py            # 51 entries validated
python3 scripts/check-python-release-metadata.py        # preflight passed (0.1.2)
cargo fmt --all -- --check
cargo +1.88 check --workspace --all-targets --features http3,tls
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace                                 # 1729 passed, 3 ignored (53 suites)
cargo clippy -p eggserve-core --features http2,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http2,tls        # 1630 passed, 3 ignored (46 suites)
cargo clippy -p eggserve-bin --features http2,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http2,tls         # 141 passed (7 suites)
cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features tls               # 141 passed (7 suites)
cargo clippy -p eggserve-core --features http3,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http3,tls        # 1643 passed, 3 ignored (46 suites; incl. 9-test H3 suite)
cargo clippy -p eggserve-bin --features http3,tls --lib --bins --tests -- -D warnings
cargo test -p eggserve-bin --features http3,tls         # 141 passed (7 suites)
cargo audit / cargo deny check
bash scripts/qualify-http3.sh                           # baseline passes; all strict gates exit 2
```

## Remaining limitations and blockers

1. **Entry prerequisite unmet** — Plan 192 is `BLOCKED`, not `READY FOR PLAN
   193`. A future promotion campaign needs a narrow readiness update that
   resolves `hyperium/h3#338` (released fix) and the `#262` remainder (every
   termination path owning both directions) and re-freezes the candidate.
2. **Two independent non-Quinn H3 families**: absent (mandatory gate).
3. **Browser Alt-Svc discovery/use/fallback**: absent (mandatory gate).
4. **Adversarial H3 frame/state coverage**: absent (mandatory gate).
5. **Network impairment + UDP-blackhole-with-H3**: absent (mandatory gate).
6. **Platform runtime with actual H3 traffic** (Linux x86-64, macOS,
   Windows): absent (mandatory gate); Linux aarch64 likewise unrecorded.
7. **Wire-level resource/flow-control/body/drain evidence** (Tracks F–I live
   portions): absent; deterministic evidence only.
8. **QUIC interop harness**: not integrated; equivalent evidence also absent.

## Final tier

**Native HTTP/3 remains experimental (opt-in).** H1 remains the default and
minimal protocol; Python remains HTTP/1.1-shaped; H3 is not default-enabled.
Live documentation is not promoted; the only doc touches are this record and
blocker pointers. A future promotion attempt reuses the Plan 192 candidate
audit, this record's freeze/gate probes, and the fail-closed harness, and must
close blockers 1–6 (plus 7–8 for a complete evidence sheet). Plan 193 is no
longer an open promotion authority — a new scoped plan is required once the
readiness blockers resolve.

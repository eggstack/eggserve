# Plan 290 — Direct H1 boundary-ownership qualification and version decision

Status: **COMPLETE**.

Program:
`plans/288-291-direct-h1-boundary-ownership-followup-program.md`.

Planning baseline:
`2d4bae12f8cb87c01a8496d765d18b1380871e72`.

## Goal

Qualify the combined parser-range, aggregate-header ownership, and
service-response Date/Server ownership contract from Plans 288–289, prove
default non-regression, and make the semver/package decision for publication.

This plan is evidence/qualification authority. Do not add new architecture
unless qualification exposes a concrete defect.

## Track A — freeze the candidate

Record:

- implementation SHA(s);
- resolved Rust toolchain;
- `hyper` / `hyper-util` versions;
- package versions before the candidate;
- public API diff;
- direct no-default-features dependency graph.

Re-run `cargo info`/registry checks so the version decision is based on live
crates.io state.

## Track B — parser range qualification

Prove both validation and real H1 execution above the former EggServe maxima.

Required cases:

1. `max_buf_size = 4 MiB + 1` validates;
2. a caller-owned H1 connection configured above 4 MiB handles a request whose
   parsed header/request footprint crosses the former 4 MiB hard gate without
   an EggServe config rejection;
3. `max_headers = 10_001` validates;
4. a request with 10,001 small legal headers reaches the service when parser
   buffer capacity is sufficient;
5. 8191 still fails validation before Hyper;
6. `max_headers = 0` still fails;
7. default 64 KiB / 100 behavior remains unchanged.

Keep the oversized qualification fixtures bounded and deterministic. They are
not load tests and must not allocate near `usize::MAX`.

Record CPU/RSS only if useful to explain the explicit resource cost; no
performance improvement claim is needed.

## Track C — aggregate-header ownership qualification

With default EggServe ownership:

- existing 32 KiB behavior remains;
- over-limit request -> 431;
- counter/event behavior remains.

With External ownership on the projected H1 policy:

- a request below 1 KiB is not rejected merely because the stored
  `max_header_bytes` validator minimum is 1 KiB;
- a request above 1 MiB aggregate name+value bytes can reach the service when
  parser limits permit;
- the stored RuntimeConfig placeholder may remain a normal valid value;
- parser `max_headers` / `max_buf_size` still reject independently.

This proves policy transfer rather than "set a bigger EggServe ceiling".

## Track D — per-response Date/Server qualification

Use one service/runtime and multiple sequential requests.

Required service responses:

| Case | Date | Server | Expected under External ownership |
| --- | --- | --- | --- |
| A | valid date A | alpha | exact A / alpha |
| B | valid date B | beta | exact B / beta |
| C | absent | absent | both absent |
| D | invalid | gamma | generic runtime 500; invalid Date absent |
| E | duplicate Date | gamma | generic runtime 500 |
| F | valid Date + future Last-Modified | delta | Date kept, future Last-Modified removed |

Run the same A/B/C service responses under default ownership and prove the
historical EggServe policy still subordinates them.

Also prove:

- explicit `server` denylist wins over External Server ownership;
- unrelated duplicate headers survive;
- HEAD and body-forbidden statuses remain correctly framed;
- streaming body responses retain metadata;
- 101/tunnel handshake retains service metadata only where permitted.

## Track E — runtime-error isolation

Under External service metadata ownership, trigger:

- request-target rejection;
- aggregate-header rejection while EggServe owns that ceiling;
- body rejection;
- handler timeout;
- service panic;
- invalid external Date fallback.

Each must use the runtime-global `ResponsePolicy`, not service metadata.

A custom `RuntimeRejectionPresenter` still cannot set Date/Server.

## Track F — direct embedding fixture

Build a generic caller-owned H1 fixture, not a project-specific adapter:

```text
caller-owned TcpStream or Rustls TlsStream
  -> validated H1ConnectionPolicy
       - parser values above former maxima
       - aggregate header owner External
       - Date/Server owner External
  -> generic Service
  -> canonical streaming response
```

Include the real caller-owned Rustls/Tokio-Rustls H1 path already established
by Plan 285 so the new policy composition is proven on the published embedding
shape.

The direct graph must remain free of `eggserve-core`, `eggserve-static`,
and PHF.

## Track G — Tower/http-interop qualification

The service-response provenance change must work for direct Tower/Axum
composition because those adapters ultimately return canonical service
responses.

Test at least:

- Axum response with service Date/Server under External ownership;
- ordinary default ownership remains unchanged;
- streaming body;
- duplicate application headers.

No new Tower-specific policy API.

## Track H — compatibility/high-level regression

Run:

- direct `eggserve-server` default tests;
- `eggserve-core` compatibility tests;
- H2 tests;
- H3 tests where current CI normally runs them;
- TLS-bin and package/topology checks;
- Python metadata checks if touched indirectly by synchronized release files.

Explicitly prove the shared parser validation widening does not modify H2's
independent `Http2Config.max_header_list_size`.

## Track I — API/semver decision

Preferred implementation from Plans 288–289 is additive:

- new public ownership type/methods;
- private H1ConnectionPolicy fields;
- wider accepted values;
- no removed/ret-typed public fields/constants.

If that remains true, a compatible `0.3.x` patch is appropriate subject to
live registry state.

Select a new pre-1.0 minor instead if implementation causes any source
incompatibility such as:

- adding a field to an exhaustively constructible public struct;
- changing `ResponsePolicy` field types;
- removing/renaming public runtime-limit constants;
- changing an existing method signature.

Record the decision in:

`release/plan-290-direct-h1-boundary-ownership-qualification.md`.

## Track J — documentation truth

Before closure, docs must state:

- old parser maxima were EggServe conservative policy, not Hyper requirements;
- defaults remain unchanged;
- large parser values increase resource exposure;
- external aggregate-header ownership is narrow and explicit;
- external Date/Server ownership applies to successful service responses;
- runtime-generated errors remain runtime-owned;
- external Date ownership carries RFC responsibility.

Do not advertise a registry version before Plan 291 publishes it.

## Verification

At minimum:

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo check -p eggserve-server --no-default-features
cargo check -p eggserve-server --no-default-features --features http-interop
cargo check -p eggserve-server --no-default-features --features tower
cargo test -p eggserve-core
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-python-release-metadata.py
```

Run the repository's full ordinary CI matrix and retain the exact hosted-CI
run for the proof-bearing source SHA.

## Acceptance criteria

- [x] real direct H1 works above former 4 MiB / 10,000 parser gates.
- [x] real direct H1 can externalize aggregate header-byte policy.
- [x] parser protections remain active independently.
- [x] per-response Date/Server A/B/C cases pass on one runtime.
- [x] invalid/duplicate external Date fails safely.
- [x] runtime errors retain runtime metadata authority.
- [x] Last-Modified invariant remains.
- [x] Tower/http-interop direct path passes.
- [x] default/high-level/core/H2/H3 regressions pass.
- [x] direct graph topology remains narrow.
- [x] public API compatibility is classified.
- [x] exact release version strategy is recorded.
- [x] proof-bearing SHA has green hosted CI.

Evidence: `release/plan-290-direct-h1-boundary-ownership-qualification.md`.
The implementation SHA `dc39fef20dd658755ef268c4cd82916448fa3da1` passed
hosted CI run `36102738591`.

## Non-goals

- No publication.
- No downstream project migration.
- No H2/H3 support-tier change.
- No performance marketing claim.

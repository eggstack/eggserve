# Plans 288–291 — Direct H1 boundary-ownership follow-up program

Status: **IN PROGRESS** (Plans 288–289 implemented; Plans 290–291 remain).

Planning baseline:

```text
2d4bae12f8cb87c01a8496d765d18b1380871e72
docs: refresh agent skill and docs past Plan 286 (287)
```

## Purpose

Close two generic direct-H1 embedding gaps discovered after the Plan 286
`0.3.0` publication without weakening EggServe's hardened standalone
defaults:

1. the direct H1 parser accepts only EggServe's conservative hard-coded
   `max_buf_size` / `max_headers` ranges even though the underlying Hyper
   1.11.1 builder has no corresponding documented upper limit; and
2. the final H1 response boundary always owns `Date` and `Server`, so a
   caller that legitimately owns per-response metadata cannot preserve those
   fields through EggServe.

The same review also identified a third, related distinction: aggregate
post-parse request-header bytes are an EggServe semantic policy, not a Hyper
parser requirement. A direct embedder that owns that policy needs an explicit
opt-out, rather than an arbitrarily larger EggServe limit.

This program must remain generic. It does not add WAF-, proxy-, CDN-, router-,
or application-specific behavior.

## Current implementation facts

At the planning baseline:

- workspace lock resolves `hyper 1.11.1` and `hyper-util 0.1.20`;
- `eggserve-server::runtime_limits` enforces:
  - `8192 <= max_buf_size <= 4 MiB`;
  - `1 <= max_headers <= 10_000`;
  - `1 KiB <= max_header_bytes <= 1 MiB`;
- the H1 builder passes `max_buf_size` and `max_headers` directly to
  Hyper and disables Hyper's automatic Date generation;
- Hyper's public H1 server API documents only the 8192 minimum for
  `max_buf_size`; `max_headers` accepts `usize`;
- `convert_request_head` always applies `max_header_bytes` before service
  work;
- `InFlightGuard::finish` applies `finalize_runtime_response` to every H1
  response;
- that finalizer always removes service `Date` and `Server`, then applies
  the runtime-global `ResponsePolicy`;
- runtime-generated errors and service-generated responses currently share
  that final metadata step;
- direct `H1ConnectionPolicy` fields are private and the type is produced
  by validated projection, giving EggServe an additive place to expose
  embedding-only ownership overrides without adding fields to public
  `RuntimeConfig` or `ResponsePolicy`;
- high-level `Server`, H2, H3, static serving, and compatibility-core
  behavior do not require these embedding overrides.

RFC 9110 remains the semantic reference: an origin with a clock owns the
responsibility to emit a valid Date on 2xx/3xx/4xx responses, and
`Last-Modified` must not be later than the message origination time.
External Date ownership therefore transfers responsibility; it must not mean
"accept malformed Date".

## Architecture

```text
RuntimeConfig
  hardened defaults + shared validation
       |
       v
H1ConnectionPolicy
  private fields / validated projection
       |
       +-- default: EggServe owns aggregate header ceiling
       |            EggServe owns service Date/Server
       |
       '-- explicit direct-H1 embedding overrides
             aggregate header bytes -> External
             service Date          -> External
             service Server        -> External
       |
       v
canonical H1 pipeline
  Hyper parser controls remain explicit
  service-vs-runtime response provenance remains explicit
```

Parser memory/header-count controls remain numeric Hyper parser controls;
they are not disabled. The change is to stop inventing an EggServe upper
bound that Hyper itself does not require.

Aggregate header-byte enforcement remains bounded and enabled by default;
only an explicit direct-H1 policy override may transfer it to the embedder.

Runtime-generated errors remain EggServe metadata-owned even when service
response metadata is externally owned.

## Execution order

```text
288  parser-range expansion + external aggregate-header ownership
 |
289  service-response Date/Server ownership
 |
290  combined qualification + API/semver decision
 |
291  publication + registry-only/downstream closure
```

Plans 288 and 289 may be implemented in parallel from the same baseline, but
Plan 290 must qualify their combined behavior.

Implementation plans:

- `plans/288-direct-h1-parser-range-and-header-ceiling-ownership.md`
- `plans/289-direct-h1-service-response-metadata-ownership.md`
- `plans/290-direct-h1-boundary-ownership-qualification-and-version-decision.md`
- `plans/291-direct-h1-boundary-ownership-publication-closure.md`

## Frozen invariants

- secure standalone/default `RuntimeConfig` values do not change;
- `header_read_timeout`, `max_buf_size`, and `max_headers` remain explicit
  H1 parser controls;
- values below Hyper's real `max_buf_size` minimum remain rejected before
  connection construction;
- default aggregate header-byte enforcement remains 32 KiB;
- default `DatePolicy::SystemClock` and Server suppression remain unchanged;
- invalid service Date values never reach the wire merely because ownership
  is external;
- runtime-generated rejections/errors remain final-boundary runtime-owned;
- denylisted response fields remain denylisted;
- framing/hop-by-hop authority remains EggServe-owned;
- `Last-Modified <= Date` remains enforced whenever a Date is present;
- no raw Hyper type becomes public;
- no listener/TLS/H2/H3/static/Python behavior change is authorized;
- no project-specific adapter or feature flag is added;
- Rust 1.89 MSRV remains unchanged.

## API strategy

Prefer additive API on `H1ConnectionPolicy`, whose fields are already
private, rather than adding fields to public exhaustive configuration structs.

Expected shape (names may be refined during implementation):

```rust
let policy = config
    .h1_connection_policy()?
    .with_request_header_bytes_owner(PolicyOwner::External)
    .with_response_metadata_ownership(ResponseMetadataOwnership {
        date: PolicyOwner::External,
        server: PolicyOwner::External,
    });
```

The ordinary `RuntimeConfig::h1_connection_policy()` result must remain
fully EggServe-owned.

Do not encode external ownership through magic numeric sentinels such as zero,
`usize::MAX`, or a "very large" limit.

## Release intent

If Plans 288–289 are implemented with additive public methods/types, preserve
all existing public constants/types, and only widen accepted configuration
values, the expected release shape is a compatible `0.3.x` patch. Plan 290
must verify this rather than assuming it.

If implementation requires changing an existing public field type, adding
fields to an exhaustive public struct, removing a public constant, or
otherwise causing source incompatibility, Plan 290 must select the next
appropriate pre-1.0 minor instead.

Plan 291 is the sole publication/downstream-unblock authority.

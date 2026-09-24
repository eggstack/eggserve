# Plan 278 — Opt-in forward-proxy absolute-form request-target dispatch

## Status

**CLOSED — implementation and qualification passed; registry artifacts are published and proven under Plan 286.**

Plan 277’s separate candidate publication was folded into the next release per maintainer direction; no standalone Plan 277 publication is claimed.

## Purpose

Add the smallest generic EggServe seam required by explicit HTTP forward-proxy
embedders: permit a caller to opt an H1 runtime into validated absolute-form
request targets and deliver those targets to the normal `Service` boundary.

The motivating downstream is EggReplay M013B, but the implementation remains
generic EggServe substrate. No EggReplay routing, recording, CA, MITM,
redaction, proxy-policy, or outbound-client semantics enter EggServe.

Planning baseline: current `main` after Plan 276 implementation / Plan 277
release preparation.

## Current blocker

The direct H1 runtime parses the wire request successfully through Hyper, then
`crates/eggserve-server/src/connection/request.rs::convert_request_head`
unconditionally rejects every HTTP/1 request whose URI has a scheme:

```rust
if !is_h2 && req.uri().scheme_str().is_some() {
    return Err(ServiceError::rejected(
        400,
        "absolute-form request target not allowed",
    ));
}
```

The canonical `eggserve-primitives::RequestTarget::parse` is also deliberately
origin-form-only. Its tests and the shared confinement conformance corpus
currently assert that `http://example.test/a` is rejected.

That default is correct for static/application-server use. It is insufficient
for an explicit forward proxy, where RFC HTTP/1 clients send requests such as:

```text
GET http://example.test/resource?q=1 HTTP/1.1
Host: example.test
```

The proxy service must receive the logical target before it can enforce target
policy, validate Host coherence, strip proxy-only headers, establish an
outbound route, or record semantics.

EggReplay intentionally stopped at this boundary rather than adding a second
HTTP parser/private Hyper server. The upstream correction should preserve that
ownership choice.

## Architecture decision

Do **not** make absolute-form globally accepted.

Add an explicit H1 request-target dispatch policy to the direct runtime. The
default remains origin-form-only.

Suggested public vocabulary:

```rust
pub enum Http1RequestTargetMode {
    OriginOnly,
    OriginOrAbsolute,
}
```

Exact naming may follow existing EggServe conventions, but the semantics must
be explicit and non-boolean at the public boundary.

`RuntimeConfig::default()` and every existing compatibility/static frontend
must use `OriginOnly`.

A direct H1 embedder may opt into `OriginOrAbsolute` through
`RuntimeConfigBuilder` / the direct `RuntimeConfig`.

This is a request-classification/service-dispatch capability, not a forward
proxy implementation.

## Track A — Canonical target-form representation

The canonical service request must preserve enough information for a
downstream proxy to distinguish and validate absolute-form without inspecting
Hyper types.

Add a transport-neutral target-form enum in `eggserve-primitives`, e.g.:

```rust
pub enum RequestTargetForm {
    Origin,
    Absolute,
}
```

Extend the canonical target/head model so a service can inspect, for an
accepted absolute-form request:

- target form;
- semantic raw target representation;
- scheme;
- validated URI authority;
- path;
- query/path-and-query.

Preserve the existing ergonomic methods:

- `raw()`;
- `path()`;
- `query()`;
- `path_and_query()`.

For origin-form their behavior must remain source/behavior compatible.

Preferred representation:

- keep `RequestTarget::parse(...)` origin-form-only, preserving its current
  default/security contract;
- add a separate explicit constructor/classifier for absolute-form based on
  already-parsed components supplied by the HTTP adapter;
- internally `RequestTarget` may gain a private form discriminator and
  component offsets/metadata;
- add `form()` and narrow absolute-form accessors.

Do not add the `http` crate, Hyper, URL libraries, or URI parsing to
`eggserve-primitives` merely for this feature. Hyper already owns the wire URI
parse. The server adapter should validate/project its parsed scheme/authority/
path-and-query into canonical types.

Do not implement a second general URI parser.

## Track B — Preserve semantic raw-target truth

Today `RequestTarget::raw()` is documented as the accepted request target.
For absolute-form, do not silently store only the extracted path while claiming
it is the raw target.

The canonical representation must make this distinction truthful:

- `raw()` should expose the full semantic absolute request target available
  after Hyper parsing/normalization;
- `path()` exposes only the path component;
- `query()` exposes only the query;
- `path_and_query()` exposes the path/query component, not the scheme/authority;
- `form()` identifies origin vs absolute.

If Hyper cannot preserve byte-for-byte spelling for a legal absolute target,
document the same semantic-vs-wire limitation already acknowledged for origin
targets. Do not add a second raw socket parser to recover spelling.

## Track C — Direct runtime opt-in

Add the request-target mode to `eggserve-server::RuntimeConfig` and its
builder.

Required behavior:

### OriginOnly (default)

Unchanged:

- ordinary origin-form reaches the service;
- HTTP/1 absolute-form is rejected before service dispatch;
- CONNECT authority-form retains the existing tunnel path;
- asterisk-form behavior remains unchanged;
- H2/H3 behavior remains unchanged.

### OriginOrAbsolute

For HTTP/1:

- origin-form continues to work exactly as before;
- well-formed absolute-form may reach service dispatch;
- CONNECT authority-form remains the existing tunnel candidate and is not
  reclassified as absolute-form;
- asterisk-form remains unchanged.

The mode must not affect H2 pseudo-field handling or promote any H2 support
tier.

Because `RuntimeConfig` is public/hand-constructible, defaulting and every
construction/projection path must remain safe. This enum has no invalid scalar
state, so it should not be forced into the numeric `runtime_limits` validation
kernel unless implementation proves a real cross-field invariant.

## Track D — Absolute-form validation at the Hyper -> canonical boundary

When `OriginOrAbsolute` is enabled, validate absolute-form before service
dispatch.

Required checks:

1. HTTP version is H1.
2. URI contains a syntactically valid scheme and authority.
3. URI authority passes canonical `Authority` validation.
4. Host header parsing remains duplicate-aware.
5. If Host and URI authority are both present they must be coherent under the
   current canonical authority comparison; mismatches remain 400.
6. Userinfo/ambiguous authority is rejected.
7. A malformed or missing authority fails closed.
8. Full semantic absolute target length is bounded by
   `max_request_target_bytes`, not only the extracted path/query.
9. Aggregate header and body-framing limits remain unchanged.
10. No request-target contents are added to logs/ops events on rejection.

Do not force the absolute target scheme to equal
`ConnectionContext.scheme`. For a forward proxy, the transport from client to
proxy and the logical target URI are distinct concepts. Preserve the target
scheme as canonical request metadata so the downstream service can apply its
own allowed-scheme policy.

The existing H2 scheme-vs-transport validation remains unchanged.

## Track E — Keep static/confinement authority origin-only

`ConfinedPath` and static filesystem resolution remain origin-form security
authorities. Do not teach them absolute URI syntax.

Audit `eggserve-static::StaticService`, which currently resolves
`head.target().path()`.

Even if a caller deliberately combines `OriginOrAbsolute` runtime mode with
`StaticService`, absolute-form must not become an alternate static-file
request surface accidentally.

Add an explicit static-service form check before filesystem resolution:

- origin-form: existing behavior;
- absolute-form: reject with the existing safe unsupported-request response
  convention (do not resolve its extracted path).

The ordinary static CLI/server never enables `OriginOrAbsolute`.

## Track F — Compatibility/core projection

Audit `eggserve-core` configuration bridges after the new direct-server field
is added.

The compatibility/static/multiprotocol surfaces should remain
`OriginOnly` unless there is a separately documented reason to expose the new
mode through a generic Rust embedding path.

Do not add a Python or CLI switch as part of Plan 278.

If a core `RuntimeConfig` projection constructs the direct config
field-by-field, set the new direct field explicitly to the safe default so
future refactors cannot inherit a changed default accidentally.

## Track G — Service-level conformance tests

Add direct `eggserve-server` tests over the real H1 connection driver, not
only unit construction.

Required matrix:

### Default regression

- origin-form GET dispatches;
- absolute-form GET returns the established 400 and service call count stays 0;
- CONNECT authority-form still produces the existing tunnel intent;
- asterisk-form behavior unchanged.

### Opt-in absolute-form

Send raw HTTP/1 requests through a local connection:

- `GET http://example.test/a?b=1 HTTP/1.1` + matching Host reaches service;
- service observes form=absolute, scheme=`http`,
  authority=`example.test`, path=`/a`, query=`b=1`;
- explicit port is preserved;
- IPv4 and bracketed IPv6 authority are represented truthfully;
- empty absolute path canonicalizes according to HTTP/Hyper semantics and is
  covered by a pinned test;
- duplicate/conflicting Host is rejected;
- Host vs URI authority mismatch is rejected;
- invalid/userinfo authority is rejected before service;
- over-limit full absolute target returns 414;
- origin-form still dispatches in the opt-in mode;
- keep-alive can process an allowed absolute request followed by another
  allowed request without state leakage.

### Static safety

- default StaticService behavior unchanged;
- StaticService under an intentionally opt-in runtime rejects absolute-form and
  does not resolve/open a file.

Tests must be deterministic/local and must not use public Internet.

## Track H — Canonical/fuzz corpus updates

The current request-target fuzz/conformance corpus marks absolute-form as
unconditionally rejected because it exercises `RequestTarget::parse`, the
origin-only constructor.

Preserve that assertion for `RequestTarget::parse`.

Add separate tests/property coverage for the explicit absolute-form
constructor/projection. Do not weaken the path/confinement corpus so absolute
URIs start passing `ConfinedPath`.

Add corpus cases for at least:

- normal DNS authority;
- explicit port;
- bracketed IPv6;
- empty path;
- query;
- upper/lowercase scheme;
- Host mismatch;
- userinfo;
- overlong target.

If fuzzing targets the canonical constructor directly, keep allocation/length
bounds explicit.

## Track I — Observability and error contract

No new high-cardinality raw-target logging.

If ops needs visibility, expose only bounded categorical data such as
`target_form=absolute` and rejection category. Reuse current 400/414 response
policy and privacy behavior.

Do not echo absolute URIs, Host values, credentials, or query strings in
operator-facing errors.

## Track J — Documentation

Update relevant current-authority docs:

- direct-server/runtime embedding docs;
- request-target/canonical primitives docs;
- architecture/runtime ownership docs;
- dependency/consumer guidance where applicable;
- AGENTS/developer skill if it enumerates request-target invariants.

Document that:

- EggServe still defaults to application/static origin-form behavior;
- the opt-in mode exists for generic explicit-forward-proxy embedders;
- EggServe does not become a proxy and does not perform outbound routing;
- CONNECT remains a separate tunnel capability;
- static confinement remains origin-form-only.

## Required verification

Use the current repository commands at execution time. At minimum:

```bash
cargo fmt --all -- --check
cargo +1.89 check --workspace --all-targets
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo test -p eggserve-server
cargo test -p eggserve-static
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
bash scripts/install-cargo-tools.sh
bash scripts/check-supply-chain.sh
```

Retain the Plan-276/277 direct-server Tower feature lanes; this change must not
regress the publication-pending adapter surface.

Hosted CI for the implementation SHA must be green before Plan 278 can be
marked implemented/closed locally.

## Acceptance criteria

- [x] Maintainer direction defers Plan 277 publication and its registry
      blockers into the combined Plan 286 release; source work proceeds against
      the qualified workspace candidate.
- [x] origin-form remains the default direct H1 policy.
- [x] existing default absolute-form rejection remains covered.
- [x] direct H1 can explicitly opt into absolute-form dispatch.
- [x] canonical service requests expose target form, scheme, authority,
      path/query without Hyper types.
- [x] `RequestTarget::parse` remains origin-form-only/source-compatible.
- [x] full absolute request-target length is bounded.
- [x] Host/URI authority mismatch fails before service invocation.
- [x] client-proxy transport scheme is not confused with target URI scheme.
- [x] CONNECT authority-form semantics are unchanged.
- [x] H2/H3 semantics/support tiers are unchanged.
- [x] static/confinement paths remain origin-form-only and cannot be widened by
      the runtime opt-in.
- [x] no raw target/credentials/query leakage is introduced in diagnostics.
- [x] no new HTTP parser, outbound proxy/client stack, URL dependency, or
      downstream-project-specific type is added.
- [x] Rust 1.89 and routine platform/feature CI remain green.
- [x] a closure record identifies exact public API additions and test evidence.

## Non-goals

- No forward-proxy routing implementation.
- No CONNECT relay implementation beyond existing generic tunnel capability.
- No TLS interception/CA lifecycle.
- No reverse-proxy behavior.
- No automatic Host rewriting.
- No proxy authentication.
- No H2 Extended CONNECT or H3 changes.
- No Python/CLI product switch.
- No static-file feature expansion.
- No publication in this plan; publication is Plan 279.

## Handoff

After implementation and hosted qualification, Plan 279 owns package-version
selection, crates.io publication, and a registry-only consumer proving the seam
that EggReplay M013B requires.

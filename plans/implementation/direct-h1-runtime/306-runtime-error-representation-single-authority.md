# Direct H1 Runtime Milestone 306 — Runtime error-representation single authority

Status: ready for handoff

Repository baseline: `465b80cdabf90801e6ef929563b12886c509c201`

Source roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md#milestone-12--runtime-error-representation-single-authority`

Long-term requirements:

- `plans/000-long-term-specification.md#2` (single H1 authority, single `Service` contract)
- `plans/001-terminology-and-domain-model.md` (`Response`, `StatusCode`, `ErrorRepresentationPolicy`)
- `plans/003-planning-process.md#7-corrective-passes`

Applicable historical contracts:

- `architecture/crate-topology.md` (Plan 253 connection-overlap ledger; the "no second ... normalization" claim)
- `architecture/error-taxonomy.md`
- `docs/http-primitives.md`
- `release/plan-225-compatibility-facade-closure.md` (Plan 215 facade authority)

Primary class: invariant

## 1. Objective

Remove the second runtime-error representation table.

`crates/eggserve-primitives/src/primitives/canonical/response.rs:436`
`runtime_error_with_policy` documents itself as "the sole runtime-error
representation table for H1, H2, and H3". In fact, two other
implementations exist and are live:

1. `crates/eggserve-server/src/response.rs:58`
   `canonical_error_owned_with_policy` — a hand-rolled error-body builder
   whose 13-of-17 function set is normalized-identical to the core copy.
2. `crates/eggserve-core/src/response.rs:43`
   `canonical_error_owned_with_policy` — a second copy of the same builder.

The core copy is the one that matters: it is the path the CLI and the
Python wheel reach, and it produces error bodies through a different code
path than the documented authority.

This milestone collapses the runtime error-representation to the single
`eggserve-primitives` authority, adds the two files to the topology gate's
facade list so the collapse cannot silently reverse, and proves
byte-identical error output before and after.

## 2. Why this milestone is ready

- The documented authority already exists and already produces the correct
  bodies.
- The equivalence is provable from the current source (see §3), so no
  behavioral design work is required.
- No new public API is needed if the consolidation is done as a delegation
  (the core/server files keep their public shape and forward to
  primitives).
- Hard dependencies (Plan 215 facade authority, Plan 249 single H1
  authority, Plan 253 ledger) are closed.

## 3. Current implementation evidence

### The two implementations

`crates/eggserve-primitives/src/primitives/canonical/response.rs:436`:

```rust
pub fn runtime_error_with_policy(
    status: StatusCode,
    is_head: bool,
    policy: crate::policy::ErrorRepresentationPolicy,
) -> Response {
    let body = status
        .canonical_reason()
        .map(|reason| format!("{} {reason}\n", status.as_u16()))
        .unwrap_or_default();
    // ... content-type under Minimal, Allow under 405, body suppression
    // under Empty / is_head / body-forbidden ...
}
```

`crates/eggserve-core/src/response.rs:43` (and the near-identical
`crates/eggserve-server/src/response.rs:58`):

```rust
pub(crate) fn canonical_error_owned_with_policy(
    status: hyper::StatusCode,
    body: &str,
    is_head: bool,
    policy: ErrorRepresentationPolicy,
) -> Response<BoxBodyInner> {
    let code = u16::from(status);
    let canonical = crate::primitives::StatusCode::new(code)
        .unwrap_or(crate::primitives::StatusCode::INTERNAL_SERVER_ERROR);
    let body_forbidden = !canonical.permits_payload_body();
    let effective_body = if body_forbidden || is_head { "" } else { body };
    // ... content-type under Minimal, Allow under 405 ...
    // normalize_metadata(code, ...) then Response::builder().status(status)
}
```

### The live core call sites

`crates/eggserve-core/src/server/connection/response.rs` and
`crates/eggserve-core/src/server/connection/pipeline.rs` call the parallel
family for the HTTP/2 path:

- `internal_error_with_policy` (`core/src/response.rs:188`)
- `service_unavailable_with_policy` (`core/src/response.rs:209`)
- `bad_request_with_policy` (`core/src/response.rs:144`)
- `expectation_failed_with_policy` (`core/src/response.rs:221`)
- `payload_too_large_with_policy` (`core/src/response.rs:166`)

and call the primitives-backed authority for the rest:

- `runtime_error_with_policy` (`core/src/response.rs:120`, delegating to
  `eggserve_primitives::canonical::runtime_error_with_policy`)
- `not_found` (`core/src/response.rs:236`, test-only under
  `cfg(all(test, feature = "http2"))`)

So the compatibility crate currently carries **two** error-construction
implementations and uses them side by side.

### Equivalence evidence (derived at the baseline)

The bodies are byte-identical, because the hardcoded body strings in the
parallel family are exactly `"{status} {canonical_reason}\n"`:

| Status | Hardcoded body | `canonical_reason` | Same? |
|---|---|---|---|
| 400 | `"400 Bad Request\n"` | `"Bad Request"` | yes |
| 404 | `"404 Not Found\n"` | `"Not Found"` | yes |
| 413 | `"413 Payload Too Large\n"` | `"Payload Too Large"` | yes |
| 417 | `"417 Expectation Failed\n"` | `"Expectation Failed"` | yes |
| 500 | `"500 Internal Server Error\n"` | `"Internal Server Error"` | yes |
| 503 | `"503 Service Unavailable\n"` | `"Service Unavailable"` | yes |

(`crates/eggserve-primitives/src/primitives/canonical/status.rs:168-196`
supplies the reason table; `crates/eggserve-core/src/response.rs:141-236`
supplies the hardcoded bodies.)

The two remaining behavioral differences are in the *inputs*, not the
output:

1. **Status type.** The parallel family takes a `hyper::StatusCode` and
   derives the canonical code with
   `hyper::StatusCode::from_u16(code).unwrap_or(INTERNAL_SERVER_ERROR)`, then
   builds the response with the **original** hyper status. The primitives
   authority takes a canonical `primitives::StatusCode` (100–599 only).
   Every live core call site passes a fixed constant, so no observable
   difference exists today — but the parallel family would emit a
   hyper status outside the canonical range with a canonical-status body if
   a non-constant were ever passed.
2. **Header normalization path.** The parallel family uses
   `normalize_metadata`; the authority uses `normalize_response`. The
   resulting header sets are equivalent for the error cases in scope, but
   the two paths must be proven equal by test, not by inspection.

### Dead code inside the parallel family

`crates/eggserve-core/src/response.rs`:

- `canonical_error` (`:10-11`), `canonical_error_with_policy` (`:31`),
  `canonical_error_owned_with_policy` (`:43`) — the reachable-from-outside
  call graph is only from the http2-gated modules; in a default
  (non-`http2`) build the whole cluster is unreachable, which is why
  `#[allow(dead_code)]` is required on `canonical_error`.
- `bad_request` (`:139`), `payload_too_large` (`:157`),
  `internal_error` (`:179`), `service_unavailable` (`:200`),
  `full_body` (`:239`) — marked `#[allow(dead_code)]`.

`crates/eggserve-server/src/response.rs` carries the same
`#[allow(dead_code)]` cluster at `:25`, `:153`, `:170`, `:179`, `:199`, with
zero external callers for `canonical_error`,
`canonical_error_with_policy`, `canonical_error_owned_with_policy`,
`bad_request`, `payload_too_large`, `internal_error`, and
`service_unavailable`.

### Gate gap

`scripts/check-crate-topology.py:833-839` lists the compatibility facades
that must re-export the direct authority:

```python
facades = {
    "ops/mod.rs": "pub use eggserve_server::ops::*;",
    "server/errors.rs": "pub use eggserve_server::errors::*;",
    "server/response_policy.rs": "pub use eggserve_server::response_policy::*;",
    "policy.rs": "pub use eggserve_primitives::policy::*;",
    "runtime_limits.rs": "pub use eggserve_server::runtime_limits::*;",
}
```

`response.rs` is deliberately absent from this list, so nothing prevents
the two files from diverging further.

### Documentation contradiction

`architecture/crate-topology.md:152-155` states that core's
"request/service/tunnel/canonical types are facades over the direct
authorities (no second envelope, taxonomy, normalization, or state
machine)". The parallel error builder is a second normalization
implementation and contradicts that sentence.

## 4. Invariants that must not regress

- The runtime remains the only owner of error framing, error metadata, and
  the runtime-owned header denylist. Applications never construct runtime
  error responses.
- Error responses keep their current wire bytes. The consolidation must be
  byte-for-byte invisible on every status, policy, HEAD, and
  body-forbidden combination.
- `ErrorRepresentationPolicy::Minimal` still emits `Content-Type:
  text/plain; charset=utf-8`; `Empty` still emits no body; `405` still
  emits `Allow: GET, HEAD`.
- Body-forbidden statuses (1xx, 204, 205, 304) still emit no payload, and
  `304` still keeps its representation `Content-Length`.
- `HEAD` responses still suppress the payload while carrying the correct
  length semantics.
- No error response is synthesized after final response commitment.
- `eggserve-primitives` stays Hyper-free and transport-neutral.
- `eggserve-static` stays the sole static/path/filesystem authority.
- The topology gate keeps passing, and its `--self-test` mutation suite
  keeps passing.
- No user-visible behavior changes. This milestone is a consolidation, not
  a behavior change.

## 5. Scope

### In scope

- Replace the parallel error-body construction in
  `crates/eggserve-core/src/response.rs` and
  `crates/eggserve-server/src/response.rs` with delegation to
  `eggserve_primitives::canonical::runtime_error_with_policy` (or an
  equivalently single-sourced internal helper), keeping the existing
  crate-private call sites working.
- Remove the dead `#[allow(dead_code)]` cluster once its last caller is
  gone, or keep the exact set that remains genuinely reachable — with the
  reason recorded.
- Add `response.rs` (both crates) to the topology gate's facade list, or
  an equivalent gate rule that proves the error-representation is
  single-sourced.
- Byte-level equivalence tests over the full status × policy × HEAD ×
  body-forbidden matrix for the runtime error path.
- A gate self-test mutation proving the new rule fails when a second
  implementation is reintroduced.
- Documentation corrections to `architecture/crate-topology.md` and
  `architecture/error-taxonomy.md`.

### Explicitly out of scope

- Changing any error status, body text, header set, or policy default.
- Adding new error statuses or representations.
- The `core/src/server/connection/response.rs` kernel and the
  `Date`/`Server` provenance divergence — that is Milestone 307.
- The accept-error backoff divergence — Milestone 309.
- The `ResponseProvenance` / `PolicyOwner` ownership model.
- Any change to `eggserve-primitives`' public API unless the
  consolidation genuinely requires it; prefer a crate-private delegation.
- Publication.

## 6. Required production changes

### Crates and ownership

Expected owners:

- `eggserve-primitives`: unchanged public API. If a strictly additive
  helper is needed to express the delegation cleanly, it is additive and
  documented; otherwise nothing changes here.
- `eggserve-server`: `src/response.rs` keeps its crate-private surface but
  stops constructing error bodies independently.
- `eggserve-core`: `src/response.rs` keeps its crate-private surface but
  stops constructing error bodies independently; `pub(crate) mod
  response;` stays crate-private.
- `eggserve-h3`: no change; it already routes through
  `eggserve_server::connection::finalize_canonical_response`.
- `eggserve-python`, `eggserve-bin`, `eggserve-static`: no change.

No new crate.

### Config and policy

None. `ErrorRepresentationPolicy` keeps its current variants and defaults.

### Protocol and compatibility

The wire output must not change. The equivalence test must assert exact
bytes for:

- every status the runtime can emit (400, 404, 405, 413, 417, 500, 503, and
  any other status reachable from the error paths);
- each `ErrorRepresentationPolicy` variant;
- `HEAD` and non-`HEAD`;
- body-forbidden statuses;
- the `Allow` header on 405;
- the `Content-Type` header under `Minimal`.

Compare the pre-change output (captured at the baseline) with the
post-change output. The test must fail if any byte differs.

### Runtime and concurrency

None. No new locks, tasks, channels, or allocation paths on the error
path beyond what the single authority already does.

### Frontend or operator surface

None. The CLI and the Python wheel observe identical bytes.

### Security and confinement

The error path must remain the runtime's responsibility: a service failure
must not be able to inject its own error framing or headers. The
consolidation must not widen what an error response may carry.

### Documentation and static guards

- `architecture/crate-topology.md` — correct the "no second ...
  normalization" claim to state precisely which authorities exist and which
  are facade-only after this milestone.
- `architecture/error-taxonomy.md` — name the single
  runtime-error representation authority.
- `scripts/check-crate-topology.py` — add the facade/authority rule and its
  `--self-test` mutation.

## 7. Ordered work packages

### Work package A — Capture the baseline wire output

Before changing any production code, capture the exact bytes of the
runtime error responses for the full matrix described in §6, from the
current tree, for both the direct server and the core compatibility path.

Store the capture as a fixture the equivalence test compares against.

Acceptance evidence: a committed fixture or an inline expected-bytes table
covering every matrix cell.

### Work package B — Delegate the core error path

Replace the parallel construction in `crates/eggserve-core/src/response.rs`
with delegation to the primitives authority. Keep the crate-private
function names and signatures so the http2-gated call sites in
`crates/eggserve-core/src/server/connection/{response,pipeline}.rs` need no
change, or update them consistently if the signatures must change.

Acceptance evidence: `crates/eggserve-core/src/response.rs` contains no
independent error-body construction; the equivalence test passes.

### Work package C — Delegate the direct error path

Apply the same delegation to `crates/eggserve-server/src/response.rs`, so
the direct and compatibility paths share one construction.

Acceptance evidence: the direct crate's error output is byte-identical to
the baseline capture; the server crate contains no independent error-body
construction.

### Work package D — Remove or justify the dead cluster

Delete the now-unreachable `#[allow(dead_code)]` helpers, or record why
each surviving one is still required. Remove any `#[allow(dead_code)]`
that is no longer needed.

Acceptance evidence: no `#[allow(dead_code)]` remains in either
`response.rs` without a recorded reason; `cargo clippy --workspace
--lib --bins --tests -- -D warnings` is clean.

### Work package E — Gate rule and self-test

Add a topology-gate rule that fails if:

- `crates/eggserve-core/src/response.rs` contains an independent error-body
  construction instead of delegating to the primitives authority; or
- `crates/eggserve-server/src/response.rs` does the same.

Add a matching `--self-test` mutation that reintroduces a parallel
construction in a synthetic fixture tree and asserts the gate fails.

Acceptance evidence: `python3 scripts/check-crate-topology.py` passes;
`python3 scripts/check-crate-topology.py --self-test` passes with the new
mutation included.

### Work package F — Equivalence test

Add a test (crate-level in `eggserve-server`, and a compatibility-level
one in `eggserve-core` where the http2 feature is available) that asserts
the error-response bytes for the full matrix, comparing against the
Work package A capture.

Acceptance evidence: the test fails when the delegation is replaced by a
mutated builder (demonstrate the failure), and passes on the delivered
code.

### Work package G — Documentation

Update `architecture/crate-topology.md` and
`architecture/error-taxonomy.md` to name the single authority and to
describe the remaining classified parallel connection modules accurately
(the Plan 253 ledger accuracy is Milestone 307's scope; this work package
only ensures the error-authority claims are true).

Acceptance evidence: no document claims a second error representation
table exists or that one does not.

## 8. Failure, cancellation, restart, and contention semantics

- No change. Errors are still produced after the service call returns and
  before response commitment; a failure after commitment still closes or
  truncates the connection rather than synthesizing a second response.
- The delegation must not introduce an await point, a lock, or an
  allocation on the hot error path beyond the existing builder.
- Panic containment behavior is unchanged.

## 9. Compatibility and migration

No consumer-visible change: the wire bytes are identical. The only visible
change is that `#[allow(dead_code)]` markers disappear and the crate-private
module surface shrinks, which is not public API.

Semver: no public API change is expected. If the implementation requires an
additive public helper in `eggserve-primitives`, classify it (patch for
additive, per the 291 precedent) and record it.

## 10. Required tests

### Focused unit tests

- The full status × policy × HEAD × body-forbidden equivalence matrix.
- `Allow` header on 405 under each policy.
- `Content-Type` under `Minimal`; absence under `Empty`.
- Body-forbidden statuses produce no payload.
- `304` keeps its representation `Content-Length`.

### Integration tests

- Direct server: an HTTP/1.1 request that triggers each error status
  returns the exact expected bytes.
- Core compatibility path under the `http2` feature: the same matrix.
- Python facade: an error response body and headers match the Rust
  expectation (the Python suite already asserts status/headers; add a body
  byte assertion if it does not exist).

### Restart and recovery tests

None required; no lifecycle change.

### Contention and cancellation tests

None required.

### Security and negative tests

- A service error cannot inject framing, routing, or authentication
  headers into the runtime error response.
- The error response never carries a body for body-forbidden statuses.
- No `Content-Length`/`Transfer-Encoding` conflict in the error response.

### Migration and compatibility tests

- The pre-change and post-change byte captures are identical for every
  matrix cell.

## 11. Required verification commands

```bash
cargo test -p eggserve-server --lib
cargo test -p eggserve-core --lib
cargo test -p eggserve-core --features http2,tls --lib
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo clippy -p eggserve-core --features http2,tls --lib --tests -- -D warnings
python3 scripts/check-crate-topology.py
python3 scripts/check-crate-topology.py --self-test
python3 scripts/verify-conformance-matrix.py
cargo fmt --all -- --check
./scripts/verify.sh fast
```

## 12. Documentation updates

- `architecture/crate-topology.md` — the facade/authority statement for the
  error path.
- `architecture/error-taxonomy.md` — the single named authority.
- `plans/subsystems/direct-h1-runtime-roadmap.md` — Milestone 12 status.
- `plans/registry.md` — the Milestone 12 row.
- `plans/closure/direct-h1-runtime/306-runtime-error-representation-single-authority.md`
  (new).

## 13. Acceptance criteria

- `crates/eggserve-core/src/response.rs` and
  `crates/eggserve-server/src/response.rs` contain no independent runtime
  error-body construction; the error representation is produced once, in
  `eggserve-primitives`.
- The equivalence test proves the error-response bytes are identical to the
  baseline capture for every status, policy, HEAD, and body-forbidden cell.
- The topology gate fails when a parallel error construction is
  reintroduced, and its `--self-test` mutation proves the rule is wired.
- No `#[allow(dead_code)]` remains in either `response.rs` without a
  recorded reason.
- `architecture/crate-topology.md` and `architecture/error-taxonomy.md`
  name one authority and no longer claim a second one exists.
- No user-visible byte changes; no public API break; no crate ownership
  change.

## 14. Stop conditions

Stop and report rather than improvise if:

- the equivalence test shows any byte difference for any matrix cell and
  the difference cannot be shown to be an intentional, separately approved
  behavior change;
- the consolidation would require changing an error status, body text,
  header set, or policy default;
- `eggserve-primitives` would need to become Hyper-aware;
- the dead-cluster removal would require touching a live call site outside
  the error path;
- the remaining classified parallel connection modules turn out to be
  entangled with this work — that entanglement belongs to Milestone 307.

## 15. Closure evidence required

Create `plans/closure/direct-h1-runtime/306-runtime-error-representation-single-authority.md`
containing:

- the baseline byte capture and the post-change capture, with a diff
  proving they are identical;
- the equivalence test output over the full matrix;
- the gate rule text and the `--self-test` mutation result;
- the list of removed helpers and the reasoning for anything retained;
- confirmation that `eggserve-primitives` public API is unchanged (or the
  additive change and its semver classification);
- the verification command/outcome table;
- residual findings and the roadmap/registry disposition.

## 16. Handoff notes

This is a consolidation with a hard "no observable change" requirement. The
equivalence evidence is the deliverable, not the refactor. If the two
implementations turn out to differ anywhere, that difference is a finding
for Milestone 307 or a new corrective plan — not a license to pick one.

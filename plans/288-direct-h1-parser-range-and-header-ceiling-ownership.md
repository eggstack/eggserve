# Plan 288 — Direct H1 parser range and aggregate-header ownership

Status: **PLANNED**.

Program:
`plans/288-291-direct-h1-boundary-ownership-followup-program.md`.

Planning baseline:
`2d4bae12f8cb87c01a8496d765d18b1380871e72`.

## Goal

Remove arbitrary EggServe-only upper bounds from the H1 parser buffer and
header-count controls while preserving safe defaults, and add an explicit
direct-H1 embedding seam for callers that own aggregate post-parse
request-header byte policy.

This is not a request to disable parser defenses.

## Findings

### Hyper-native parser controls

The workspace currently resolves `hyper 1.11.1`.

EggServe currently validates:

- `max_buf_size <= 4 MiB`;
- `max_headers <= 10_000`.

Those upper bounds are EggServe policy, not requirements exposed by Hyper's H1
builder:

- Hyper documents `max_buf_size` as requiring a minimum of 8192;
- Hyper's `max_headers` takes `usize` and documents the memory/performance
  cost of setting it, but no equivalent 10,000 maximum.

EggServe should continue pinning both values explicitly so a Hyper upgrade
cannot silently change defaults; explicit caller-selected values need not be
artificially capped at the current conservative guidance values.

### Aggregate header bytes are different

`max_header_bytes` is enforced after parsing by
`convert_request_head`. It is an EggServe semantic/application admission
ceiling, not a Hyper parser bound.

Simply raising its 1 MiB maximum does not solve embedding ownership. A caller
that already owns aggregate-header policy needs EggServe not to reject first.

## Track A — relax only arbitrary parser upper bounds

In `crates/eggserve-server/src/runtime_limits.rs`:

- retain `MIN_MAX_BUF_SIZE = 8192`;
- reject `max_buf_size < MIN_MAX_BUF_SIZE` exactly as today;
- stop rejecting `max_buf_size` merely because it is greater than the
  current 4 MiB EggServe guidance value;
- retain `max_headers > 0`;
- stop rejecting `max_headers` merely because it exceeds 10,000;
- do not add a replacement arbitrary upper cap.

If implementation identifies a real underlying platform/allocation bound that
must be rejected before Hyper, derive it from the actual Rust/Hyper
representation and document/prove it. Do not substitute another policy number.

Defaults remain:

- `max_buf_size = 64 KiB`;
- `max_headers = 100`.

Large configured values must be documented as increasing per-connection or
per-request memory exposure.

## Track B — public constant compatibility

`runtime_limits` is public. Do not remove existing public
`MAX_MAX_BUF_SIZE` or `MAX_MAX_HEADERS` constants in this patch.

Reconcile their documentation so they are no longer falsely described as the
runtime validator's absolute maximum. Prefer treating them as retained
conservative/legacy guidance constants.

If clearer names such as `RECOMMENDED_MAX_BUF_SIZE` /
`RECOMMENDED_MAX_HEADERS` are useful, add aliases rather than deleting old
symbols. Whether to deprecate the legacy names is a Plan 290 compatibility
decision; avoid injecting avoidable downstream warnings in the implementation
pass.

`MAX_MAX_HEADER_BYTES` remains an active hard bound for the EggServe-owned
aggregate-header policy and is not part of this relaxation.

## Track C — explicit aggregate-header ownership on H1ConnectionPolicy

Do not add another field to public `H1PolicyOwnership`; that struct is
exhaustively constructible and changing its field set would create avoidable
source incompatibility.

Instead add an additive direct-policy override, conceptually:

```rust
impl H1ConnectionPolicy {
    pub fn with_request_header_bytes_owner(
        self,
        owner: PolicyOwner,
    ) -> Self;
}
```

Internal `H1ConnectionPolicy` state defaults to
`PolicyOwner::EggServe`.

The ordinary:

```rust
RuntimeConfig::h1_connection_policy()
```

must therefore preserve today's behavior.

No new `RuntimeConfig` field is required.

## Track D — make request conversion ownership-aware

Change the private request conversion boundary so aggregate-header enforcement
is optional by ownership, mirroring the existing request-target pattern.

Preferred internal shape:

```rust
convert_request_head(
    req,
    max_target_bytes: Option<usize>,
    target_mode,
    max_header_bytes: Option<usize>,
    ...
)
```

Behavior:

- EggServe owner -> `Some(config.max_header_bytes)`, existing 431 + counter +
  event behavior unchanged;
- External owner -> `None`, skip only the aggregate name+value byte ceiling;
- parser `max_buf_size`, `max_headers`, Host/authority validation, framing,
  target-form validation, and all other canonical conversion remain active.

Do not fake external ownership by setting `max_header_bytes` to a huge value.

## Track E — validation model

`RuntimeConfig::validate()` may continue validating the stored
`max_header_bytes` placeholder against EggServe's normal range even when a
later direct policy marks the ceiling External. An embedder can retain the
default 32 KiB placeholder; the projected H1 policy determines whether it is
executed.

That keeps public `RuntimeConfig` safe and avoids making its validation
conditional on embedding-only state.

The relaxed `max_buf_size` / `max_headers` validation is shared with the
compatibility facade. This is acceptable because it expands explicitly chosen
configuration without changing defaults. H2 has its own
`Http2Config.max_header_list_size`; do not couple this change to H2.

## Track F — tests

Add focused server tests for:

1. default validation unchanged;
2. `max_buf_size = 4 MiB + 1` is accepted;
3. `max_headers = 10_001` is accepted;
4. values below 8192 still reject `max_buf_size`;
5. zero still rejects `max_headers`;
6. default H1 policy still rejects aggregate headers over
   `max_header_bytes` with 431 and existing observability;
7. an H1 policy with external aggregate-header ownership delivers a request
   exceeding the stored `max_header_bytes` value to the service;
8. external aggregate ownership does not disable parser header count/buffer
   enforcement;
9. target ceiling ownership remains independent;
10. high-level `Server` behavior remains EggServe-owned by default.

Add a source/behavior guard ensuring future refactors cannot silently make the
external aggregate-header switch disable `max_headers` or `max_buf_size`.

## Track G — compatibility facade and docs

Update direct runtime docs and the relevant compatibility limit docs:

- defaults remain the recommended hardened baseline;
- parser maxima above the old guidance are explicit operator resource choices;
- aggregate-header External ownership is a direct-H1 embedding seam;
- it transfers policy responsibility; it is not a "disable all header
  defenses" switch.

Do not expose this as a CLI/Python convenience flag in this plan.

## Verification

At minimum:

```bash
cargo fmt --all -- --check
cargo test -p eggserve-server
cargo test -p eggserve-core
cargo check -p eggserve-server --no-default-features
cargo check -p eggserve-server --no-default-features --features http-interop
cargo check -p eggserve-server --no-default-features --features tower
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
```

Run the repository's ordinary Rust/supply-chain CI before closing.

## Acceptance criteria

- [ ] 4 MiB and 10,000 are no longer hard parser validation maxima.
- [ ] Hyper's real 8192 minimum remains fail-closed.
- [ ] zero `max_headers` remains fail-closed.
- [ ] secure defaults do not change.
- [ ] existing public hard-max constants are not removed.
- [ ] direct H1 can explicitly externalize only aggregate header-byte policy.
- [ ] external aggregate ownership does not disable parser limits.
- [ ] default/high-level Server behavior remains unchanged.
- [ ] H2/H3 behavior is unchanged.
- [ ] focused tests and normal CI pass.

## Non-goals

- No unlimited/default parser policy.
- No H2/H3 limit redesign.
- No request-target policy change.
- No body-limit change.
- No project-specific compatibility mode.

# Plan 271 — Optional unlimited total connection lifetime with timeout-policy parity

## Purpose

Add an explicit, source-compatible way to disable EggServe's hard total
connection lifetime while keeping all existing bounded header, handler, body,
idle, response-write, admission, and shutdown protections.

This closes the second downstream embedding gap identified after the 0.2.0
release: applications with long-lived healthy pooled HTTP/1 connections cannot
currently preserve that behavior because `connection_total_timeout` is
mandatory, nonzero, and defaults to 60 seconds.

Planning baseline:

```text
100b33c fix: exclude evidence MANIFEST from twine upload set
```

This plan may implement independently of Plan 270, but both are required by the
downstream-embedding release qualification in Plan 272.

## Current behavior

The canonical shared runtime kernel currently defines:

```rust
pub const DEFAULT_CONNECTION_TOTAL_TIMEOUT: Duration = Duration::from_secs(60);
pub connection_total_timeout: Duration;
```

Validation rejects zero and requires:

- `header_read_timeout <= connection_total_timeout`;
- `handler_timeout <= connection_total_timeout`;
- `body_read_timeout <= connection_total_timeout`.

The H1 driver computes one deadline from connection start and treats expiry as
a hard ceiling across all requests on a keep-alive connection.

This is a valid hardened default and must remain the default. The gap is the
absence of an explicit opt-out for embedders that intentionally rely on
long-lived connections while retaining the independent idle/request/write
bounds.

## Compatibility constraint

Do not change the public
`RuntimeConfig.connection_total_timeout: Duration` field type in the 0.2.x
patch line.

Changing it to `Option<Duration>` or adding a new mandatory public struct field
would break callers that construct the public config directly.

Use an additive sentinel/policy on the existing field.

## Required API

Adopt the following source-compatible contract unless implementation evidence
shows a materially better equivalent:

- `Duration::ZERO` means **no total connection lifetime ceiling**;
- nonzero durations retain their exact current meaning;
- the default remains 60 seconds;
- `RuntimeConfigBuilder::connection_total_timeout(Duration::ZERO)` is valid;
- add a discoverable convenience such as
  `disable_connection_total_timeout()` or
  `unlimited_connection_lifetime()` that sets the same canonical value.

Document zero explicitly as disabled/unlimited. Do not use an undocumented
"huge duration" convention.

Do not add another boolean public field solely to preserve the existing
`Duration` field. A zero sentinel is preferable because it keeps struct
construction source-compatible and mirrors other EggServe zero-as-policy
values where appropriate.

## Shared validation authority

Update `eggserve-server::runtime_limits::SharedRuntimeValues::validate` as the
single authority.

When `connection_total_timeout == Duration::ZERO`:

- do not emit the current `> 0` violation;
- do not apply the three cross-field `<= connection_total_timeout`
  constraints;
- continue validating header, handler, and body timeouts themselves as nonzero
  bounded values;
- continue validating every unrelated admission/parser/idle/write/shutdown
  limit exactly as today.

When the total timeout is nonzero, preserve every current cross-field
constraint and diagnostic.

Update compatibility-core construction/projection to share exactly the same
semantics. Do not create a direct-only validation fork.

## H1 driver semantics

Make the H1 driver represent the total deadline as truly optional.

When total lifetime is disabled:

- do not create a synthetic one-year/far-future deadline;
- do not increment `connection_total_timeouts`;
- do not emit `ConnectionTotalTimeout`;
- do not cancel requests with `ConnectionTimeout` solely because of total
  connection age;
- allow a healthy active/keep-alive connection to live until another explicit
  bound or shutdown closes it.

Independent protections remain active:

- header-read timeout;
- handler timeout;
- request-body timeout;
- keep-alive idle timeout;
- response-write no-progress timeout;
- request-count limit when configured;
- admission limits;
- server shutdown/drain.

Refactor the driver/select deadline calculation so the absence of a total
deadline uses a pending/optional timer rather than a fake far-future deadline.

If the existing `far_future()` helper becomes unused after the correct
optional-deadline implementation, remove it.

## Tunnel/drain semantics

Audit the branches that currently reuse the total lifetime as an outer bound
for accepted tunnel tasks.

With total lifetime disabled:

- active tunnels may remain live without an age ceiling;
- server shutdown must still cancel/drain them within the configured shutdown
  policy;
- response-write/request/body bounds continue where applicable;
- no detached tunnel task may survive direct server completion.

If `ConnectionActivity::drain_tunnels` currently requires an absolute
deadline, add an internal optional/unbounded drain variant rather than
manufacturing a fake deadline.

Do not turn unlimited connection lifetime into unlimited shutdown time.

## H2/H3 and compatibility parity

Because the validation kernel is shared, allowing zero must not create a
protocol-specific trap.

Audit every production use of `connection_total_timeout` across:

- direct H1;
- compatibility H1 projection;
- HTTP/2 when the feature is enabled;
- HTTP/3 when the feature is enabled;
- tunnel/upgrade lifetime handling;
- Python/CLI config translation if those surfaces expose the field.

Any path that can receive zero must interpret it as disabled/unlimited, never
as "expire immediately".

If H2/H3 cannot safely support zero in the same pass, fail closed in those
protocol-specific constructors with an explicit diagnostic while keeping the
direct H1 opt-out usable; however, shared validation must not silently admit
zero into a driver that interprets it incorrectly. Prefer full parity because
the field is shared runtime policy.

Do not change protocol support tiers.

## Tests

Add deterministic tests covering:

1. **Default unchanged**
   - default remains exactly 60 seconds.

2. **Zero accepted**
   - builder and direct validated struct accept zero;
   - convenience builder method produces the same canonical state.

3. **Cross-field semantics**
   - handler/body/header values greater than 60 seconds are valid when total is
     disabled, subject to their own bounds;
   - the existing `<= total` errors still fire for a nonzero smaller total.

4. **H1 lifetime disabled**
   - configure a very short comparison total timeout in one server and zero in
     another;
   - the bounded server closes at the total deadline;
   - the zero server remains usable across that same elapsed interval and can
     serve a later request on the same keep-alive connection.

5. **Idle bound still active**
   - zero total lifetime does not disable keep-alive idle closure.

6. **Handler/body/write bounds still active**
   - representative existing timeout regressions remain green with total
     lifetime disabled.

7. **Shutdown still bounded**
   - zero total lifetime does not prevent direct server shutdown/drain.

8. **Tunnel lifetime**
   - a tunnel can outlive the ordinary total-lifetime comparison interval when
     disabled;
   - shutdown still terminates it under the normal drain policy.

9. **Feature parity**
   - H2/H3 feature tests prove zero is either correctly unlimited or
     explicitly rejected before execution; never immediate expiry.

Avoid multi-second sleeps; use millisecond-scale deterministic timeouts or
Tokio paused time where the existing harness supports it.

## Documentation

Update:

- direct `RuntimeConfig` rustdoc;
- compatibility `RuntimeConfigBuilder` rustdoc;
- runtime-limit architecture docs;
- public API boundary/migration guidance;
- any configuration table that currently says
  `connection_total_timeout > 0`.

State clearly:

- default remains 60 seconds;
- zero is an explicit expert opt-out;
- disabling the total lifetime does not disable idle/request/write/shutdown
  bounds;
- static/default frontends retain their current default and need not expose a
  CLI switch unless a separate product plan requires it.

## Verification

Run at minimum:

```sh
cargo fmt --all -- --check
cargo clippy -p eggserve-server --all-targets -- -D warnings
cargo test -p eggserve-server
cargo test -p eggserve-core
cargo test --workspace
cargo +1.89 check --workspace --all-targets
cargo +1.89 check --workspace --all-targets --features http2,tls
cargo +1.89 check --workspace --all-targets --features http3,tls
python3 scripts/check-crate-topology.py
python3 scripts/verify-conformance-matrix.py
```

Run routine CI on the exact implementation SHA.

## Acceptance criteria

- [ ] Default total connection lifetime remains exactly 60 seconds.
- [ ] Zero is a documented supported unlimited/disabled total-lifetime value.
- [ ] Existing public `Duration` field and existing builder setter remain
      source-compatible.
- [ ] A discoverable builder convenience exists for disabling the total
      lifetime.
- [ ] Shared validation skips total-lifetime cross-field comparisons only when
      the total is disabled.
- [ ] Nonzero total-lifetime validation/diagnostics are unchanged.
- [ ] Direct H1 uses no fake far-future deadline when total lifetime is
      disabled.
- [ ] Long-lived healthy H1 keep-alive survives past a comparison total timeout
      when disabled.
- [ ] Idle, handler, body, response-write, admission, request-count, and
      shutdown bounds remain active.
- [ ] Tunnel lifetime and shutdown semantics are correct without a total
      deadline.
- [ ] H2/H3/compatibility paths cannot interpret zero as immediate expiry.
- [ ] No protocol support tier or default frontend behavior changes.
- [ ] Focused/full tests, feature checks, topology/conformance checks, and
      routine CI pass.

## Non-goals

- No default change from 60 seconds.
- No CLI/Python knob unless already mechanically exposed by runtime config.
- No generic timeout-policy enum that breaks the public struct.
- No removal of defense-in-depth timeouts.
- No connection pool, retry, or client behavior.
- No protocol-tier promotion.
- No application-specific keep-alive policy.

The goal is one explicit expert opt-out while preserving every other hardening
boundary.

## Implementation status

Implemented on the 0.2.1 release candidate. Shared validation treats zero as
disabled and retains the nonzero cross-field checks. Direct and compatibility
drivers use optional deadlines, without a synthetic far-future total deadline;
tunnel drains stay bounded during shutdown. The combined leaf-only fixture
demonstrates a second request on the same keep-alive connection after the
comparison deadline, and configuration tests assert the unchanged 60-second
default and zero acceptance. Full feature and regression evidence is recorded
in `release/plan-272-downstream-embedding-qualification-closure.md`.

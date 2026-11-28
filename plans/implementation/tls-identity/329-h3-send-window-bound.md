# TLS Identity Milestone 329 - H3 send-window bound

Status: implemented

Repository baseline: `e9c98a0` (`main` after PR #6; clean tree)

Source roadmap:

- `plans/subsystems/tls-identity-roadmap.md#7` (Milestone 329)

Long-term requirements:

- `plans/000-long-term-specification.md#2` (neutral `eggnet-tls` ownership)
- `plans/000-long-term-specification.md#3` (invariants)
- `plans/000-long-term-specification.md#4` (TLS identity remains experimental)

Applicable ADRs:

- none

Primary class: invariant

## 1. Objective

Close the remaining low finding from milestone 327
(`plans/closure/tls-identity/327-tls-substrate-corrective.md` section 13):
`Http3Config::validate` rejects only zero for `send_window`, even though
quinn documents that field as the per-connection retained-write memory cap, and it
rejects only zero for `max_send_buf_size`, while the sibling receive windows and the
HTTP/2 send-buffer ceiling are bounded above.

## 2. Why this milestone is ready

No dependency blocks it: the fields, the validator, and the unit-test module already
exist in `eggserve-h3`, and the only open decision is the ceiling. The in-repo
precedent settles it: `http2.max_send_buf_size` is a local buffer ceiling bounded to
`1..=u32::MAX`, and the QUIC VarInt ceiling (`2^62-1`) applies to fields that are
encoded on the wire. `send_window` is not a wire field, so the VarInt ceiling would
document the wrong invariant. No public type, dependency, or wire change is needed,
and H3 remains experimental.

## 3. Current implementation evidence

- `crates/eggserve-h3/src/config.rs:108-113` rejects `0` for both fields and names
  each one; there is no upper bound on either.
- `crates/eggserve-h3/src/config.rs:90` and `:96` bound the two receive windows to
  `QUIC_VARINT_MAX`; `:114-120` bounds `max_idle_timeout` the same way.
- `crates/eggserve-h3/src/config.rs:31` declares `send_window: u64`; `:39` declares
  `max_send_buf_size: usize`; defaults are 4 MiB and 256 KiB at `:57` and `:61`.
- `crates/eggserve-h3/src/quic.rs:59` passes `send_window` straight into
  `quinn::TransportConfig::send_window` with no intermediate bound.
- `quinn-proto` 0.11.19 `src/config/transport.rs:130-140` documents `send_window` as
  "Maximum number of bytes retained from application writes across all streams of
  a connection" and tells endpoints to set it "low enough to avoid memory exhaustion
  if every connection uses the entire window".
- `crates/eggserve-h3/src/response.rs:299-302` uses `max_send_buf_size` only as a
  chunk size clamped by the shared `stream_chunk_size`, so it cannot allocate above
  that shared ceiling today; its missing upper bound is preventive, not a live
  exposure.
- `crates/eggserve-core/src/server/config/http2.rs:104` bounds the HTTP/2
  `max_send_buf_size` to `1..=u32::MAX` - the in-repo precedent for a local buffer
  ceiling, as distinct from a VarInt wire value.
- `architecture/http3.md:103` describes `send_window` as "QUIC outbound flow-control
  budget", which reads as a congestion-control knob rather than the retained-write
  memory cap quinn implements.

## 4. Invariants that must not regress

- QUIC VarInt wire fields stay bounded by `QUIC_VARINT_MAX` (`2^62-1`): both receive
  windows and `max_idle_timeout`.
- Defaults are unchanged: `send_window` 4 MiB, `max_send_buf_size` 256 KiB.
- Zero stays rejected for both fields, each naming itself.
- No application 0-RTT, TLS 1.3-only QUIC, no migration, no stateless-retry change.
- H3 stays experimental; no CLI or Python knob is added.
- No key or certificate material appears in any error, Debug, or event.

## 5. Scope

### In scope

- An upper bound for `send_window` and `max_send_buf_size` at `u32::MAX`.
- Field-named diagnostics and field documentation stating the ceiling and semantics.
- Unit tests at, and above, the ceiling for both fields.
- `architecture/http3.md` semantics and ceiling, `CHANGELOG.md`, roadmap and
  registry records, and the 327 section 13 resolution pointer.

### Explicitly out of scope

- Treating `send_window` as a wire-encoded VarInt bound.
- Any new CLI or Python configuration surface.
- Changing quinn transport policy, congestion control, or any default.
- Coupling `send_window` to `stream_receive_window` or adding cross-field ratios.

## 6. Required production changes

`eggserve-h3::config::Http3Config::validate`:

- Reject `send_window == 0 || send_window > u64::from(u32::MAX)` with
  `http3.send_window` and the accepted range in the message.
- Reject `max_send_buf_size == 0 || max_send_buf_size > u32::MAX as usize` with
  `http3.max_send_buf_size` and the accepted range in the message (the cast matches
  the existing HTTP/2 check and is lossless on the 32- and 64-bit targets the
  workspace supports).
- Document on both fields that they are local memory ceilings, not wire values, and
  what quinn does with each.

No new error type: the existing `ServerError::Config` `invalid(field, detail)` closure
carries the diagnostic.

## 7. Ordered work packages

### Work package A - Bounds and diagnostics

Intent: no configuration can disable quinn's per-connection retained-write cap or
exceed the local buffer ceiling the HTTP/2 twin already enforces.

Required changes: the two range checks in `validate` and the field doc comments.

Acceptance evidence: an out-of-range value is rejected with the field named and the
range stated; the ceiling value is accepted.

### Work package B - Unit tests

Intent: the pre-fix behavior fails deterministically for both fields.

Required changes: `crates/eggserve-h3/src/config.rs` tests covering
`send_window` and `max_send_buf_size` above the ceiling and at the ceiling, plus a
default-config validity assertion.

Acceptance evidence: before the fix, the above-ceiling cases validate successfully.

### Work package C - Documentation and records

Intent: the documented meaning and bound match the enforced behavior, and 327's
open finding is closed by this milestone's closure record.

Required changes: `architecture/http3.md` rows for both fields,
`CHANGELOG.md`, the roadmap and registry entries, and the 327 section 13 pointer.

## 8. Failure, cancellation, restart, and contention semantics

Validation is pure and runs before any endpoint exists: `RuntimeConfig::validate`
calls `Http3Config::validate` when H3 is enabled, `ServerBuilder::build` invokes it,
and `Server::start` re-runs it at `crates/eggserve-core/src/server/mod.rs:601` before
the H3 endpoint is assembled at `:700`. A rejected configuration therefore fails
startup with `ServerError::Config` before the UDP socket is bound, with no partial
endpoint and no state to unwind. There is no lock, task, or reload interaction on
this path.

## 9. Compatibility and migration

- A value above `u32::MAX` (4 GiB - 1) is now rejected where it was previously accepted. `Http3Config`
  is reachable only from Rust, H3 is experimental, and the defaults are unchanged.
- Field types, the error type, and the public surface are unchanged; no variant is
  added.
- A caller that deliberately set an unbounded send window must clamp to
  `u32::MAX` or keep the 4 MiB default; neither the CLI nor Python exposes H3
  transport knobs.

## 10. Required tests

### Unit tests (`eggserve-h3`)

- `send_window` one byte above the ceiling is rejected and names
  `http3.send_window`; the ceiling value is accepted.
- `max_send_buf_size` one above the ceiling is rejected and names
  `http3.max_send_buf_size`; the ceiling value is accepted.
- The default configuration validates, and the existing
  `zero_send_buf_size_names_its_own_field`, `quic_window_bounds_are_validated`, and
  `quic_idle_timeout_bound_is_validated` stay green.

### Integration and feature shape

- `cargo test -p eggserve-core --features http3,tls --lib` stays green (the config
  projection is unchanged).
- `cargo test -p eggserve-core --features http3,tls --test http3_runtime` stays green
  (the endpoint path consumes the validated config).

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo test -p eggserve-h3
cargo test -p eggserve-core --features http3,tls --lib
cargo test -p eggserve-core --features http3,tls --test http3_runtime
cargo clippy -p eggserve-h3 --all-targets -- -D warnings
cargo clippy -p eggserve-core --features http3,tls --lib --tests -- -D warnings
cargo check --workspace --all-targets --features http3,tls
cargo +1.89 check --workspace --all-targets --features http3,tls
git diff --check
python3 scripts/check-crate-topology.py
```

## 12. Documentation updates

- `architecture/http3.md`: `send_window` describes the retained-write memory cap
  rather than a flow-control budget; both rows state the `u32::MAX` ceiling and
  distinguish it from the VarInt receive-window bound.
- `CHANGELOG.md`: an Unreleased bullet recording the H3 buffer-ceiling validation
  and that it closes the 327 follow-up finding.
- `plans/subsystems/tls-identity-roadmap.md` and `plans/registry.md`: milestone 329.
- `plans/closure/tls-identity/327-tls-substrate-corrective.md` section 13: a pointer
  that milestone 329 owns the finding, replaced by "resolved by 329" when its closure
  record lands.

## 13. Acceptance criteria

1. `send_window > u32::MAX` is rejected with `http3.send_window` named and the
   accepted range stated; `send_window == u32::MAX` validates.
2. `max_send_buf_size` above `u32::MAX` is rejected with
   `http3.max_send_buf_size` named; the ceiling value validates.
3. Zero remains rejected for both fields, each named correctly.
4. The default configuration validates, and the defaults are unchanged.
5. No CLI or Python surface changes, and H3 remains experimental.
6. The 327 closure's section 13 finding is marked resolved by 329's closure record.

## 14. Stop conditions

Stop and report if the bound requires changing a field type, adding a public error
type or configuration knob, changing a default, or if any in-tree or documented
configuration legitimately needs a value above `u32::MAX`.

## 15. Closure evidence required

The exact commands and outcomes, the test names, the chosen ceiling with its
rationale (local buffer ceiling versus VarInt wire value), the documentation
updates, and the 327 section 13 annotation replacing the ownership pointer with the
resolution.

## 16. Handoff notes

The `max_send_buf_size` clamp through `stream_chunk_size` means today's exposure is
limited; the bound is preventive and keeps the two local buffer ceilings consistent
with the HTTP/2 twin. The QUIC wire fields keep their VarInt ceiling, so the two
classes of bound must not be merged.

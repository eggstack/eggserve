# TLS Identity Milestone 329 - Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tls-identity/329-h3-send-window-bound.md`

Source subsystem roadmap:

- `plans/subsystems/tls-identity-roadmap.md#7` (Milestone 329)

Repository baseline reviewed: `e9c98a0` (`main` after PR #6)

Implementation commits or pull requests:

- not yet committed (working tree; commit is a user-owned action)

## 1. Executive finding

`Http3Config::validate` now bounds both H3 local buffer ceilings at
`u32::MAX`. `send_window` — quinn's per-connection retained-write memory cap —
can no longer be raised arbitrarily, and `max_send_buf_size` now matches the
HTTP/2 twin's `1..=u32::MAX` bound. The QUIC VarInt ceiling (`2^62-1`) still
applies only to the wire-encoded receive windows and idle timeout, so the two
classes of bound are not merged. This closes the low finding recorded in the
327 closure (section 13).

## 2. Requirement-to-evidence matrix

| Acceptance criterion | Evidence | Result | Notes |
|---|---|---|---|
| `send_window > u32::MAX` is rejected naming `http3.send_window` and the range | `config::tests::send_window_bound_is_validated` | pass | Exercises `u32::MAX + 1`; the message contains `1 and u32::MAX`. |
| `send_window == u32::MAX` validates | same test | pass | |
| `max_send_buf_size` above `u32::MAX` is rejected naming the field | `config::tests::max_send_buf_size_bound_is_validated` | pass | Compiled only on 64-bit targets, where such a `usize` exists. |
| The ceiling `max_send_buf_size` validates | same test | pass | |
| Zero stays rejected for both fields, each named | existing `zero_send_buf_size_names_its_own_field` | pass | The two-arm split from 327 is preserved; the range check keeps them separate. |
| The default configuration validates and defaults are unchanged | `config::tests::default_config_is_valid`; `config.rs:68` 4 MiB, `:72` 256 KiB | pass | |
| No CLI or Python surface change; H3 stays experimental | only `crates/eggserve-h3` and documentation changed | pass | No manifest, frontend, or tier change. |
| 327 section 13 finding marked resolved | 327 closure section 13 | pass | Pointer replaced by the resolution. |

## 3. Production implementation evidence

- `crates/eggserve-h3/src/config.rs:119`: `send_window` rejects zero and any
  value above `u64::from(u32::MAX)`, naming `http3.send_window`.
- `crates/eggserve-h3/src/config.rs:125`: `max_send_buf_size` rejects zero and
  any value above `u32::MAX as usize`, matching
  `crates/eggserve-core/src/server/config/http2.rs:104`.
- `crates/eggserve-h3/src/config.rs:30-37` and `:44-50`: field documentation
  states that both are local memory ceilings rather than wire values, what
  quinn does with `send_window`, and the `stream_chunk_size` clamp on
  `max_send_buf_size`.
- Tests: `send_window_bound_is_validated` (`:223`),
  `max_send_buf_size_bound_is_validated` (`:242`), `default_config_is_valid`
  (`:259`).
- `architecture/http3.md:103`, `:107`, and the paragraph at `:110-117`.
- No field type, default, error type, or wire behavior changed; `quic.rs` and
  `response.rs` are untouched.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all
cargo fmt --all -- --check
cargo test -p eggserve-h3 -- --test-threads=1
cargo clippy -p eggserve-h3 --all-targets -- -D warnings
cargo test -p eggserve-core --features http3,tls --lib -- --test-threads=1
cargo clippy -p eggserve-core --features http3,tls --lib --tests -- -D warnings
cargo test -p eggserve-core --features http3,tls --test http3_runtime -- --test-threads=1
cargo check --workspace --all-targets --features http3,tls
cargo +1.89 check --workspace --all-targets --features http3,tls
git diff --check
python3 scripts/check-crate-topology.py
```

### Results

- `eggserve-h3`: 19 lib tests passed; 0 doc-tests.
- `eggserve-core --features http3,tls --lib`: 164 passed.
- `http3_runtime`: 16 passed.
- Clippy for `eggserve-h3` (all targets) and for `eggserve-core`
  (`http3,tls`, lib and tests): exit 0 with `-D warnings`.
- Workspace check on stable and on the 1.89.0 MSRV toolchain: exit 0.
- `git diff --check` and `scripts/check-crate-topology.py`: clean.

### Pre-fix behavior

The two new tests were run once against the pre-fix validator by temporarily
restoring the earlier non-zero-only arms in the working copy: both failed with
`called Result::unwrap_err() on an Ok value`, proving an above-ceiling value
previously validated. The fixed file was then restored byte-for-byte and the
config tests re-ran green (6 passed).

`./scripts/verify.sh fast` was not used as the gate: it cannot complete on this
macOS host because of the pre-existing `eggserve-server` test recorded in the
327 closure, which this milestone does not touch.

## 5. Invariant review

- QUIC VarInt wire fields (`stream_receive_window`, `connection_receive_window`,
  `max_idle_timeout`) keep their `2^62-1` bound and their existing tests.
- Defaults are unchanged: `send_window` 4 MiB, `max_send_buf_size` 256 KiB.
- Zero stays rejected for every bounded field, each naming itself.
- No 0-RTT, protocol-version, migration, retry, or identity change; H3 remains
  experimental and no CLI/Python knob was added.
- No key or certificate material appears in either new diagnostic.

## 6. Failure and recovery review

Validation is pure and runs before any endpoint exists: `RuntimeConfig::validate`
calls `Http3Config::validate` when H3 is enabled, and `Server::start` re-runs it
at `crates/eggserve-core/src/server/mod.rs:601` before the H3 endpoint is
assembled at `:700`. A rejected value therefore fails startup with
`ServerError::Config` before the UDP socket is bound, with no partial endpoint
and no state to unwind. No lock, task, or reload path is involved.

## 7. Migration and compatibility review

- A value above `u32::MAX` is now rejected where it was previously accepted.
  `Http3Config` is reachable only from Rust and H3 is experimental; the
  defaults and the public field types are unchanged.
- An embedding that deliberately set an unbounded send window must clamp to
  `u32::MAX` or keep the 4 MiB default.
- `max_send_buf_size` was already clamped by `stream_chunk_size` at
  `crates/eggserve-h3/src/response.rs:299-302`, so that half is preventive and
  has no observed runtime exposure.
- No variant, method, or dependency is added or removed.

## 8. Security review

- `send_window` is the one H3 knob that could silently remove quinn's
  per-connection retained-write memory cap; it is now bounded, so a
  misconfigured embedder cannot make one connection retain unbounded
  unacknowledged write memory.
- The two local ceilings share the HTTP/2 twin's bound, so the same class of
  buffer cannot be bounded differently in the two protocols.
- The diagnostics name the field and the range only; they carry no secret or
  identity material.

## 9. Documentation and operations

- `architecture/http3.md` now calls `send_window` the retained-write memory cap
  rather than a flow-control budget, states both ceilings, and distinguishes
  them from the VarInt wire bounds.
- `CHANGELOG.md` Unreleased gains the Plan 329 bullet and the header now covers
  Plans 320–329.
- `plans/registry.md`, `plans/subsystems/tls-identity-roadmap.md`, and the 327
  closure section 13 carry the closed status and the resolution.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| none | No new finding. The pre-existing macOS `app_bounded_body_accepts_and_over_limit_rejects` failure remains owned by `direct-h1-runtime`, unchanged and not exercised here. | `./scripts/verify.sh fast` still cannot complete on this host | Tracked in the 327 closure; separate milestone. |

## 11. Roadmap disposition

Milestone 329 is closed. It is the last open finding from the 327 post-merge
review. No hard dependency blocks the next TLS identity or H2/H3 milestone.

## 12. Registry updates

- `plans/registry.md`: milestone 329 marked closed with this record; next
  milestone number stays `330`.
- `plans/subsystems/tls-identity-roadmap.md`: milestone 329 marked closed.
- `plans/implementation/tls-identity/329-h3-send-window-bound.md`: status
  implemented.

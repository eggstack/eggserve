# CLI, Ops, and Observability Milestone 313 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/cli-ops-observability/313-lint-policy-enforcement.md`

Source subsystem roadmap:

- `plans/subsystems/cli-ops-observability-roadmap.md`

Repository baseline reviewed: `dfc9d2d` (the published `v0.5.0` corrective line)

Implementation commits or pull requests:

- Commit `c53dcf4` — the implementation: the `missing_docs` remediation
  across the seven workspace crates and the excluded Python crate, the
  `SAFETY:` comment remediation, the six clippy fixes in the excluded Python
  crate, the workspace lint table, the CI lanes, and the documentation.
- Commit `09b805c` — Plan 316's implementation, which also carries the
  doc comments added to `fs/dirbuf.rs` and `fs/windows.rs`. Those two files
  are the only ones both milestones touch, so the split is recorded here
  rather than hidden.

## 1. Executive finding

Two policies the repository already stated in prose are now enforced
mechanically, and the enforcement found real problems on the way.

- **`missing_docs` is enabled.** Every public item in every workspace crate
  now carries a doc comment. The remediation surface was **539** undocumented
  public items.
- **`clippy::undocumented_unsafe_blocks` is enabled.** Every `unsafe` block in
  the compiled code now carries a `SAFETY:` comment naming the invariant that
  makes it safe.

The milestone's premise about the second lint was **wrong in a way that
mattered**: the lint it named does not exist. `undocumented_unsafe_blocks` is
not a rustc lint and not a rustdoc lint — on the current toolchain both
reject it as an unknown lint. The lint that actually enforces the policy is
the **clippy** lint `clippy::undocumented_unsafe_blocks`, which belongs in
`[workspace.lints.clippy]` and is enforced by the clippy invocation rather
than by `rustdoc`. The requirement is unchanged; the lint's identity and its
enforcing tool differ from what the plan assumed. The deviation is recorded
in the implementation plan.

The same premise change had a second consequence the plan did not anticipate:
the lint only fires on code the compilation unit actually compiles, so on
Linux it covers only **2 of the repository's 37** `unsafe` blocks. The
Windows confinement boundary — the security-critical one — is `#[cfg(windows)]`
and never compiles on Linux. Enforcing the policy therefore required a clippy
lane that compiles the Windows code, which the milestone adds to the
`platform-compile` job's Windows leg.

The excluded Python crate was **not** covered by the plan's own verification
commands, and it carried six clippy errors. The plan's work package E
requires that crate in the lane; all six are fixed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| `missing_docs` is enabled workspace-wide | Root `Cargo.toml` `[workspace.lints.rust] missing_docs = "warn"`; all seven members inherit via `[lints] workspace = true` | pass |
| `clippy::undocumented_unsafe_blocks` is enabled | Root `Cargo.toml` `[workspace.lints.clippy] undocumented_unsafe_blocks = "warn"` | pass |
| The lint level matches the `-D warnings` policy | Both are `warn`, so the existing clippy invocation is the single enforcement point | pass |
| Every public item is documented | 539 items documented; `cargo clippy --workspace --lib --bins --tests -- -W missing_docs` reports 0 | pass |
| Every `unsafe` block carries an accurate `SAFETY:` comment | 23 comments added; 0 remaining on Linux **and** on the Windows target | pass |
| No `unsafe` block outside the approved boundaries | The static crate's 36 Windows items and 4 Unix FIFO items are all inside the four boundaries in `docs/unsafe-code-policy.md`; `fs/unix.rs` and `fs/dirbuf.rs` contain zero `unsafe` | pass — no finding to escalate |
| The `python-bindings-internal` feature is linted | CI step *Python-facing lint lanes*; `scripts/verify.sh` mirrors it; both lanes are clean | pass |
| The excluded Python crate is linted | CI step *Python-facing lint lanes*; `cargo clippy --manifest-path crates/eggserve-python/Cargo.toml --lib -- -D warnings` is clean | pass |
| The Windows boundary is linted where it compiles | CI step *Windows confinement clippy lane* in the `platform-compile` job's Windows leg; local cross-check `cargo clippy -p eggserve-static --lib --target x86_64-pc-windows-msvc -- -D warnings` is clean | pass |
| No public item was removed, renamed, or made private | The remediation is comment-only, except for one private `Copy` derive removal (see §8) | pass |
| No `unsafe` block's logic changed | The `unsafe {` counts in `fs/windows.rs` and `fs/mod.rs` are unchanged at 34 and 4 | pass |
| The documented lint policy matches the enforced policy | `architecture/testing-and-conformance.md`, `docs/unsafe-code-policy.md`, `AGENTS.md`, and the skill file all state the same two lints and the same four lanes | pass |

## 3. Production implementation evidence

### 3.1 The lint table

```toml
[workspace.lints.rust]
unsafe_code = "deny"
missing_docs = "warn"

[workspace.lints.clippy]
undocumented_unsafe_blocks = "warn"
```

Both lints are `warn` on purpose: the repository's single enforcement point
stays the existing `-D warnings` clippy invocation, so a contributor sees one
error rather than two policies.

### 3.2 The `missing_docs` remediation — 539 items

| Crate | Items | Principal files |
|---|---|---|
| `eggserve-server` | 249 | `ops/events.rs` (98 — 69 `EventKind` variants), `ops/counters.rs` (86), `runtime_limits.rs` (19), `config.rs` (14), `ops/sinks.rs` (12), `rejection.rs` (11) |
| `eggserve-primitives` | 118 | `primitives/body.rs`, `canonical/*`, `header_block.rs`, `http.rs`, `policy.rs`, `trailers.rs`, `tunnel.rs`, and others |
| `eggserve-static` | 93 | `secure_root.rs` (41), `path/rejected.rs` (18), `path/mod.rs` (10), `lib.rs` (9), `path/policy.rs` (6) |
| `eggserve-core` | 35 | `config.rs` (17), `server/static_service.rs` (9), `limits.rs` (9) |
| `eggserve-bin` | ~23 | `args.rs`, `lib.rs`, `main.rs` |
| `eggnet-tls` | 19 | `lib.rs` |
| `eggserve-h3` | 2 | `lib.rs` |

Plus the integration-test files, which are also public API of their test
crate: ~24 in `crates/eggserve-core/tests/**` and 2 in
`crates/eggserve-server/tests/**`.

No `#[allow(missing_docs)]` was added anywhere. The documentation states what
each item *is* and what it guarantees; fallible items carry `# Errors`
sections; public types document the invariants they uphold.

### 3.3 The `SAFETY:` remediation — 23 comments

| Location | Count | The invariant named |
|---|---|---|
| `fs/windows.rs` — `GetLastError` in `try_clone` | 1 | Thread-local Win32 error state; the failed `DuplicateHandle` set it. |
| `fs/windows.rs` — `unsafe impl Sync for OwnedHandle` | 1 | The wrapped handle is a process-global integer, and every operation on it — including the exactly-once `CloseHandle` on drop — is serialized by the OS, so no thread observes a torn or double-closed handle. |
| `fs/windows.rs` — `DuplicateHandle` in `duplicate_raw_handle` | 1 | `GetCurrentProcess()` is a valid pseudohandle; `source` is a live handle borrowed for the call and not consumed; `&mut new_handle` is a stack-allocated aligned output pointer; `DUPLICATE_SAME_ACCESS` copies access rights. |
| `fs/windows.rs` — `GetLastError` in `duplicate_raw_handle` | 1 | Thread-local Win32 error state; no caller preconditions. |
| `fs/windows.rs` — `OwnedHandle::from_raw` | 1 | `DuplicateHandle` succeeded, so `new_handle` is a fresh handle no other path owns or closes; `from_raw` takes that ownership and `Drop` closes it exactly once. |
| `fs/windows.rs` — Windows test module | 14 | `CreateFileW` ×7 (the UTF-16 buffer is live and NUL-terminated and is not retained by the API; the two null arguments are the optional security-attributes and template-handle parameters), `CloseHandle` ×6 (the handle came from the `CreateFileW` above, is still open, and is closed exactly once), `OwnedHandle::from_raw(null_mut())` ×1 (a deliberate invalid-handle fixture whose `Drop` checks `is_valid()` first). |
| `fs/mod.rs` — Unix FIFO test fixtures | 4 | `c_path` is a `CString` with no interior NUL, so `c_path.as_ptr()` is a valid NUL-terminated string that outlives the call; `mkfifo` only reads it and does not retain it. |

**No `unsafe` block outside the approved boundaries was found.** Every one of
the 40 `unsafe` items in the static crate lies inside one of the four
boundaries in `docs/unsafe-code-policy.md`: the Windows FFI boundary, the
Windows qualification-test boundary, or the Unix FIFO test-fixture boundary.
`fs/unix.rs` and `fs/dirbuf.rs` contain zero `unsafe` blocks.

### 3.4 The excluded Python crate — six clippy errors fixed

The crate is outside the workspace, so it inherits neither lint table nor the
workspace clippy invocation, and CI ran no clippy lane for it. All six errors
are fixed:

| Site | Lint | Fix |
|---|---|---|
| `src/server/body_bridge.rs:86` | `needless_borrow` | `PyIterator::from_object(&bound)` → `from_object(bound)` |
| `src/server/sync_handler.rs:514` | `if_same_then_else` | The `NOT_MODIFIED` branch and the `is_head` branch had identical bodies; they are merged into one condition (`status == NOT_MODIFIED \|\| is_head`). The third branch is unchanged, so the behavior is identical. |
| `src/server/tunnel_bridge.rs:241` | `type_complexity` | The nested generic is factored out into a `PyTunnelRx` type alias. |
| `src/lib.rs:1271` | `clone_on_copy` | The `from_py_object` derive generates a `.clone()` on the extracted value, and `PyHttpVersion` was `Copy`. The `Copy` derive is removed; `Clone` remains, so the generated call is legitimate. The trait was not needed by any caller, and the type is a private Rust struct, so the Python API is unchanged. |
| `src/server/static_responder.rs:46` | `too_many_arguments` (11/7) | The parameter list *is* the Python-visible method signature. Grouping the optional arguments into an options object would change that public API, so the arity is fixed and the lint is silenced at this one site, with the reason stated. |
| `src/lib.rs:1535` | `too_many_arguments` (8/7) | Same reasoning: the parameter list is the Python-visible constructor signature. |

The two `too_many_arguments` sites are the only lint suppressions in the
milestone, and both are single-function allows with a stated reason, not
blanket allows.

### 3.5 The lanes

`.github/workflows/ci.yml` gained two steps and `scripts/verify.sh` mirrors
them:

- **Python-facing lint lanes** (the `rust` job, and `cmd_fast`): the excluded
  Python crate, plus `eggserve-static` and `eggserve-core` with
  `--features python-bindings-internal`.
- **Windows confinement clippy lane** (the `platform-compile` job's Windows
  leg, and `cmd_platform_check`): `cargo clippy -p eggserve-static --lib -- -D
  warnings`. The local cross-check uses
  `--target x86_64-pc-windows-msvc`.

## 4. Verification executed

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --lib --bins --tests -- -D warnings` | clean, with both lints enabled in the inherited table |
| `cargo clippy --workspace --lib --bins --tests -- -W missing_docs` | 0 diagnostics |
| `cargo clippy --workspace --lib --bins --tests -- -W clippy::undocumented_unsafe_blocks` | 0 diagnostics |
| `cargo clippy -p eggserve-static --lib --target x86_64-pc-windows-msvc -- -D warnings` | clean |
| `cargo clippy -p eggserve-static --lib --tests --target x86_64-pc-windows-msvc -- -D warnings` | clean |
| `cargo clippy -p eggserve-static --features python-bindings-internal --lib --tests -- -D warnings` | clean |
| `cargo clippy -p eggserve-core --features python-bindings-internal --lib --tests -- -D warnings` | clean |
| `cargo clippy --manifest-path crates/eggserve-python/Cargo.toml --lib -- -D warnings` | clean (was 6 errors) |
| `cargo test -p eggserve-static --lib` | 372 passed, 0 failed, 3 ignored |
| `cargo test --workspace` | see §4.1 |
| `bash scripts/test-python-wheel.sh` | see §4.2 |
| `python3 scripts/check-crate-topology.py --self-test` | 76 checks passed |

### 4.1 Workspace tests

Run after the lint table was enabled, so the suite also proves the enabled
lints do not break any build configuration. Result: **all workspace test
suites and all doc-tests pass** (the doc-tests matter here — they compile
the code examples in the newly added doc comments).

### 4.2 The excluded Python crate's test suite

The six Python-crate fixes are behavior-relevant — the `if_same_then_else`
merge, the `Copy` derive removal, and the type alias all touch the Python
surface — so the wheel's test suite is the evidence that they changed nothing
observable. Result: **`bash scripts/test-python-wheel.sh` — 851 tests, OK.**

## 5. Invariant review

| Invariant | Result |
|---|---|
| No public API reduction | holds — the remediation is comment-only, except for one private `Copy` derive removal that has no Python-visible effect |
| No `unsafe` block's logic changed | holds — the `unsafe {` counts are unchanged at 34 (`fs/windows.rs`) and 4 (`fs/mod.rs`); no block was added, removed, or restructured |
| No blanket `#[allow]` | holds — no `#[allow(missing_docs)]` anywhere; the only two allows are single-function `too_many_arguments` allows with a stated reason |
| The approved unsafe boundaries are unchanged | holds — `docs/unsafe-code-policy.md`'s policy text is unchanged; only a cross-reference to the mechanical enforcement was added |
| The confinement boundary is unchanged | holds — no runtime or filesystem logic was touched |
| The excluded crate keeps working | holds — its own lint table is untouched and its Python API is unchanged |
| The MSRV lane still resolves the same features | holds — Plan 315's resolution is unaffected; `cargo +1.89 check` is clean |

## 6. Failure and recovery review

Not applicable to the lint policy itself: the lints are compile-time, and a
violation fails the build rather than a runtime.

The lane must fail the job on a warning: there is no `continue-on-error` in
CI and no blanket `#[allow]` on a module to silence a count.

The one behavior-relevant change is the `if_same_then_else` merge in the
excluded Python crate. It was verified by inspection (the two branches were
byte-identical, and the third branch is untouched) and by the wheel's test
suite.

## 7. Migration and compatibility review

- No public API change and no semver event.
- Doc comments are additive and visible on docs.rs; a consumer reading the
  published docs sees more, not less. The crates' published documentation
  improves substantially: 539 public items were previously undocumented.
- `SAFETY:` comments are internal to the source and are not published
  behavior.
- The `python-bindings-internal` feature's meaning is unchanged.
- The excluded Python crate's Python API is unchanged. The one Rust-side
  change — dropping `Copy` from a private struct — is not observable from
  Python.
- The `PyTunnelRx` type alias and the merged `if` branch are internal
  refactors with identical behavior.

## 8. Security review

- `clippy::undocumented_unsafe_blocks` is the mechanical form of
  `docs/unsafe-code-policy.md`. Every `SAFETY:` comment names the invariant
  that makes its block safe, so a later edit which invalidates that invariant
  is visible in review. The comments were written against the actual
  implementation, not templated.
- A block outside the approved boundaries would be a finding to report, never
  a block to bless. **No such block was found**: all 40 `unsafe` items in the
  static crate sit inside the four approved boundaries, and
  `fs/unix.rs` and `fs/dirbuf.rs` contain no `unsafe` at all.
- The lint is enforced on the Windows boundary, which is the security-critical
  one. Before this milestone the policy was prose that no tool checked; now a
  new `unsafe` block without a `SAFETY:` comment fails the Windows lane in
  routine CI.
- The `unsafe` count did not rise: 34 blocks in `fs/windows.rs` and 4 in
  `fs/mod.rs`, unchanged.
- `unsafe_code = "deny"` remains in force, so a new `unsafe` block still
  requires a narrow module-level exception in an approved boundary.

## 9. Documentation and operations

- `architecture/testing-and-conformance.md` — a new *Enforced lint policy*
  section stating both lints, their levels, the four lanes, and what a
  contributor does when a lint fires.
- `docs/unsafe-code-policy.md` — a cross-reference to the mechanical
  enforcement. The policy text and the approved-boundaries table are
  unchanged.
- `AGENTS.md` — the toolchain note now states that the two policies are
  mechanically enforced, that the existing `-D warnings` invocation is the
  single enforcement point, that a blanket `#[allow]` is not the answer, and
  that the Windows boundary is only linted where `#[cfg(windows)]` code is
  compiled. It also records that the excluded Python crate inherits neither
  lint table nor the workspace clippy invocation and is linted by its own
  lane.
- `.opencode/skills/eggserve-dev/SKILL.md` — the same statement, with the
  four lane commands in the command list.
- `plans/implementation/cli-ops-observability/313-lint-policy-enforcement.md` —
  the recorded material deviation.

## 10. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| low | `cargo clippy -p eggserve-server` also lints `eggserve-primitives`, because Cargo does not apply `--cap-lints allow` to a workspace-member path dependency. A crate-scoped clippy invocation therefore covers two crates. | Known Cargo behavior, not a defect. The workspace-wide invocation is the authoritative lane, and the crate-scoped ones are additional. |
| low | `RuntimeRejectionKind::Internal` and twelve `EventKind` variants have no producer anywhere in the workspace. The static-serving kinds (`FileNotFound`, `FileDenied`, `FileError`, `DotfileDenied`, `SymlinkDenied`, `RootEscapeDenied`, `DirectoryListingLimit`) read as vocabulary reserved for an `eggserve-static` integration that does not emit events yet. | Documented as reserved rather than given an invented behavior. Recorded here as a finding for a future milestone. |
| low | The `docs/dependency-policy.md` allowed-categories table lists `windows-sys`, which no member manifest declares — the Windows FFI is hand-written `extern "system"` declarations. | Pre-existing drift, carried from Plan 315 and not in this milestone's scope. |
| low | 18 pre-existing broken intra-doc links in `crates/eggserve-server/src/**` (rustdoc 1.99.1 does not resolve bare intra-doc links in module-level `//!` docs against items in that same module) were repaired as part of this milestone, because the `RUSTDOCFLAGS="-D warnings" cargo doc` acceptance criterion is unconditional. | Fixed, doc-comment-only, by qualifying the paths. The `cargo doc -D warnings` check is now clean for `eggserve-server`. |

## 11. Roadmap disposition

**Milestone closed.**

The two policies the repository states are now mechanically enforced, the
remediation surface is closed, the Windows boundary is linted where it
actually compiles, the excluded Python crate is in a lane, and the
documentation matches the enforcement. A future contributor who adds a public
item without a doc comment, or an `unsafe` block without a `SAFETY:`
comment, fails CI rather than passing review by luck.

The roadmap's Milestone 313 moves `ready` → `closed`.

## 12. Registry updates

`plans/registry.md`:

- The CLI, ops, and observability row's current milestone becomes 313,
  closed, with this closure record linked.
- The dependency-ready implementation plans row for 313 moves `ready` →
  `closed`, with this closure record and the recorded deviation.

`plans/subsystems/cli-ops-observability-roadmap.md`:

- Milestone 313 moves `ready` → `closed`, with this closure record linked and
  the recorded deviation named.

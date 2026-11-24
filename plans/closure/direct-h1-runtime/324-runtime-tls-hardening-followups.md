# Direct H1 Runtime Milestone 324 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/direct-h1-runtime/324-runtime-tls-hardening-followups.md`

Source subsystem roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md`

Repository baseline reviewed: `1585072dfd8fcf8d3df704d0d245f5fc40d0bf7e`.

Implementation state: the change is in the working tree. No commit was created
by this task.

## 1. Executive finding

All three fail-closed gaps are corrected without a TLS identity/trust/auth
change and without a new public error variant.

1. **Systemd adoption is once-only.** `ServerBuilder` tracks adopted indices
   and names. `from_systemd_index` and `from_systemd_name` reject a duplicate
   **before** `adopt_systemd_listener` borrows the fd, so a second
   `FromRawFd`/double close is impossible. `from_systemd_name` resolves the
   name to its index first, so a repeated name and two names aliasing one
   index both fail closed.
2. **Ambiguous framing closes.** Both the direct and compatibility pipelines
   now use `LifecycleDisposition::for_ambiguous_framing()`
   (`close_and_cancel_body`) instead of `KEEP_ALIVE`, keeping the 400 status
   and sanitized body.
3. **Identity PEM is bounded before parsing.** `MAX_IDENTITY_PEM_BYTES`
   (1 MiB) is enforced in `parse_identity_pem` and by `take(MAX + 1)` file
   reads in `load_identity`; the existing chain/single-key checks still run
   after parsing.

## 2. Requirement-to-evidence matrix

| Plan requirement | Status | Evidence |
|---|---|---|
| Duplicate index rejected before fd access | met | `check_systemd_adoption`; unit test |
| Duplicate name and index alias rejected before fd access | met | name resolution + `check_systemd_adoption(index, Some(name))`; unit test |
| Successful/failed adoption semantics unchanged | met | Tracking records only after successful adoption; existing `listener_ownership` tests pass |
| Systemd guard documented | met | builder docs; `docs/unsafe-code-policy.md` |
| Framing rejection closes in both pipelines | met | `for_ambiguous_framing()` in both pipelines; unit tests |
| 400 status/sanitized body unchanged | met | Only the disposition changed |
| Identity PEM in-memory ceiling | met | `parse_identity_pem` pre-check; over/at-ceiling tests |
| Identity file read ceiling without metadata | met | `take(MAX + 1)`; file-path tests |
| Chain/single-key/pairing checks unchanged | met | Existing `eggnet-tls` tests pass |
| No new `TlsError` variant | met | `CertReadError`/`KeyReadError` reused |
| Docs updated | met | §6 |
| No H3 behavior change | met | Not touched |

## 3. Implementation evidence

### 3.1 Systemd once-only contract

```rust
// crates/eggserve-core/src/server/mod.rs
#[cfg(unix)]
fn check_systemd_adoption(&self, index: usize, name: Option<&str>)
    -> Result<(), ServerError>
```

Both adopters call it before `adopt_systemd_listener`; the index/name are
recorded only after success. `systemd_index_for_name` was factored out of the
adoption path so the name can be resolved and checked first. The redundant
`adopt_systemd_listener_by_name` wrapper was removed.

### 3.2 Framing disposition

`LifecycleDisposition::for_ambiguous_framing()` is a documented constructor
for `close_and_cancel_body()`; both pipelines call it at the
`validate_body_framing` rejection. The comment records that Hyper 1.11 makes
the branch unreachable (duplicate `Content-Length` rejected, lone `CL`+`TE`
normalized), so this is defense-in-depth, not a live wire path.

### 3.3 Identity PEM bound

- `MAX_IDENTITY_PEM_BYTES = 1024 * 1024`, documented next to the trust bound.
- `parse_identity_pem` rejects `cert_pem.len() > MAX` with `CertReadError` and
  `key_pem.len() > MAX` with `KeyReadError` before constructing PEM readers.
- `load_identity` reads each file through `take(MAX + 1)` and rejects a buffer
  over the ceiling, then delegates to `parse_identity_pem` (one parser, one
  pairing check).
- No `TlsError` variant was added; the enum is unchanged.

## 4. Verification executed

| Command | Outcome |
|---|---|
| `cargo test -p eggnet-tls` | PASS — 9 integration tests (includes the new bound test) |
| `cargo test -p eggserve-server --lib` | PASS — 92 (includes `ambiguous_framing_closes_and_cancels_body`) |
| `cargo test -p eggserve-core --lib` | PASS — 161 (includes `systemd_duplicate_adoption_is_rejected_before_fd_access`) |
| `cargo test -p eggserve-core --lib --features http2` | PASS — 176 (includes the compatibility framing test) |
| `cargo test -p eggserve-core --test listener_ownership` | PASS — 5 |
| `cargo test -p eggserve-core --test request_body_wire` | PASS — 25 |
| `cargo clippy --workspace --lib --bins --tests -- -D warnings` | PASS |
| `cargo fmt --all -- --check` | PASS |

The PEM test covers: certificate one byte over the ceiling; key one byte over;
certificate exactly at the ceiling still parsing; a normal file pair loading; a
file over the ceiling rejected for certificate and key; and recovery after
restoring a valid file. `take(MAX + 1)` is authoritative rather than
`metadata().len()`.

## 5. Invariant review

| Invariant | Result |
|---|---|
| One adoption, one owner | Held; duplicate rejected before the borrow |
| Failure never closes a descriptor | Held; validation failure adds no tracking entry and takes no ownership |
| Ambiguous framing never keeps a reusable connection | Held; close + cancel body in both pipelines |
| TLS identity semantics unchanged | Held; no SNI/trust/client-auth/CRL/ALPN/reload/pairing change |
| No public error-variant break | Held |
| No local read without a ceiling | Held; in-memory and file paths |

## 6. Documentation and operations

| Surface | Change |
|---|---|
| `crates/eggserve-core/src/server/mod.rs` | Once-only adoption contract + guard |
| `crates/eggnet-tls/src/lib.rs` | `MAX_IDENTITY_PEM_BYTES`, bounded load |
| `docs/tls.md` | Identity PEM input bound |
| `docs/unsafe-code-policy.md` | Systemd boundary names the duplicate guard |
| `CHANGELOG.md` | Unreleased entry |

## 7. Unresolved findings

- No end-to-end double-adoption test with a real listening fd at `3 + N`.
  Constructing that fixture requires `dup2`/process-level setup and `unsafe`
  code, which the workspace denies outside reviewed boundaries. The pure guard
  is unit-tested; the adoption path itself is covered by the existing
  `listener_ownership` suite for the single-adoption success/failure cases.
  This is reported, not fixed.
- The framing branch remains unreachable under Hyper 1.11; the evidence is the
  disposition choice and its unit tests, not a wire reproduction. This matches
  the plan.

## 8. Roadmap disposition

`plans/subsystems/direct-h1-runtime-roadmap.md` local Milestone 17 (plan 324)
is closed.

## 9. Registry updates

`plans/registry.md` row 324 moves to `closed` with a pointer to this record;
the direct-H1 roadmap milestone table and status rows are updated to match.

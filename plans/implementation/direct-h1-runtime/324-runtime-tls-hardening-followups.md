# Plan 324 — Runtime and TLS hardening follow-ups

## 1. Status

- **State**: ready for handoff
- **Plan number**: 324
- **Subsystem**: Direct H1 runtime + service contract
  (`plans/subsystems/direct-h1-runtime-roadmap.md`, Milestone 17), with a
  bounded interface change in the neutral `eggnet-tls` substrate
- **Source roadmap**: `plans/subsystems/direct-h1-runtime-roadmap.md`
- **Long-term requirements**:
  - `plans/000-long-term-specification.md#2` (fail-closed resource bounds;
    TLS identity is transport-owned)
  - `plans/001-terminology-and-domain-model.md` (`ConnectionContext`,
    `TlsError`, listener ownership)
  - `docs/unsafe-code-policy.md` (systemd descriptor boundary)
  - `docs/tls.md` (identity input contract)
- **Primary class**: invariant (resource bounds and fail-closed teardown)

## 2. Repository baseline

Repository baseline: `1585072dfd8fcf8d3df704d0d245f5fc40d0bf7e`.

Three Low-severity audit findings remain after the externally visible
corrective plans:

1. **Systemd descriptor double adoption.** `ServerBuilder::from_systemd_index`
   calls `adopt_systemd_listener(index)` and replaces the listener source
   (`crates/eggserve-core/src/server/mod.rs:343-353`). The adoption
   validates a borrowed fd and then takes ownership with
   `FromRawFd::from_raw_fd` (`crates/eggserve-core/src/server/listener.rs:249-266`).
   Calling the builder method twice with the same index (or two names that
   resolve to the same index) adopts the same descriptor twice, producing a
   double close / I/O-safety violation. `from_systemd_name` has the same
   shape (`crates/eggserve-core/src/server/mod.rs:360-370`).
2. **Framing rejection keeps the connection alive.** When
   `validate_body_framing` rejects duplicate `Content-Length` or a TE+CL
   conflict, both pipelines return 400 with
   `LifecycleDisposition::KEEP_ALIVE`
   (`crates/eggserve-server/src/connection/pipeline.rs:878-900`,
   `crates/eggserve-core/src/server/connection/pipeline.rs:860-887`).
   Under Hyper 1.11 this branch is unreachable (Hyper rejects duplicate CL
   and normalizes a lone CL+TE), so this is defense-in-depth; if a future
   parser reaches it, an ambiguous framing request should close rather than
   rely on the dropped body forcing close.
3. **Identity PEM has no pre-parse size bound.** `parse_identity_pem`
   collects all certificate items before enforcing the 8-identity chain cap,
   and `load_identity` reads certificate/key files to EOF before the same
   checks (`crates/eggnet-tls/src/lib.rs:193-220`, `287-321`). Trust roots
   and CRLs already have a pre-parse 1 MiB bound
   (`lib.rs:224-261`). A very large or adversarial local PEM can consume
   parser time/memory at startup or reload before the count checks fire.

## 3. Problem statement

These are bounded defensive gaps, not remote path-exposure defects. They
are grouped because each is a small fail-closed correction in the runtime's
ownership or parser boundary:

- double adoption violates the descriptor-ownership invariant the module
  documents (`listener.rs:40-42`);
- keep-alive on ambiguous framing is the wrong default for a rejection that
  exists specifically to prevent message-boundary confusion;
- an unbounded local PEM read is the same class of resource-exhaustion gap
  the trust/CRL parsers already close.

## 4. Goal

Guarantee one-time systemd descriptor adoption, make framing rejection
close the connection, and apply a pre-parse size ceiling to identity PEM
input without changing TLS identity/trust semantics.

## 5. Non-goals

- No change to PROXY protocol trust, TLS client-auth modes, SNI matching,
  trust roots, CRLs, ALPN, or reload semantics.
- No new public `TlsError` variant (the enum is not `non_exhaustive`;
  reuse the read-error variants with a bounded-size message). If a new
  variant is deemed necessary, stop and record the semver decision.
- No change to the systemd adoption contract on success: a single valid
  index/name is still adopted once, with the same validation and failure
  behavior.
- No new config knob for the PEM ceiling; use a fixed constant alongside
  `MAX_TRUST_PEM_BYTES`.
- No change to the H1 framing validator's rejection conditions; only the
  connection disposition changes.
- No H3 behavior change.

## 6. Scope

### Work package A — One-time systemd adoption

Track adopted systemd descriptors in `ServerBuilder`:

- Add a private set of adopted indices and adopted names (or a single
  `BTreeSet<usize>` plus a `BTreeSet<String>`); `ServerBuilder` is not
  `Clone`, so no clone propagation is needed.
- In `from_systemd_index`, check the index **before** calling
  `adopt_systemd_listener`; on a duplicate return a clear
  `ServerError::Config` and do not borrow or take the fd.
- In `from_systemd_name`, resolve the name to its index **before**
  adoption (adjust `adopt_systemd_listener_by_name` to return the index
  alongside the listener, or add a lookup helper), check both the name and
  the resolved index, then adopt once and record both.
- Successful adoption records the index/name. Validation failure leaves
  the descriptor unowned and the tracking set unchanged.
- Add a doc comment stating the once-only contract and why it is enforced
  (double `FromRawFd` would double close).

### Work package B — Fail-closed framing rejection

- Change the framing-rejection disposition in both pipelines from
  `LifecycleDisposition::KEEP_ALIVE` to
  `LifecycleDisposition::close_and_cancel_body()` (or the closest
  fail-closed disposition already used by body-policy rejection).
- Keep the 400 status and the existing sanitized error body.
- Add a comment that Hyper 1.11 makes the branch unreachable, so this is
  defense-in-depth for a future/alternate parser.
- If the disposition can be exercised through a focused helper, add a unit
  test; if not, record code-review-only evidence rather than inventing an
  unreachable test. Do not weaken the existing framing tests.

### Work package C — Identity PEM pre-parse bound

- Add `MAX_IDENTITY_PEM_BYTES` (1 MiB, matching `MAX_TRUST_PEM_BYTES`).
- In `parse_identity_pem`, reject `cert_pem.len() > MAX` and
  `key_pem.len() > MAX` before constructing the PEM readers. Return
  `TlsError::CertReadError` / `TlsError::KeyReadError` with a bounded-size
  message (no new variant).
- In `load_identity`, bound both file reads: use `Read::take(MAX + 1)`,
  then reject when the buffer exceeds `MAX`. Do not rely on
  `metadata().len()` alone (the file can grow); do not `read_to_end`
  without a limit.
- Keep the existing chain/key-count checks after parsing; the bound is
  additive.
- Do not change certificate/key pairing validation.

### Work package D — Tests

1. **Systemd duplicate guard**
   - Unit test the tracking check: inserting an index/name twice is
     rejected before adoption; a distinct index is accepted.
   - If the environment can construct a valid listening fd at 3+N (child
     process / `dup2` fixture), add an end-to-end double-adoption test and
     assert the second call fails without closing the first listener.
   - If the fd fixture is not portable, record the limitation and test the
     pure guard.
2. **Framing disposition**
   - If factorable, a unit test asserts the framing-error helper selects
     the close disposition.
   - Existing framing/body rejection tests stay green.
3. **PEM bound**
   - `parse_identity_pem` rejects a certificate or key blob one byte over
     the ceiling before parsing.
   - At the ceiling, a valid identity still parses.
   - `load_identity` rejects a file larger than the ceiling; a normal
     file still loads and pairs.
   - Trust-root/CRL bounds are unchanged.

## 7. Invariants that must not regress

1. **One adoption, one owner.** A raw systemd fd is borrowed for
   validation at most once and converted to an owned fd at most once.
2. **Failure never closes a descriptor.** Validation failure leaves the
   caller/manager's fd open, as today.
3. **Ambiguous framing never keeps a reusable connection.** The rejection
   closes and cancels the body.
4. **TLS identity semantics are unchanged.** SNI, trust, client auth,
   CRLs, ALPN, reload, and key/cert pairing are untouched.
5. **No public error-variant break.** Reuse existing `TlsError` variants.
6. **No local read without a ceiling.** Identity certificate and key
   parsing never consumes unbounded bytes.

## 8. Failure, cancellation, restart, and contention semantics

- **Duplicate adoption**: rejected before fd ownership; the first listener
  remains usable.
- **Adoption validation failure**: no tracking entry is added; the fd is
  not closed.
- **Framing rejection**: connection closes; no pipelined reuse.
- **Oversized PEM**: startup/reload returns a configuration error before
  parsing; the live TLS configuration is untouched on reload failure.
- **Concurrent reload**: the existing atomic snapshot semantics are
  unchanged; the bound applies before any new snapshot is built.
- **Partial read / file growth**: `take(MAX+1)` is authoritative; a file
  that grows after open is still rejected when it crosses the bound.

## 9. Compatibility and migration

- Single-index/name adoption behavior is unchanged. Double adoption now
  fails with `ServerError::Config` instead of undefined descriptor
  ownership; this is a bug fix for an unsupported misuse.
- Framing rejection is already a 400; the connection closing is stricter
  but only reachable behind a parser that produces the ambiguous state.
- The identity PEM ceiling is 1 MiB, far above any normal chain/key. A
  caller with an unusually large PEM gets a configuration error, not a
  panic.
- No public type/variant changes.

## 10. Required tests

### Focused

- Systemd tracking guard (duplicate index, duplicate name/alias, distinct
  index, failed adoption leaves no entry).
- Identity PEM over/at ceiling for in-memory and file paths.
- Framing disposition helper, if factored.

### Integration

- `cargo test -p eggserve-core --test listener_ownership`.
- `cargo test -p eggnet-tls`.
- Existing `crates/eggserve-server` framing/body tests.
- A reload test with an oversized identity file leaves the live config
  unchanged (if the reload test fixture is available).

### Security and negative

- Double adoption cannot double close.
- Oversized PEM is rejected without parsing the full content.
- The framing branch cannot be reached with a reusable connection.

## 11. Required verification commands

```bash
cargo test -p eggnet-tls
cargo test -p eggserve-server --lib
cargo test -p eggserve-core --test listener_ownership
cargo test -p eggserve-core --lib
./scripts/verify.sh fast
```

The systemd end-to-end fd fixture may require a Unix-only test; record
the platform and skip reason if it cannot run.

## 12. Documentation updates

- `crates/eggserve-core/src/server/mod.rs` — once-only adoption contract.
- `crates/eggnet-tls/src/lib.rs` — identity PEM ceiling constant and doc.
- `docs/tls.md` — identity PEM input bound.
- `docs/unsafe-code-policy.md` only if the systemd unsafe-boundary
  description needs the duplicate guard named.
- `plans/subsystems/direct-h1-runtime-roadmap.md` and `plans/registry.md`
  status.

## 13. Acceptance criteria

- A second `from_systemd_index`/`from_systemd_name` adoption of the same
  descriptor fails before fd ownership and leaves the first listener
  intact.
- Framing rejection closes the connection in both pipelines.
- Identity PEM input is bounded before parsing; normal identities still
  parse and pair.
- No TLS identity/trust/auth behavior change.
- No new public error variant.

## 14. Stop conditions

Stop and report rather than improvise when:

- once-only tracking cannot be added without changing a public builder
  type/semantics;
- a name lookup cannot resolve to an index without a new public API;
- the framing disposition change requires touching the validator or
  response status;
- the PEM ceiling would reject an existing supported fixture (record the
  size and reconsider the constant);
- a TLS error variant seems necessary (semver decision).

## 15. Closure evidence required

- Duplicate-adoption test output (and the recorded limitation if the
  end-to-end fd fixture is unavailable).
- Framing disposition evidence (unit test or code-review record).
- PEM over/at-ceiling test output for both in-memory and file paths.
- `listener_ownership`, `eggnet-tls`, and server test outputs.
- Confirmation that TLS identity/trust/auth tests are unchanged.
- Documentation diffs.

## 16. Handoff notes

- `adopt_systemd_listener` currently borrows the fd before validation and
  takes ownership only after; preserve that ordering. The duplicate check
  must happen **before** the borrow, or a rejected duplicate still touches
  a descriptor it must not own.
- `ServerBuilder` is not `Clone`; tracking state is builder-local. If that
  changes later, the tracking must clone with it.
- The framing branch is unreachable under Hyper 1.11; do not claim a live
  wire reproduction. The required evidence is the disposition choice plus
  the existing validator tests.
- The PEM bound is a fixed safety constant, not a policy knob. Match the
  trust/CRL ceiling unless a concrete chain-size requirement says
  otherwise.

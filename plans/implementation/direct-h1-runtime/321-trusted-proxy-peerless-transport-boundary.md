# Plan 321 — Peerless-transport trust boundary for forwarded metadata

## 1. Status

- **State**: ready for handoff
- **Plan number**: 321
- **Subsystem**: Direct H1 runtime + service contract
  (`plans/subsystems/direct-h1-runtime-roadmap.md`, Milestone 16)
- **Source roadmap**: `plans/subsystems/direct-h1-runtime-roadmap.md`
- **Long-term requirements**:
  - `plans/000-long-term-specification.md#2` (safe defaults; no implicit
    trust)
  - `plans/001-terminology-and-domain-model.md` (`ConnectionContext`,
    `TrustedProxyConfig`)
  - `docs/threat-model.md` (forwarding-spoof attacker)
  - `docs/security-policy.md` (trusted-proxy scope)
- **Primary class**: invariant (trusted-proxy trust boundary)

## 2. Repository baseline

Repository baseline: `1585072dfd8fcf8d3df704d0d245f5fc40d0bf7e`.

A security audit found that the forwarded-header trust decision uses
"no remote address" as a proxy for "Unix-domain transport":

- `ConnectionContext::for_non_socket` and `ConnectionContext::for_unix`
  both set `remote_addr: None`
  (`crates/eggserve-server/src/connection/context.rs:121-131` and
  `141-151`).
- The direct pipeline maps every `None` peer to
  `config.trusted_proxy.trust_unix`
  (`crates/eggserve-server/src/connection/pipeline.rs:94-97`).
- The compatibility pipeline does the same
  (`crates/eggserve-core/src/server/connection/pipeline.rs:72-80`).
- The public caller-owned driver accepts arbitrary byte streams plus a
  caller-supplied `ConnectionContext`
  (`crates/eggserve-server/src/connection/mod.rs:143-153`).
- The option is documented as Unix-domain-specific
  (`crates/eggserve-server/src/config.rs:654-658`,
  `crates/eggserve-primitives/src/primitives/proxy.rs:386-389`).

The trusted-proxy suite (35 tests) covers TCP peers, PROXY preambles,
Forwarded conflicts, and spoofing, but it does not distinguish a Unix
listener from a generic peerless stream.

## 3. Problem statement

`trust_unix` is supposed to mean "the immediate peer is a local Unix-domain
listener I control; accept its `Forwarded`/`X-Forwarded-*` metadata".
Instead it means "any connection with no socket address is trusted".

An embedder that serves an untrusted byte stream through
`serve_http1_connection` (the documented caller-owned transport, e.g. a
stream handed over by an outer anonymity/proxy transport) and also enables
`trusted_proxy.trust_unix` for its Unix listener will treat
attacker-supplied forwarding headers on the caller-owned stream as trusted
effective metadata. The raw peer and canonical Host are not rewritten, so
the defect matters where downstream code uses `effective_client` /
`effective_scheme` / `effective_authority` for authorization, origin
policy, or logging.

This is a conditional trust-boundary defect, not an unauthenticated path
escape. The fix is still required because the current behavior violates the
documented scope of the option and fails open on a transport the caller
does not control.

## 4. Goal

Make forwarded-header trust a function of the actual transport kind:
Unix-domain listeners owned by the runtime may use `trust_unix`; TCP/TLS/
QUIC peers use their explicit peer/CIDR policy; caller-owned peerless
streams are never trusted by `trust_unix`.

## 5. Non-goals

- No broad change to `Forwarded` parsing, rightmost-wins, conflict
  handling, bounds, or provenance vocabulary.
- No change to PROXY protocol trust (it is already gated by the immediate
  peer before reading).
- No new default trust for loopback, DNS names, or caller-owned streams.
- No change to the Python `lowlevel` trust fields' names or defaults.
- No H3 forwarded support; H3 still ignores the forwarded policy by
  construction.
- No public `ConnectionContext` field addition if it can be avoided (see
  §6).

## 6. Scope

### Work package A — Carry an internal transport kind to the trust decision

Introduce an internal, non-public transport-kind value (for example
`PeerTransport::{Socket, Unix, Opaque}`) inside `eggserve-server` and pass
it through the internal pipeline construction. It must be set by the code
that owns the transport, never by request data:

- TCP and TLS accept paths → `Socket` (the existing `remote_addr` check
  still applies).
- QUIC/H3 path → `Socket` (H3 ignores forwarded policy today; keep that).
- Unix listener accept paths (direct `Server` and core compatibility
  `accept_loop_multi`, including systemd-adopted Unix listeners) → `Unix`.
- The public caller-owned `serve_http1_connection` /
  `serve_http_connection` entry points → `Opaque`, even when the caller
  supplies `ConnectionContext::for_unix()`.

Then compute:

```text
Socket → is_trusted_peer(remote_addr)
Unix   → config.trusted_proxy.trust_unix
Opaque → false
```

`apply_forwarded_policy` in both pipelines takes the kind as an internal
argument. The two pipelines must not re-derive it from `remote_addr`.

**Public-API constraint:** `ConnectionContext` fields are currently all
public, so adding a field (public or private) can break external struct
literals. Prefer threading the kind through internal driver/pipeline
parameters. If that is not reachable without a public change, stop and
report the semver decision; do not add a breaking field silently.

### Work package B — Preserve the existing TCP/PROXY behavior

- TCP trusted-peer and explicit CIDR behavior is unchanged.
- Untrusted TCP peers still ignore forwarded headers.
- PROXY protocol acceptance still happens only from a trusted immediate
  peer before TLS/HTTP.
- The accepted-forwarded observability fields and counters are unchanged.

### Work package C — Tests for the trust matrix

Add direct-runtime and compatibility tests that distinguish transport
kind from address absence:

| Transport | `trust_unix` | `Forwarded` present | Expected |
|---|---|---|---|
| TCP trusted peer | false | yes | trusted |
| TCP untrusted peer | false | yes | ignored |
| Unix listener | true | yes | trusted |
| Unix listener | false | yes | ignored |
| Caller-owned peerless stream (`for_non_socket`) | true | yes | **ignored** |
| Caller-owned peerless stream (`for_unix` context passed to the public driver) | true | yes | **ignored** |
| Forwarded disabled | any | yes | ignored |

At least one test must go over a real Unix listener and assert the
effective client is adopted; one must use the public caller-owned driver
with a spoofed header and assert it is not. Run the compatibility pipeline
against the same matrix (or prove the shared helper is used by both).

### Work package D — Documentation and compatibility note

- `crates/eggserve-server/src/config.rs` — state that `trust_unix_local`
  applies only to runtime-owned Unix-domain listeners, never to
  caller-owned peerless streams.
- `crates/eggserve-primitives/src/primitives/proxy.rs` — same scope note
  on `trust_unix`.
- `docs/threat-model.md` — update the forwarding-spoof attacker paragraph
  to name the transport-kind boundary.
- `docs/python-api.md` — the `lowlevel` `trust_unix_local` description, if
  it implies broader scope.
- `docs/migration-guide.md` — record the behavior change for embedders
  that relied on `trust_unix` applying to caller-owned streams. This is a
  security correction, not a silent compatibility break.

## 7. Invariants that must not regress

1. **No implicit trust.** Forwarded metadata is adopted only from an
   explicitly trusted immediate transport peer; `remote_addr: None` alone
   is never trust.
2. **Canonical request data is never rewritten.** Raw peer, `Host`, and
   the request target stay untouched.
3. **PROXY trust is separate.** A PROXY preamble is still read only after
   the immediate-peer check, before TLS/HTTP, with its existing bounds.
4. **One trust decision.** Both the direct and compatibility pipelines use
   the same kind-to-trust mapping; no second copy of the boolean logic.
5. **H3 stays out.** H3 continues to ignore forwarded metadata.
6. **No new public field without a semver decision.** Prefer internal
   plumbing.

## 8. Failure, cancellation, restart, and contention semantics

- A missing or unknown transport kind must fail closed (`Opaque`), never
  fall back to trusted.
- A Unix accept path that cannot identify its listener kind must be
  treated as `Opaque` and emit a diagnostic; it must not guess.
- Concurrent requests on one connection share the connection's kind; no
  per-request mutation.
- Shutdown/cancellation semantics are unchanged.

## 9. Compatibility and migration

- TCP/TLS/QUIC behavior is unchanged.
- Runtime-owned Unix listeners keep the documented `trust_unix` behavior.
- Caller-owned peerless streams lose implicit `trust_unix` trust. This is
  the intended correction; record it in `CHANGELOG.md` and the migration
  guide. Embedders that genuinely need to trust a caller-owned transport
  must not be given an implicit path; a future explicit opt-in would be a
  new, separately reviewed surface.
- The Python facade's low-level `trust_unix_local` is passed to the native
  runtime; runtimes with a Unix listener keep working. No Python signature
  change.

## 10. Required tests

### Focused

- Kind-to-trust mapping unit test for `Socket`/`Unix`/`Opaque` with
  `trust_unix` on/off and forwarded disabled.
- Address-based trust unit tests remain green.

### Integration

- Direct `Server` on a Unix listener + `Forwarded: for=…` + `trust_unix`.
- Public `serve_http1_connection` with `ConnectionContext::for_non_socket`
  and with `for_unix`, both + spoofed `Forwarded` + `trust_unix`.
- Compatibility `serve_http_connection` parity for the same two cases.
- TCP trusted/untrusted regression (existing suite).

### Security and negative

- Spoofed `Forwarded`/`X-Forwarded-*` on an untrusted caller-owned stream
  produces no effective client/scheme/authority and increments the
  rejected counter.
- Conflicting forwarded values still fail closed.

## 11. Required verification commands

```bash
cargo test -p eggserve-server --lib
cargo test -p eggserve-core --test trusted_proxy
cargo test -p eggserve-core --test cross_protocol_conformance
cargo test -p eggserve-core --lib
./scripts/verify.sh fast
```

Add the Unix/caller-owned matrix to the focused trusted-proxy suite; do
not rely only on the existing TCP suite.

## 12. Documentation updates

- `crates/eggserve-server/src/config.rs` and the primitives `proxy.rs` doc
  comments.
- `docs/threat-model.md` forwarding-spoof section.
- `docs/python-api.md` if the trust scope is described.
- `docs/migration-guide.md` and `CHANGELOG.md` for the behavior change.
- `plans/subsystems/direct-h1-runtime-roadmap.md` and `plans/registry.md`
  status.

## 13. Acceptance criteria

- A Unix listener with `trust_unix` adopts trusted forwarded metadata.
- A caller-owned peerless stream with `trust_unix` does **not** adopt it,
  regardless of which `ConnectionContext` constructor or public driver is
  used.
- Both pipelines share one trust mapping.
- TCP/PROXY behavior and tests are unchanged.
- No public `ConnectionContext` field is added without an explicit semver
  decision recorded in the plan.

## 14. Stop conditions

Stop and report rather than improvise when:

- the internal kind cannot reach both pipelines without a public API
  change;
- a legitimate caller-owned trust use case is discovered that cannot be
  expressed without a new explicit opt-in;
- the fix requires changing PROXY preamble semantics;
- the compatibility pipeline would need a second trust implementation.

## 15. Closure evidence required

- The trust-matrix test results, including the real Unix listener and the
  caller-owned driver cases.
- A before/after demonstration of the spoofed-header behavior on a
  caller-owned stream.
- Confirmation that raw peer/Host/target are unchanged.
- Documentation and migration-guide diffs.
- Confirmation that no public field was added, or the recorded semver
  decision.

## 16. Handoff notes

- `ConnectionContext` fields are public at this baseline; check for
  external-style struct literals inside the workspace before choosing any
  field-based design.
- The direct and compatibility pipelines are two consumers; a helper in
  `eggserve-primitives` or a shared server helper is acceptable, but the
  trust decision must remain one implementation.
- Do not “fix” this by removing `trust_unix`; it is a supported Unix
  listener feature.
- The Python `lowlevel` path passes the flag through; no Python-side
  change is expected beyond tests/documentation.

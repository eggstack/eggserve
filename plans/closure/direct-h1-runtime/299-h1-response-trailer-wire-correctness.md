# Direct H1 Runtime Milestone 299 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/direct-h1-runtime/299-h1-response-trailer-wire-correctness.md`

Source subsystem roadmap:

- `plans/subsystems/direct-h1-runtime-roadmap.md#milestone-8--h1-response-trailer-wire-correctness`

Repository baseline reviewed: `c1f4348939406c02f9739cdedff3bf7568ce09c6`

Implementation commits or pull requests:

- (this commit) — Plan 299 H1 trailer wire repair: neutral declaration,
  runtime-owned H1 head synthesis, adapter enforcement, Tower bridge,
  raw-wire suites, docs

## 1. Executive finding

F3 is repaired at the single direct H1 authority with no redesign of
request trailers, interim responses, H2/H3 paths, or the deferred Tower
rendezvous. An opted-in HTTP/1.1 client (`TE: trailers`) receiving a
response with a valid head-time declaration now observes the terminal
trailer section on the raw wire; HTTP/1.0, non-opted-in H1.1, HEAD/
body-forbidden responses, and undeclared H1 trailer sources are suppressed
before polling with explicit diagnostics. Native, direct-Tower/Axum, and
core compatibility H1 converge. No publication from this milestone.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Raw H1.1 wire carries terminal trailers when opted in + declared | `h1_response_trailers_299.rs`: unknown-length + known-length tests assert `Trailer: x-end` head, no `Content-Length`, `transfer-encoding: chunked`, `x-end: yes` after zero chunk | pass | Hyper 1.11.1 locked; head declaration is the encoder requirement |
| Known-length trailer responses use legal framing | known-length test: wire omits `Content-Length`, chunked + trailer present; adapter retains byte-count validation | pass | Internal length safety preserved, wire framing legal |
| H1.1 without `TE: trailers` suppresses without polling | `h1_11_without_te_suppresses_without_polling` (AtomicBool never set, no `Trailer` head, no wire section) | pass | — |
| H1.0 never emits trailers | `h1_10_never_carries_trailers` (no head/section, no poll) | pass | — |
| HEAD / 204 / 304 do not poll or advertise | `head_and_body_forbidden_do_not_advertise_or_poll` (204, 304, HEAD) | pass | Normalization drop preserved; no advertisement |
| Forbidden/malformed declarations rejected before commitment | `declaration_validation_rejects_forbidden_and_malformed` + Tower bridge `Err` on bad `Trailer` header | pass | Denylist + name rules + bounds; dedup deterministic |
| Actual undeclared trailer fails closed without leak | `undeclared_actual_field_fails_closed_without_leak` (no `x-evil` bytes on wire) | pass | Subset enforcement in `ResponseStreamAdapter`; sanitized diagnostics |
| Duplicate legal values preserve order | `duplicate_trailer_values_preserve_order` | pass | Canonical `HeaderBlock` order preserved |
| Native vs Tower vs core-compat convergence | server `tower_declared_trailers_converge_with_native` + `tower_without_declaration_is_suppressed` + core `h1_response_trailers_299.rs` parity test | pass | Tower `Trailer` header is declaration-request only, regenerated under runtime authority |
| H2/H3 protocol-native behavior unchanged | `trailers_interim` 26/26 core; http2/http3 lib lanes green; no H2/H3 source change | pass | Full http2 integration suite timed out in this env (see §4); lib + focused suites green |
| No second framing authority / buffering / early poll / custom encoder | declaration is names-only metadata; future polled once after body; no body buffering; Hyper selects framing | pass | Topology gate green |
| Semver/release disposition recorded | §7 | pass | No publication from 299 |

## 3. Production implementation evidence

- `eggserve-primitives::trailers::TrailerDeclaration` (new, transport-neutral,
  Hyper/Tokio/Tower-free): bounded names-only declaration (`32` names /
  `8 KiB` aggregate, canonical name rules + trailer denylist, deterministic
  case-insensitive dedup, `from_names` / `parse_header_value` /
  `header_value`); exported from `primitives` and core compat facade.
- `ResponseStream`: additive `with_declared_trailers` /
  `with_known_length_and_declared_trailers`, `trailer_declaration()` /
  `take_trailer_declaration()` / `into_parts_with_declaration()`; legacy
  `with_trailers` / `with_known_length_and_trailers` source-compatible
  (H2/H3 unchanged; H1 without declaration suppressed, not silently lost);
  `Response::strip_response_trailers()` drops declaration + future without
  polling; `response_trailer_declaration()` accessor.
- `eggserve-server::adapters::ResponseStreamAdapter`: carries the declaration
  through `into_parts_with_declaration`; actual terminal fields must be a
  declared subset, else committed-response failure (truncated close,
  sanitized `ResponseStreamProducerError` event, no second response, no
  metadata leak).
- `eggserve-server` H1 pipeline: trailer policy keeps only
  (allowed + payload-permitting + non-HEAD + declared); all other
  trailer-bearing responses strip before polling with
  `ResponseTrailerSuppressed` diagnostics; kept responses synthesize the
  runtime-owned `Trailer` head and remove wire `Content-Length` after
  normalization via `normalize_then_convert_with_h1_trailer_head`
  (Hyper selects chunked; internal byte-count checks unchanged).
- `eggserve-server::interop::response_from_http_body`: Tower bridge consumes
  the ecosystem `Trailer` header as a validated declaration request
  (malformed/forbidden fail before commitment), strips it with other
  framing headers, stores neutral names, and builds declaration-aware
  streams; no-declaration Tower trailers take the legacy path (H1
  suppressed, H2/H3 native).
- `eggserve-core` compatibility forwarding only: same H1 policy (synthesis
  gated to H1 versions so H2/H3 stay protocol-native), `normalize_then_convert_with_h1_trailer_head`
  helper, `TrailerDeclaration` re-export; no second H1 implementation.
- No new crate, no `RuntimeConfig` knob, no CLI/Python surface change.

## 4. Verification executed

### Commands run

```bash
cargo test -p eggserve-server --no-default-features --features tower
cargo clippy -p eggserve-server --no-default-features --features tower --lib --tests -- -D warnings
cargo test -p eggserve-server --test h1_response_trailers_299
cargo test -p eggserve-server --no-default-features --features tower --test h1_response_trailers_299
cargo test -p eggserve-core --test h1_response_trailers_299
cargo test -p eggserve-core --no-default-features --features tower --lib
cargo test -p eggserve-core --no-default-features --features http-interop --lib
cargo test -p eggserve-server --no-default-features --features tower --test interop_http_tower
cargo test -p eggserve-core --test trailers_interim
cargo test -p eggserve-core --features http2,tls --lib
cargo test -p eggserve-core --features http3,tls --lib
cargo clippy --workspace --lib --bins --tests -- -D warnings
cargo test --workspace
cargo fmt --all -- --check
python3 scripts/verify-conformance-matrix.py
python3 scripts/check-crate-topology.py
cargo check --manifest-path crates/eggserve-python/Cargo.toml --locked
```

### Results

- server tower lane: 170 passed, 3 ignored.
- new server wire suite: 9 passed (default) / 11 passed (`tower`).
- new core parity test: 1 passed.
- core tower lib: 154 passed; core http-interop lib: 154 passed.
- server interop tower: 16 passed; core `trailers_interim`: 26 passed.
- core http2 lib: 164 passed; core http3 lib: 156 passed.
- workspace clippy: clean; workspace tests: 1998 passed, 5 ignored.
- fmt, conformance matrix (51 + 55 + 17), topology gate, Python crate
  check: green.
- Not run to completion in this environment: full `verify.sh fast`
  (exceeded 10 min runner budget after fmt gate) and the full
  `eggserve-core --features http2,tls` integration suite (exceeded 10 min).
  No failures observed; lib + focused lanes covering the touched
  primitives/H1 paths are green. No `verify.sh full`/`deep` (not required
  by the plan beyond affected lanes; raw-TCP wire evidence is included).

## 5. Invariant review

- EggServe remains the sole H1 framing authority: applications never set
  `Transfer-Encoding`/wire `Trailer`; the only advertisement is runtime
  synthesized after normalization. Evidence: normalization still strips
  hop-by-hop; interop strips + regenerates; pipeline owns synthesis.
- Canonical trailer validation stays single-source (`Trailers` validator +
  `is_forbidden_trailer_field` reused by declaration). No duplicate
  forbidden registry. Evidence: topology gate + code review.
- Declaration metadata bounded/validated before commitment (count/bytes/
  denylist/name rules). Evidence: unit test + bridge `Err` path.
- Actual fields cannot escape the declared set; post-commit violation fails
  closed without a second response or leak. Evidence: negative wire test.
- No full-response buffering, no early trailer-future poll, no custom H1
  encoder. Evidence: adapter polls future once after body EOF.
- HTTP/1.0 never emits; H1.1 without `TE: trailers` never emits and does
  not poll; HEAD/body-forbidden never poll and never advertise. Evidence:
  matrix tests with poll flags.
- H2/H3 unchanged (no source change; existing suites green).
- Native/Tower/core-compat converge on the single direct H1 authority
  (core forwarding only). Evidence: parity tests + topology gate.
- One-shot bodies, `OpsContext` logging (no `println!`), sanitized
  diagnostics. Evidence: workspace clippy + event usage.

## 6. Failure and recovery review

- Client disconnect during data/trailer phase drops the adapter (cancelled
  counter via `Drop`), releasing body + trailer producers. Existing
  cancellation accounting unchanged.
- Trailer future error/panic after commitment closes/truncates with
  sanitized diagnostics (existing `fail_producer`/`fail_panic` paths;
  undeclared-subset violation reuses the producer-error path with an
  explicit warn event).
- Shutdown with active trailer-bearing streams follows the existing
  graceful-drain path (no new tasks/locks/channels for declaration).
- Keep-alive after successful trailer responses verified implicitly by the
  suite's `Connection: close` exchanges plus the unchanged workspace
  keep-alive coverage; repeated trailer connections covered by sequential
  suite cases.
- No response waits for trailer values before sending body bytes
  (declaration is names-only, available at commitment).

## 7. Migration and compatibility review

- Source-compatible: existing `with_trailers` /
  `with_known_length_and_trailers` compile unchanged (used by Python sync
  handler and legacy callers). H2/H3 semantics for those constructors are
  unchanged.
- Behavior change (documented, H1-only): H1 wire delivery now requires
  anticipated names via `with_declared_trailers` /
  `with_known_length_and_declared_trailers` (native) or an ecosystem
  `Trailer` header (Tower). H1 trailer sources without a declaration are
  suppressed before polling with `ResponseTrailerSuppressed` diagnostics
  instead of delivering body without trailers. Migration path documented in
  `docs/http-primitives.md` + `docs/http-interop.md`.
- Tower adapter contract: ecosystem `Trailer` is declaration-request input,
  never raw framing; documented as such.
- Semver: additive public API in `eggserve-primitives` (`TrailerDeclaration`,
  declaration-aware constructors/accessors) is a minor-level edge; no
  breaking signature change (`into_parts` preserved, new
  `into_parts_with_declaration` added). Plan 297 already reserved the next
  `eggserve-server` release as 0.4.0 for a feature-edge change; 299 rides
  that reserved release when cut (no separate bump requested here).
  Core/static/bin/Python versions untouched.
- No publication from this milestone (per plan §5/G).

## 8. Security review

- Declarations and emitted fields bounded (32 / 8 KiB); denylist
  (framing/routing/connection/auth/expectation) enforced at declaration and
  unchanged at terminal validation. Negative tests cover
  `content-length`/`transfer-encoding`/`trailer`/`te`/`connection`/
  `host`/`upgrade`/proxy-auth/`expect` plus malformed names and header
  injection via comma formatting (name validation rejects separators).
- No user-supplied framing reaches the wire: `Transfer-Encoding` stays
  stripped/ignored; `Trailer` regenerated by the runtime from validated
  names only.
- Undeclared actual fields fail closed with no metadata reflection (wire
  test asserts absence of the undeclared value).
- `docs/threat-model.md` + `docs/security-policy.md` unchanged (no
  new boundary; safe defaults preserved).

## 9. Documentation and operations

- Updated: `docs/http-primitives.md` (declaration contract; corrected the
  old "Trailer header naming is omitted when not knowable (allowed)" line,
  which the Hyper 1.11.1 evidence proves wrong for supported H1 wire
  behavior), `docs/http-interop.md` (Tower declaration bridge),
  `architecture/runtime.md` (commitment contract note),
  `architecture/primitives-api.md` (new API rows),
  `architecture/testing-and-conformance.md` (new suites).
- Regression guard: `h1_response_trailers_299.rs` (server, 11 tests) fails
  if wire trailers disappear while terminal frames exist (declared
  opted-in cases assert wire presence); core parity test guards forwarding.
- Operator surface: none (no CLI/Python/config change). Rust embedders use
  the new declaration-aware constructors for H1 wire delivery.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Full `verify.sh fast` + full http2 integration suite not run to completion in this environment (runner time budget) | No signal of regression (all touched-path lanes green), but not the full gate in one invocation | Re-run `./scripts/verify.sh fast` in CI or a longer session before any release cut |
| low | Keep-alive reuse spectral (multiple trailer responses per connection) covered by existing keep-alive suites, not a dedicated 299 multi-trailer keep-alive case | Bounded; single-response wire + suppression evidence is direct | Optional follow-up case if a future plan touches connection reuse |

No medium+ findings. No new corrective plan required.

## 11. Roadmap disposition

Milestone closed. The direct-H1 subsystem returns to closed status for the
299 scope (roadmap Milestone 8 → closed; subsystem may close once the
registry reflects it). No follow-on trailer redesign required. The deferred
Tower trailer-rendezvous optimization (Plan 295) stays deferred and untouched.

## 12. Registry updates

- `plans/registry.md`: 299 → closed (closure record link); direct-H1
  subsystem → closed; blocked work stays empty.
- `plans/subsystems/direct-h1-runtime-roadmap.md`: Milestone 8 → closed;
  status → closed; current-state notes 299 completion.

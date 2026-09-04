# Plan 173 — Octet-Preserving Canonical HTTP Metadata

## Status

**IMPLEMENTED / CLOSED.**

Prerequisite: Plan 172 present. Plans 161–171 remain closed historical work.

## Closure record

Tracks A–E implemented on `main`:

- `HeaderValue` is octet-preserving (`Bytes` storage, `from_bytes` /
  `from_static_bytes` / `as_bytes()`, fallible `to_str()` → 
  `HeaderValueTextError`; `new(str)` retained as alias). Validation matches
  `http::HeaderValue::from_bytes` (`HTAB`, `SP`–`~`, obs-text; rejects `CR` /
  `LF` / `NUL` / `DEL` / `CTL`s). `OWS` (`SP`/`HTAB`) stripping is a deliberate
  canonical invariant for both text and byte constructors. `HeaderBlock` stays
  ordered/duplicate-preserving with `push_bytes(..)`. `Display` is lossy
  diagnostic only.
- Inbound (`RequestHead::try_from_hyper`, connection `convert_request_head`)
  and outbound (`to_hyper_response`, error helpers) conversions preserve exact
  octets; protocol headers use checked `to_str()` at interpretation.
  `tests/octet_fidelity.rs` proves wire → Hyper → service and service →
  normalization → wire preservation (incl. duplicates) over the duplex parser
  path, plus `CR`/`LF`/`NUL`/`DEL` rejection and byte-counted limits.
- Python stdlib-shaped surfaces stay text-only (opaque headers omitted, not
  coerced); Rust canonical layer is byte-correct.
- `RequestTarget` gains truthful `raw_bytes()` / `path_bytes()` /
  `query_bytes()` over accepted origin-form bytes; `/path` vs `/path?`
  deliberately canonicalizes to `None`; wire corpus regression-tested.
- Breaking `as_str()` → `to_str()` migration documented in
  `docs/migration-guide.md`; stability inventory and primitives docs updated.
  Security review points hold (existing smuggling/framing/privacy suites pass;
  logs omit raw hostile bytes).

## Goal

Make EggServe's canonical application-facing HTTP metadata truthful for general downstream server use by removing mandatory UTF-8 conversion from header field values and by explicitly qualifying request-target byte fidelity.

This is an HTTP primitive correction, not an ASGI feature. ASGI is the reference consumer because its HTTP scope exposes headers and query data as bytes and requires duplicate header preservation, but the resulting API must remain useful to native Rust services and other protocol adapters.

## Current problem

`HeaderBlock` has the right collection model: it is ordered, duplicate-preserving, and case-insensitive for lookup. The weak point is `HeaderValue`, which currently stores `String` and exposes `as_str()`.

The Hyper-to-canonical adapter currently converts each incoming `http::HeaderValue` using `to_str()`. Legal opaque field values that contain non-ASCII octets are therefore rejected before the downstream `Service` runs.

That is narrower than the underlying HTTP library and narrower than a general HTTP application-server boundary should be. Hyper/http header values are byte-oriented and can contain valid opaque bytes that are not representable as a Rust UTF-8 `str`.

The same text-only representation also prevents a downstream application server from returning an otherwise valid opaque response field value through canonical `Response` without lossy encoding or rejection.

## Design principle

The canonical representation should preserve the validated HTTP field-value octets. Text interpretation is optional application behavior.

Target model:

```rust
pub struct HeaderValue {
    bytes: bytes::Bytes,
}

impl HeaderValue {
    pub fn from_bytes(value: impl Into<Bytes>) -> Result<Self, HeaderError>;
    pub fn from_str(value: impl AsRef<str>) -> Result<Self, HeaderError>;
    pub fn as_bytes(&self) -> &[u8];
    pub fn to_str(&self) -> Result<&str, HeaderValueTextError>;
}
```

The exact storage type may be `Bytes`, `Vec<u8>`, or a small EggServe-owned wrapper chosen after allocation/API review. The semantic contract is more important than the concrete storage: values are bytes; string conversion is fallible.

Do not use `String::from_utf8_lossy`, Latin-1 coercion, percent encoding, or silent replacement as the canonical representation.

## Track A — Establish the exact legal byte contract

### A1. Align validation with HTTP field-value semantics

Review the canonical validation rules against the `http` crate and RFC 9110/HTTP/1 parsing behavior.

Requirements:

- reject CR, LF, NUL, DEL, and any byte the underlying HTTP transport refuses as an HTTP field value;
- preserve legal visible/opaque octets without UTF-8 interpretation;
- retain current optional-whitespace normalization only if it is a deliberate canonical invariant and does not destroy significant field-value bytes;
- do not weaken request-smuggling/header-injection protections;
- keep field-name validation ASCII token-based.

Prefer sharing/converging on the same accepted byte domain as the transport adapter so canonical construction cannot create values that later fail at wire conversion.

### A2. Determine whitespace preservation policy

Current `HeaderValue::new()` strips leading/trailing SP/HTAB. Before changing storage, verify whether this is still desirable for a generic canonical boundary.

HTTP field-line parsing removes framing whitespace around the field value, but application-produced response values should not undergo surprising additional normalization after construction.

Choose and document one rule:

- canonical values represent the parsed field value after transport-level OWS removal; or
- canonical values preserve exactly the validated bytes supplied by application code, while inbound parsing reflects what Hyper provides.

Do not retain trimming merely because the old string constructor did it.

## Track B — Evolve `HeaderValue` and `HeaderBlock`

### B1. Add byte-native construction/access

Provide a primary byte constructor and byte accessor.

Expected capabilities:

```rust
HeaderValue::from_bytes(...)
HeaderValue::from_static_bytes(...)?
value.as_bytes()
value.to_str() -> Result<&str, ...>
```

A convenient string constructor should remain for ordinary services.

Avoid proliferating nearly identical `RawHeaderValue` and `HeaderValue` types. One canonical type should represent HTTP field values.

### B2. Preserve collection semantics

`HeaderBlock` must continue to preserve:

- field order;
- duplicate fields;
- duplicate value order;
- case-insensitive field-name lookup;
- original/best-effort field-name casing according to the existing contract.

Do not convert the canonical structure into a map.

Add byte-oriented helpers only where they improve ergonomics without duplicating APIs unnecessarily.

### B3. Text-only call sites

Audit every `as_str()`/string assumption in:

- request-body framing/policy code;
- response normalization;
- Date/Server/privacy policy;
- static serving;
- MIME/cache/range handling;
- Python adapters;
- CLI/logging/ops events;
- tests and examples.

Protocol-defined headers that are required to be ASCII/text should perform explicit checked conversion at the point of semantic interpretation. Generic forwarding/storage must remain byte-preserving.

Do not make arbitrary opaque values printable in logs. Sanitized logs should either omit them or use an explicitly bounded escaped/hex diagnostic representation.

## Track C — Hyper conversion fidelity

### C1. Inbound conversion

Replace `HeaderValue::to_str()`-based canonical conversion with byte-preserving conversion from the underlying HTTP header value.

Add real wire tests containing legal non-UTF-8/obs-text bytes where Hyper accepts them. Prove that:

```text
wire bytes -> Hyper -> EggServe HeaderBlock -> Service
```

preserves the field value bytes and duplicate ordering.

Do not fabricate a test through only `HeaderBlock::from_bytes`; exercise the actual connection parser path.

### C2. Outbound conversion

Ensure canonical response headers convert back to `http::HeaderValue` using the byte API and preserve exact canonical bytes.

Add a custom-service wire test proving:

```text
Service Response HeaderValue bytes -> EggServe normalization -> HTTP/1 wire
```

without UTF-8 coercion.

Retain final-boundary stripping/normalization for hop-by-hop/framing/privacy-controlled fields. Byte preservation does not mean bypassing EggServe response policy.

## Track D — Python compatibility

The existing Python APIs may reasonably expose header values as strings in stdlib-shaped surfaces. Do not silently change those APIs to Python `bytes` unless their documented contract already permits it.

Separate two layers:

- Rust canonical primitives are byte-correct;
- Python compatibility facades may enforce their own text subset and reject values they cannot represent.

If `eggserve.lowlevel` is intended as a future generic downstream substrate, audit whether it should gain explicit bytes-capable header access additively. Do not make that expansion a prerequisite for a separate Rust/PyO3 downstream application-server project.

## Track E — Request-target fidelity qualification

### E1. Measure before redesigning

`RequestTarget` currently stores a raw `String`, path `String`, and optional query `String`. ASGI's `raw_path` is optional, and an HTTP/1 origin-form target is normally representable as visible ASCII/percent-encoded data. Therefore do not replace this type speculatively.

Build a wire-level corpus for accepted request targets covering at minimum:

- percent-encoded UTF-8;
- percent-encoded arbitrary octets;
- mixed-case percent escapes;
- repeated separators;
- empty and non-empty query strings;
- reserved characters accepted by Hyper/EggServe;
- non-ASCII literal octets if the parser accepts any;
- encoded slash/question-mark boundaries;
- malformed targets that must remain rejected.

Compare original wire target bytes with the canonical `RequestTarget::raw()`/path/query representation.

### E2. Add truthful byte accessors where possible

If accepted origin-form targets round-trip losslessly through the current `String` representation, add non-allocating byte accessors such as:

```rust
pub fn raw_bytes(&self) -> &[u8]
pub fn path_bytes(&self) -> &[u8]
pub fn query_bytes(&self) -> Option<&[u8]>
```

These accessors may simply expose the UTF-8 storage bytes if and only if that is exactly the accepted wire representation.

If Hyper normalizes some accepted target before EggServe sees it, document that the canonical API cannot truthfully provide original raw-path bytes for those cases. A downstream ASGI server should then omit optional `raw_path` rather than fabricate it.

Do not introduce a second raw request-target parser or bypass Hyper solely to satisfy optional metadata.

### E3. Empty query distinction

Review `/path` versus `/path?` semantics. The current `RequestTarget` maps an empty query to `None`. ASGI `query_string` is bytes and can represent `b""` but does not itself distinguish the presence of a bare trailing `?`; most application semantics do not require this distinction.

Document whether EggServe deliberately canonicalizes the two forms. Preserve the distinction only if there is a concrete HTTP/application-server need and the wire parser makes it cheap and truthful.

## Public API migration

This plan likely changes the semver-considered `HeaderValue` API. Follow Plan 171's pre-1.0 policy.

Preferred compatibility strategy:

- retain `HeaderValue::new(str)` as a convenience alias/string constructor if possible;
- add `from_bytes()` and `as_bytes()`;
- replace infallible `as_str() -> &str` with fallible `to_str()` or an equivalent explicitly fallible API;
- migrate internal callers in one change set;
- document the source break for callers that assumed all HTTP field values are UTF-8.

Do not keep an infallible `as_str()` by panicking or lossy-decoding opaque bytes.

If this is published after the Plan 171 transition, classify the release according to the repository's current pre-1.0 compatibility policy rather than assuming a patch release.

## Security review points

The byte migration must explicitly test:

- CR/LF injection rejection;
- NUL/control rejection;
- invalid field-name rejection;
- `Content-Length` duplicate/smuggling rules unchanged;
- `Transfer-Encoding` handling unchanged;
- `Connection`/hop-by-hop stripping unchanged;
- response privacy policy unchanged;
- log sanitization does not emit raw hostile bytes;
- aggregate header-byte limits count bytes, not Unicode scalar values;
- no accidental allocation amplification from repeated text conversion.

## Verification

Run at minimum:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p eggserve-core --test public_api_consumers
cargo test -p eggserve-core --test api_stability
cargo test -p eggserve-core --test no_hyper_in_public_api
cargo test -p eggserve-core --test canonical_wire
cargo test -p eggserve-core --test smuggling
cargo check -p eggserve-core --examples
cargo test --doc -p eggserve-core
bash scripts/verify-cargo-packages.sh --mode all
```

Use the repository's actual test target names if they differ; do not create duplicate suites just to match this list.

Also run the installed Python test suite because header primitives cross native adapters, even if Python's documented public types remain text-only.

## Acceptance criteria

- [ ] canonical `HeaderValue` can represent every field-value byte sequence accepted by EggServe's HTTP transport policy without UTF-8 coercion;
- [ ] inbound legal opaque bytes reach `Service` unchanged;
- [ ] outbound legal opaque bytes reach the wire unchanged except for explicit EggServe response policy;
- [ ] CR/LF/control/header-injection protections are unchanged or stronger;
- [ ] header byte limits are still measured in bytes and remain bounded;
- [ ] duplicate fields and ordering are preserved;
- [ ] ordinary text-header ergonomics remain straightforward;
- [ ] text interpretation is explicitly fallible;
- [ ] Python stdlib-shaped compatibility behavior is not silently broadened/broken;
- [ ] request-target wire fidelity is measured, documented, and regression-tested;
- [ ] byte accessors are provided only where they truthfully represent accepted wire data;
- [ ] optional `raw_path` limitations are documented rather than fabricated;
- [ ] API snapshots/migration docs reflect the new primitive contract;
- [ ] no raw Hyper types enter the canonical application-facing API.

## Non-goals

Do not add:

- ASGI scope/event construction;
- URL routing or percent-decoding policy for application frameworks;
- a second HTTP parser;
- HTTP/2 pseudo-header support;
- lossy Unicode normalization of field values;
- generic header deserialization/typed-header framework;
- Python ASGI bytes objects as part of `eggserve-core`;
- WebSocket/upgraded transport support.

## Handoff

Plan 174 should assume byte-correct metadata and focus exclusively on request/application lifecycle ownership. If this plan discovers that request-target fidelity is limited by Hyper, record the limitation and proceed; exact `raw_path` is not important enough to justify piercing the canonical transport boundary.
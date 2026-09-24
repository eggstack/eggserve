# Plan 278 — absolute-form service-dispatch implementation closure

## Disposition

Source implementation and focused local qualification are complete. Registry
publication is intentionally deferred to the combined next release, per
maintainer direction; Plan 277's 0.2.3 artifacts are not claimed as published.
Hosted CI for the final combined change SHA remains part of release closure.

## Implementation

- Added `RequestTargetForm` and a transport-neutral absolute-target projection
  constructor. `RequestTarget::parse` remains origin-form-only.
- Added `Http1RequestTargetMode`, defaulting to `OriginOnly`, to direct H1
  configuration and its builder. Compatibility core explicitly projects
  `OriginOnly`.
- H1 conversion permits only validated absolute-form under explicit opt-in,
  bounds the full semantic URI, validates URI and duplicate Host authorities,
  and retains scheme/authority/path/query metadata.
- StaticService rejects absolute-form before filesystem resolution.
- Added real-listener tests for the default rejection, opt-in projection,
  authority mismatch, and full-target 414 enforcement; added canonical
  constructor coverage. Existing origin-only corpus assertions remain.

## Local evidence

- `cargo check -p eggserve-server -p eggserve-core -p eggserve-static --all-targets` — passed.
- `cargo test -p eggserve-server --test downstream_embedding` — 3 passed.
- `cargo test -p eggserve-primitives request_target` — 37 passed.
- `cargo test -p eggserve-static` — 345 passed, 2 ignored.
- `cargo fmt --all` — passed.

## Release carry-forward

Plan 279's exact crates.io-only consumers and publication evidence are deferred
to the combined publication owned by Plan 286. No EggServe proxy routing or
outbound forwarding is introduced. The next source plan is Plan 280.

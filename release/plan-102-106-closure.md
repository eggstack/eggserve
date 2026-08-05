# Superseded historical record: Plans 102–106

Plan 107 reopens the runtime streaming, ownership, request-body, release-smoke,
and evidence claims recorded below. This file is retained as history; the
current truth is the implementation and hosted checks for Plan 107.

## Final commit

SHA: 45aaefa

## Plans implemented

- **Plan 102** — Corrective track roadmap (Plans 103–106)
- **Plan 103** — CLI logging modes, semaphore bounds, listing limits, index fallback, body drain, Python metadata
- **Plan 104** — Runtime/service ownership separation, body policy layering
- **Plan 105** — Product-surface freeze, binary-size reduction, dist profile
- **Plan 106** — Verification simplification, documentation reconciliation, roadmap closure

## Key defects fixed

- `--log-format none` now produces no output (NopLogSink)
- `--quiet` wraps sink with FilteredLogSink (warn/error only)
- Semaphore construction validated against `MAX_PERMITS` — cannot panic from config
- Listing entry limits honest and enforced
- `index.htm` fallback consistent with `index.html`
- Rejected bodies close connection without fixed draining
- Request conversion never silently changes semantics
- Static and custom body policy layered correctly (service-declared, runtime-enforced)
- Custom services require no static root
- One transport path serves static and custom services
- One runtime file-stream semaphore governs canonical file responses
- Every retained runtime field is effective

## Removed public configuration fields

None removed — all existing fields retained. Plan 105 froze the product surface.

## Final runtime/static ownership summary

- **Static service**: owns path confinement, policy enforcement, file resolution
- **Runtime**: owns sockets, framing, deadlines, connection admission, file-stream admission
- **Custom services**: no implicit filesystem root; body policy declared per-request
- **Service body policy**: honored within runtime limits (Reject/Buffer/Stream)

## Accepted size optimizations

- Default CLI excludes optional TLS/client code via feature gating
- dist profile: `opt-level = "z"`, fat LTO, single codegen unit, symbol stripping
- Measurements recorded in `benchmarks/088-baseline/`

## CI/fuzz work removed, retained, or moved to manual

### Removed from routine CI
- `--all-targets` flag (was compiling benchmarks/examples accidentally)
- `cargo test -p eggserve-core --features client-tls` (moved to `verify.sh full`)

### Retained in routine CI
- Format check, clippy (`--lib --bins --tests`), workspace tests
- Server TLS lint and tests

### Fuzz targets consolidated
21 targets → 12 targets by merging redundant parsers:
- `fuzz_header_name` + `fuzz_header_value` → merged into `fuzz_header_block`
- `fuzz_method` → merged into `validate_method`
- `fuzz_status_code` + `fuzz_response_builder` + `fuzz_content_length_reconciliation` → merged into `fuzz_normalize_response`
- `validate_request_target` + `fuzz_request_head` → merged into `request_target`
- `fuzz_event_serialization` → removed (not security-critical)

### Release workflow
- Added per-platform installed-wheel smoke tests (import, binary, --version, loopback GET)

## Local commands and results

```sh
# Format
cargo fmt --all -- --check                                  # PASS

# Clippy
cargo clippy --workspace --lib --bins --tests -- -D warnings  # PASS (0 warnings)
cargo clippy -p eggserve-bin --features tls --lib --bins --tests -- -D warnings  # PASS

# Tests
cargo test --workspace                                      # 1423 passed, 10 ignored
cargo test -p eggserve-bin --features tls                   # 88 passed

# verify.sh fast
./scripts/verify.sh fast                                    # PASS
```

## Hosted job results

- **rust** job: ✓ PASS in 3m28s (format, clippy, workspace tests, TLS lint and tests)
- **python** job: ✓ PASS in 3m46s (build CLI, stage binary, build wheel, install, import boundary, smoke, test suite)

## Remaining documented limitations

- **Windows**: functional but not hardened for untrusted content. Adversarial qualification test scaffold established (Plan 086, 114 tests). Independent adversarial review incomplete.
- **Follow-symlinks**: weaker than default symlink-denied mode. Uses canonicalize-based resolution outside descriptor-relative hardening guarantee.
- **HTTP/2, redirects, retries, cookies, proxy, multi-range**: outside scope.
- **Python wheels**: CPython 3.14 only (`>=3.14,<3.15`).

## Explicit statements

- No automated publication or scope expansion was introduced
- Routine CI has exactly two jobs (rust, python)
- Python CI tests the installed wheel, not the source checkout
- Deep verification remains manual and environment-aware
- Release builds and publication are manual
- No scheduled workflows, no OIDC publication, no SBOM generation

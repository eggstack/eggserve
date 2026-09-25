# Policy System — Deep Dive

eggserve uses a layered policy system to control what can be served. Policies are checked at multiple stages: path validation, filesystem resolution, and response construction. Final origin-response metadata is an explicit runtime policy (Plan 165) applied after service construction so applications cannot bypass it.

## Policy Types

### `StaticPolicy` (`eggserve-primitives/src/primitives/policy.rs`)

The top-level composite policy. Aggregates filesystem sub-policies plus the
static validator policy. (The parse-level `PathPolicy`/`DotfilePolicy` live
separately in `eggserve-static/src/path/policy.rs`; see below.)

```rust
pub struct StaticPolicy {
    pub directory_listing: DirectoryListingPolicy,
    pub symlinks: SymlinkPolicy,
    pub dotfiles: DotfilePolicy,
    pub static_metadata: StaticMetadataPolicy,
}
```

`StaticPolicy::safe_default()` returns the most restrictive filesystem
configuration plus standard validators:
- `DirectoryListingPolicy::Disabled`
- `SymlinkPolicy::Denied`
- `DotfilePolicy::Denied`
- `StaticMetadataPolicy::standard()` (emit `ETag` + `Last-Modified`)

### `StaticMetadataPolicy` (Plan 165)

Controls filesystem-derived validators on static responses. Default emits
both; `minimal_fingerprint()` suppresses both to avoid disclosing host/content
timestamp characteristics. Suppression is preferable to content hashing (no
unbounded startup/read cost). When `Last-Modified` is retained, the final
boundary drops it when it would be later than `Date`.

### `ErrorRepresentationPolicy` (Plan 165)

`Minimal` (default) emits fixed generic plain-text bodies with fixed
`Content-Type` and no version/path/exception detail. `Empty` emits no body
bytes for runtime-generated errors. Application `Ok` 4xx/5xx bodies are never
rewritten; only runtime-constructed errors are affected. `HEAD` suppression
remains correct.

### `ResponsePolicy` (`crates/eggserve-server/src/response_policy.rs`, Plan 165)

Final-boundary origin policy applied after service/static construction and
canonical normalization but before bytes are emitted. This is the
`eggserve-server` crate path — not core `server/`. No service/frontend may
bypass it with raw Hyper responses. Fields:

```text
ResponsePolicy
  server_identification: Option<String>  // None = suppressed (default)
  date_policy: DatePolicy                // SystemClock (default) | Custom | Suppress
  stripped_response_headers: Vec<String> // validated denylist, post-service
  error_policy: ErrorRepresentationPolicy
```

- **Server:** suppressed by default; optional fixed value. Never emits crate,
  Rust, Hyper, OS, TLS, or Python versions. Application `Server` is
  subordinate.
- **Date:** EggServe is the sole authority; Hyper `auto_date_header(false)`.
  `SystemClock` preserves one-`Date` compatibility. `Custom(provider)` uses a
  caller-supplied trusted time value (EggServe owns formatting/validation).
  `Suppress` is an explicit RFC 9110 tradeoff (origin with a clock should send
  `Date` on 2xx/3xx/4xx). No fixed/stale or randomized dates.
- **Denylist:** validated names, removed after service construction (all
  duplicates). Framing/hop-by-hop/`date`/`content-range` cannot be denylisted;
  runtime-required headers cannot be removed when it would make the response
  invalid. Built-in `minimal_fingerprint()` preset strips `x-powered-by`
  (plus `Server` suppression); caller extends for project fields. No wildcard.
- **Profile:** `ResponsePolicy::minimal_fingerprint()` + 
  `StaticMetadataPolicy::minimal_fingerprint()` + stricter Plan 164 limits is
  the generic minimal-fingerprint origin. It minimizes gratuitous signals, it
  does not claim un-fingerprintability. The router/WAF owns rate limiting;
  this is origin hardening only. No I2P types in core.

### `DirectoryListingPolicy`

```rust
pub enum DirectoryListingPolicy {
    Disabled,
    Enabled,
}
```

Controls whether directory listing HTML is returned for directory requests. Default: `Disabled`.

### `SymlinkPolicy`

```rust
pub enum SymlinkPolicy {
    Denied,
    Follow,
}
```

Controls whether symlinks are followed during filesystem resolution. Default: `Denied`. When `Denied`, the descriptor-relative traversal on Unix refuses symlinks at both `statat` and `openat` time.

### `DotfilePolicy` (serving level)

```rust
pub enum DotfilePolicy {
    Denied,
    Serve,
}
```

Controls whether dotfiles (paths containing components starting with `.`) are served. Default: `Denied`.

## The Two DotfilePolicy Types

This is a critical architectural detail:

| Type | Location | Variants | Controls | When Checked |
|------|----------|----------|----------|--------------|
| `path::DotfilePolicy` | `eggserve-static/src/path/policy.rs` | `{Denied, Allow}` | Whether dotfile paths are *accepted* during parsing | Path validation stage |
| `policy::DotfilePolicy` | `eggserve-primitives/src/primitives/policy.rs` | `{Denied, Serve}` | Whether dotfiles are *served* in responses | Response stage |

Both must agree for dotfiles to be served. This double-check ensures:
1. Dotfile paths are rejected early (before filesystem access) if path-level policy denies them
2. Even if a dotfile path somehow reaches the filesystem layer, the serving-level policy still denies it

## Policy Flow

```
Request arrives
    │
    ▼
Path Validation
    ├── path::DotfilePolicy → reject dotfile paths
    ├── path::reject_backslash → reject backslashes
    └── (other path checks)
    │
    ▼
Filesystem Resolution
    ├── SymlinkPolicy → deny symlinks (descriptor-relative)
    └── Root confinement → deny escapes
    │
    ▼
Response Construction
    ├── DotfilePolicy (serving) → deny dotfiles
    ├── DirectoryListingPolicy → deny/allow listing
    └── (other response checks)
```

## Safe Defaults

Every policy defaults to the most restrictive setting:

| Policy | Default | Effect |
|--------|---------|--------|
| `DirectoryListingPolicy` | `Disabled` | No directory listing HTML |
| `SymlinkPolicy` | `Denied` | No symlink following |
| `DotfilePolicy` (path) | `Denied` | Dotfile paths rejected early |
| `DotfilePolicy` (serving) | `Denied` | Dotfiles not served |
| Bind address | `127.0.0.1` | Loopback only |
| Request body | rejected | No body processing |

Users must explicitly opt-in to less restrictive behavior via CLI flags or Python config.

## CLI Flag Mapping

| CLI Flag | Policy Field | Effect |
|----------|-------------|--------|
| `--directory-listing` | `DirectoryListingPolicy::Enabled` | Enable directory listing |
| `--follow-symlinks` | `SymlinkPolicy::Follow` | Follow symlinks |
| `--allow-dotfiles` | `DotfilePolicy::Serve` | Serve dotfiles |
| `--public` | Bind to `0.0.0.0` | Accept non-loopback connections |

## Python API Mapping

```python
from eggserve.lowlevel import StaticPolicy

policy = StaticPolicy(
    directory_listing=True,   # → DirectoryListingPolicy::Enabled
    follow_symlinks=True,     # → SymlinkPolicy::Follow
    allow_dotfiles=True,      # → DotfilePolicy::Serve
)
```

All fields default to `False` (most restrictive).

## Security Properties

1. **Default deny** — Every policy starts at the most restrictive setting
2. **Explicit opt-in** — Less restrictive behavior requires explicit flags
3. **Layered enforcement** — Policies are checked at multiple stages (path, filesystem, response)
4. **No silent overrides** — Security defaults cannot be overridden without user intent
5. **Double dotfile check** — Path-level and serving-level dotfile policies must both agree

## External Deadline/Ceiling Ownership (Plans 280/282/283)

Application-facing H1 deadlines and semantic ceilings carry explicit
ownership in `eggserve-server/src/config.rs`: `PolicyOwner::{EggServe,
External}` per knob (`H1PolicyOwnership`: handler/request-body/keep-alive-
idle/response-write-progress deadlines plus global body and target
ceilings). Each direct connection consumes a narrow `H1ConnectionPolicy`
projection built via `RuntimeConfig::h1_connection_policy()` (listener,
pool, and TLS-handshake settings intentionally absent). Runtime rejections
are presented, not formatted, by downstream code: the presentation-only
`RuntimeRejectionPresenter` in `eggserve-server/src/rejection.rs` maps a
`RuntimeRejection` (kind + status facts only, no request/transport/error
text) to an optional bounded `RuntimeErrorPresentation` (application
headers + ≤64 KiB body); "status/disposition/framing/privacy stay
EggServe-owned; parser buffer/header-count mandatory."

## See Also

- [path-confinement.md](path-confinement.md) — Path-level policy enforcement
- [filesystem-confinement.md](filesystem-confinement.md) — Symlink policy in filesystem traversal
- [eggserve-core.md](eggserve-core.md) — Policy module location

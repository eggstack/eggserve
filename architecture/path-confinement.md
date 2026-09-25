# Path Confinement — Deep Dive

The path confinement pipeline validates and normalizes every incoming request target before it touches the filesystem. A `ConfinedPath` cannot be constructed without passing through the full pipeline.

> **Authority (Plan 219).** The pipeline below is implemented once in
> `eggserve-static` (`src/path/`). `eggserve-core` keeps compatibility
> facades only (`primitives::{ConfinedPath, PathPolicy, PathRejection, ...}`
> re-export the static types; `src/path/` is deleted).

## Pipeline Stages

`RequestTarget::parse()` is the origin-form HTTP request-target classifier
(absolute-form enters opt-in via `from_absolute_components`, Plan 278). The
runtime passes its validated `path()` component to
`ConfinedPath::from_path_component()`. Direct `ConfinedPath::parse()` remains
the stable convenience adapter for raw target text and delegates
classification to `RequestTarget` before running path-only security stages.

```
Raw Request Target
    │
    ▼
┌─────────────────────────────────┐
│ 1. RequestTarget::parse()       │  Classify origin form and split query
│    primitives/request_target.rs │  reject absolute/authority/asterisk forms
└─────────────────┬───────────────┘
                  │
                  ▼
┌─────────────────────────────────┐
│ 2. percent_decode()             │  Single-pass decode, reject malformed/NUL/invalid UTF-8,
│    path/decode.rs               │  encoded separators (/ and \)
└─────────────────┬───────────────┘
                  │
                  ▼
┌─────────────────────────────────┐
│ 3. normalize_path()             │  Collapse `//`, strip leading slashes;
│    path/components.rs           │  `.` and `..` survive normalization (rejected in stage 5)
└─────────────────┬───────────────┘
                  │
                  ▼
┌─────────────────────────────────┐
│ 4. split_components()           │  Split into path segments
│    path/components.rs           │
└─────────────────┬───────────────┘
                  │
                  ▼
┌─────────────────────────────────┐
│ 5. validate_components()        │  Per-component checks (includes platform checks):
│    path/components.rs           │    - Reject `.` and `..` (and double-encoded variants)
│                                  │    - Reject NUL bytes
│                                  │    - Reject literal `/` or `\` in component
│                                  │    - Reject dotfiles (if policy requires)
│                                  │    - Platform checks (reserved names, ADS, drive prefixes,
│                                  │      trailing dots/spaces) via platform::check_component()
└─────────────────┬───────────────┘
                  │
                  ▼
           ConfinedPath
```

## Module Map

| Module | File | Purpose |
|--------|------|---------|
| `mod.rs` | `path/mod.rs` | `ConfinedPath` type — the validated path |
| `request_target.rs` | `primitives/request_target.rs` | Canonical HTTP target classification |
| `decode.rs` | `path/decode.rs` | Percent decoding |
| `components.rs` | `path/components.rs` | Normalization, splitting, validation |
| `rejected.rs` | `path/rejected.rs` | `PathRejection` enum (17 variants) |
| `policy.rs` | `path/policy.rs` | `PathPolicy`, `DotfilePolicy` (path-level) |
| `platform.rs` | `path/platform.rs` | Windows-specific checks |

## `ConfinedPath`

The output of the pipeline. An opaque, validated type:

```rust
pub struct ConfinedPath {
    decoded: String,           // percent-decoded, normalized
    components: Vec<String>,   // non-empty path segments
    path_policy: PathPolicy,   // retained for downstream resolution
}
```

Methods:
- `as_str()` — The full decoded path string
- `components()` — Slice of path segments
- `path_policy()` — The policy used during validation
- `from_path_component()` — Apply path confinement after a canonical target
  adapter has selected the path component. Fast path: a leading-`/` input
  without `%` or `//` skips decode/normalize (`path/mod.rs:50-59`);
  component validation is still mandatory (same defense-in-depth boundary).
- `parse()` pre-checks (before classification): 8192-byte cap
  (`TooLong`), NUL pre-reject (`NulByte`), then `RequestTarget::parse`
  delegation with whitespace→`UnsupportedUriForm` (`path/mod.rs:20-40`).

## Rejection Types (`PathRejection`)

17 variants covering every possible rejection reason:

| Variant | Stage | Meaning |
|---------|-------|---------|
| `Empty` | parse | Empty request target |
| `TooLong` | parse | Target exceeds 8192 bytes (active pre-check; mapped to 414) |
| `UnsupportedUriForm` | parse | Not origin-form (absolute or authority form) |
| `MalformedPercentEncoding` | decode | Invalid `%XX` sequence |
| `InvalidUtf8` | decode | Decoded bytes are not valid UTF-8 |
| `NulByte` | decode, components | Decoded path contains NUL |
| `ControlCharacter` | decode | Decoded path contains an ASCII control character |
| `AbsolutePath` | (reserved) | Path starts with `/` (after normalization) — reserved variant (`#[allow(dead_code)]` in `rejected.rs`) |
| `ParentComponent` | components | `..` component found |
| `CurrentComponent` | components | `.` component found |
| `SeparatorAmbiguity` | decode, components | Encoded or literal `/` or `\` found |
| `DotfileDenied` | components | Dotfile component (if policy requires) |
| `WindowsPrefixDenied` | platform | Windows drive prefix (`C:\`) |
| `WindowsReservedNameDenied` | platform | Reserved name (`CON`, `NUL`, etc.) |
| `WindowsAlternateStreamDenied` | platform | Alternate data stream (`file:stream`) |
| `SymlinkDenied` | fs | Symlink encountered during traversal |
| `RootEscapeDenied` | fs | Path escapes configured root |

## Path Policy (`path::PathPolicy`)

Controls path-level validation. The parse-level dotfile variants are
`DotfilePolicy::{Denied, Allow}` (`path/policy.rs:12-16`) — not `Serve`
(`Serve` belongs to the serving-level `policy::DotfilePolicy` only):

```rust
pub struct PathPolicy {
    pub dotfiles: DotfilePolicy,       // allow or deny dotfile components
    pub reject_backslash: bool,        // reject `\` in path
}
```

Note: This is distinct from `policy::DotfilePolicy` (serving level). Both must agree for dotfiles to be served.

## Platform Checks (`platform.rs`)

Runs on all platforms, rejecting Windows-specific path patterns:

- **Drive prefixes** — Rejects `C:`, `\\server\share`, etc.
- **Reserved names** — Rejects `CON`, `NUL`, `PRN`, `AUX`, `COM1`–`COM9`, `LPT1`–`LPT9`
- **Alternate data streams** — Rejects `file:stream` syntax
- **Trailing dots/spaces** — Rejects components ending with `.` or ` ` (Windows normalization aliasing)

## Security Properties

1. **No bypass** — A `ConfinedPath` can only be produced by the pipeline. There is no `unsafe` way to construct one.
2. **Deterministic** — Same input always produces the same output (after normalization).
3. **No filesystem access** — Path confinement is pure string manipulation. No `stat()`, no `open()`.
4. **Policy-aware** — Validation is parameterized by `PathPolicy`, but safe defaults deny everything.

## See Also

- [filesystem-confinement.md](filesystem-confinement.md) — What happens after path validation
- [policy-system.md](policy-system.md) — Policy types and enforcement
- [primitives-api.md](primitives-api.md) — Public API for path validation

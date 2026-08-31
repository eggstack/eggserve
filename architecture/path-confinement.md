# Path Confinement — Deep Dive

The path confinement pipeline validates and normalizes every incoming request target before it touches the filesystem. A `ConfinedPath` cannot be constructed without passing through the full pipeline.

## Pipeline Stages

`ConfinedPath::parse()` (`path/mod.rs:21-38`) runs these stages in order:

```
Raw Request Target
    │
    ▼
┌─────────────────────────────────┐
│ 1. parse_origin_form()          │  Strip query string, reject non-origin forms
│    path/request_target.rs       │
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
| `request_target.rs` | `path/request_target.rs` | HTTP origin-form parsing |
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

## Rejection Types (`PathRejection`)

17 variants covering every possible rejection reason:

| Variant | Stage | Meaning |
|---------|-------|---------|
| `Empty` | parse | Empty request target |
| `TooLong` | parse | Target exceeds maximum length |
| `UnsupportedUriForm` | parse | Not origin-form (absolute or authority form) |
| `MalformedPercentEncoding` | decode | Invalid `%XX` sequence |
| `InvalidUtf8` | decode | Decoded bytes are not valid UTF-8 |
| `NulByte` | decode, components | Decoded path contains NUL |
| `ControlCharacter` | decode | Decoded path contains an ASCII control character |
| `AbsolutePath` | (unused) | Path starts with `/` (after normalization) — reserved variant |
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

Controls path-level validation:

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

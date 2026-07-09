# Path Confinement — Deep Dive

The path confinement pipeline validates and normalizes every incoming request target before it touches the filesystem. A `ConfinedPath` cannot be constructed without passing through the full pipeline.

## Pipeline Stages

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
│ 2. percent_decode()             │  Single-pass decode, reject malformed/NUL/invalid UTF-8
│    path/decode.rs               │
└─────────────────┬───────────────┘
                  │
                  ▼
┌─────────────────────────────────┐
│ 3. normalize_path()             │  Collapse `//`, `./`, `../`, trailing slashes
│    path/components.rs           │
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
│ 5. validate_components()        │  Per-component checks:
│    path/components.rs           │    - Reject `.` and `..`
│                                  │    - Reject NUL bytes
│                                  │    - Reject backslash (if policy requires)
│                                  │    - Reject dotfiles (if policy requires)
└─────────────────┬───────────────┘
                  │
                  ▼
┌─────────────────────────────────┐
│ 6. platform_checks()            │  Windows: reserved names, ADS, drive prefixes
│    path/platform.rs             │  (skipped on non-Windows)
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
| `rejected.rs` | `path/rejected.rs` | `PathRejection` enum (16 variants) |
| `policy.rs` | `path/policy.rs` | `PathPolicy`, `DotfilePolicy` (path-level) |
| `platform.rs` | `path/platform.rs` | Windows-specific checks |

## `ConfinedPath`

The output of the pipeline. An opaque, validated type:

```rust
pub struct ConfinedPath {
    decoded_path: String,      // percent-decoded, normalized
    components: Vec<String>,   // non-empty path segments
}
```

Methods:
- `decoded_path()` — The full decoded path string
- `components()` — Iterator over path segments

## Rejection Types (`PathRejection`)

16 variants covering every possible rejection reason:

| Variant | Stage | Meaning |
|---------|-------|---------|
| `Empty` | parse | Empty request target |
| `TooLong` | parse | Target exceeds maximum length |
| `UnsupportedUriForm` | parse | Not origin-form (absolute or authority form) |
| `MalformedPercentEncoding` | decode | Invalid `%XX` sequence |
| `InvalidUtf8` | decode | Decoded bytes are not valid UTF-8 |
| `NulByte` | decode | Decoded path contains NUL |
| `AbsolutePath` | components | Path starts with `/` (after normalization) |
| `ParentComponent` | components | `..` component found |
| `CurrentComponent` | components | `.` component found |
| `SeparatorAmbiguity` | components | Backslash found (if policy requires) |
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

Windows-only (compiled but only effective on Windows targets):

- **Drive prefixes** — Rejects `C:\`, `\\server\share`, etc.
- **Reserved names** — Rejects `CON`, `NUL`, `PRN`, `AUX`, `COM1`–`COM9`, `LPT1`–`LPT9`
- **Alternate data streams** — Rejects `file:stream` syntax

## Security Properties

1. **No bypass** — A `ConfinedPath` can only be produced by the pipeline. There is no `unsafe` way to construct one.
2. **Deterministic** — Same input always produces the same output (after normalization).
3. **No filesystem access** — Path confinement is pure string manipulation. No `stat()`, no `open()`.
4. **Policy-aware** — Validation is parameterized by `PathPolicy`, but safe defaults deny everything.

## See Also

- [filesystem-confinement.md](filesystem-confinement.md) — What happens after path validation
- [policy-system.md](policy-system.md) — Policy types and enforcement
- [primitives-api.md](primitives-api.md) — Public API for path validation

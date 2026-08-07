# Policy System — Deep Dive

eggserve uses a layered policy system to control what can be served. Policies are checked at multiple stages: path validation, filesystem resolution, and response construction.

## Policy Types

### `StaticPolicy` (`policy.rs`)

The top-level composite policy. Aggregates all sub-policies.

```rust
pub struct StaticPolicy {
    pub directory_listing: DirectoryListingPolicy,
    pub symlinks: SymlinkPolicy,
    pub dotfiles: DotfilePolicy,
}
```

`StaticPolicy::safe_default()` returns the most restrictive configuration:
- `DirectoryListingPolicy::Disabled`
- `SymlinkPolicy::Denied`
- `DotfilePolicy::Denied`

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

| Type | Location | Controls | When Checked |
|------|----------|----------|--------------|
| `path::DotfilePolicy` | `path/policy.rs` | Whether dotfile paths are *accepted* during parsing | Path validation stage |
| `policy::DotfilePolicy` | `policy.rs` | Whether dotfiles are *served* in responses | Response stage |

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

## See Also

- [path-confinement.md](path-confinement.md) — Path-level policy enforcement
- [filesystem-confinement.md](filesystem-confinement.md) — Symlink policy in filesystem traversal
- [eggserve-core.md](eggserve-core.md) — Policy module location

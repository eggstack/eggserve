//! Security policy types for filesystem access control.
//!
//! All policy types default to the most restrictive setting. Callers must
//! explicitly opt in to less restrictive behaviors.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum PolicyMode {
    Strict,
    Compat,
}

/// Controls whether directory listings are generated for directory requests
/// that lack an `index.html`.
///
/// Default: `Disabled`. Directories without an index file return 403.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DirectoryListingPolicy {
    #[default]
    Disabled,
    Enabled,
}

/// Controls whether symbolic links are followed during path resolution.
///
/// Default: `Denied`. Symlinks are refused at the filesystem layer using
/// descriptor-relative traversal (`openat` with `O_NOFOLLOW` on Unix).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SymlinkPolicy {
    #[default]
    Denied,
    Follow,
}

/// Controls whether dotfiles (paths containing a component starting with `.`)
/// are served.
///
/// Default: `Denied`. Dotfiles return 403.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DotfilePolicy {
    #[default]
    Denied,
    Serve,
}

/// Controls which filesystem-derived validators are emitted on static responses.
///
/// Both fields default to `true` (emit). The minimal-fingerprint profile
/// suppresses both to avoid disclosing host/content timestamp characteristics.
///
/// When `Last-Modified` is retained, the final runtime boundary enforces the
/// RFC rule that it must not be later than `Date` (a future mtime drops
/// `Last-Modified` rather than emitting an inconsistent pair).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StaticMetadataPolicy {
    /// Emit metadata-derived `ETag` (weak, size + mtime). Default: `true`.
    pub emit_etag: bool,
    /// Emit `Last-Modified` from filesystem mtime. Default: `true`.
    pub emit_last_modified: bool,
}

impl StaticMetadataPolicy {
    /// Default policy: emit both validators (preserves current behavior).
    pub fn standard() -> Self {
        Self {
            emit_etag: true,
            emit_last_modified: true,
        }
    }

    /// Minimal-fingerprint policy: suppress both validators.
    ///
    /// Suppression is preferable to content hashing: hashing adds unbounded
    /// startup/read cost for theoretical resistance, while suppression trades
    /// weaker conditional caching for a smaller fingerprint surface.
    pub fn minimal_fingerprint() -> Self {
        Self {
            emit_etag: false,
            emit_last_modified: false,
        }
    }
}

/// Controls the representation of runtime-generated client errors.
///
/// `Minimal` (default) emits fixed status-specific plain-text bodies with a
/// fixed `Content-Type` and no version/path/exception detail. `Empty` emits
/// no body bytes (`Content-Length: 0`) for runtime-generated errors.
///
/// Service-level custom 4xx/5xx returned via `Ok(Response)` are application
/// content and are never rewritten by this policy; only errors constructed
/// by the EggServe runtime itself are affected. `HEAD` suppression remains
/// correct under both variants.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ErrorRepresentationPolicy {
    /// Fixed generic plain-text bodies (current behavior). Default.
    #[default]
    Minimal,
    /// Deliberately empty bodies for runtime-generated errors.
    Empty,
}

/// Composite security policy for static file serving.
///
/// Combines directory listing, symlink, and dotfile policies into a single
/// configuration. [`StaticPolicy::safe_default()`] denies all optional
/// behaviors; callers must explicitly opt in.
///
/// # Examples
///
/// ```
/// use eggserve_core::policy::{StaticPolicy, DirectoryListingPolicy};
///
/// let mut policy = StaticPolicy::safe_default();
/// policy.directory_listing = DirectoryListingPolicy::Enabled;
/// ```
#[derive(Debug, Clone)]
#[must_use]
pub struct StaticPolicy {
    pub directory_listing: DirectoryListingPolicy,
    pub symlinks: SymlinkPolicy,
    pub dotfiles: DotfilePolicy,
    /// Static validator emission policy. Default: emit both `ETag` and
    /// `Last-Modified`.
    pub static_metadata: StaticMetadataPolicy,
}

impl Default for StaticPolicy {
    fn default() -> Self {
        Self::safe_default()
    }
}

impl StaticPolicy {
    pub fn safe_default() -> Self {
        Self {
            directory_listing: DirectoryListingPolicy::Disabled,
            symlinks: SymlinkPolicy::Denied,
            dotfiles: DotfilePolicy::Denied,
            static_metadata: StaticMetadataPolicy::standard(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_default_disables_directory_listing() {
        let policy = StaticPolicy::safe_default();
        assert_eq!(policy.directory_listing, DirectoryListingPolicy::Disabled);
    }

    #[test]
    fn safe_default_denies_symlinks() {
        let policy = StaticPolicy::safe_default();
        assert_eq!(policy.symlinks, SymlinkPolicy::Denied);
    }

    #[test]
    fn safe_default_denies_dotfiles() {
        let policy = StaticPolicy::safe_default();
        assert_eq!(policy.dotfiles, DotfilePolicy::Denied);
    }
}

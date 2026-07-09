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

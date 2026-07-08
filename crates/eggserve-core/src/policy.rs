#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyMode {
    Strict,
    Compat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectoryListingPolicy {
    Disabled,
    Enabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymlinkPolicy {
    Denied,
    Follow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DotfilePolicy {
    Denied,
    Serve,
}

#[derive(Debug, Clone)]
pub struct StaticPolicy {
    pub directory_listing: DirectoryListingPolicy,
    pub symlinks: SymlinkPolicy,
    pub dotfiles: DotfilePolicy,
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

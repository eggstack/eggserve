//! Security policy types controlling what requests and paths are allowed.

/// Operating mode that relaxes security constraints for compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyMode {
    /// Default mode: deny symlinks, dotfiles, directory listings.
    Strict,
    /// Compatibility mode: relax some defaults for `http.server` parity.
    Compat,
}

/// Security policy applied to incoming requests.
#[derive(Debug, Clone)]
pub struct Policy {
    /// The current policy mode.
    pub mode: PolicyMode,
}

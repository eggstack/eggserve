use super::rejected::PathRejection;

/// Controls whether dotfile paths are accepted during [`ConfinedPath`](super::ConfinedPath) parsing.
///
/// Default: `Denied`. Paths containing a component starting with `.` are
/// rejected before filesystem resolution.
///
/// This is distinct from [`crate::policy::DotfilePolicy`], which controls
/// whether dotfiles are served in the final response. Both must allow dotfiles
/// for them to be served.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DotfilePolicy {
    #[default]
    Denied,
    Allow,
}

/// Configuration for path validation during [`ConfinedPath`](super::ConfinedPath) parsing.
///
/// Default: dotfiles denied, backslash rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathPolicy {
    pub dotfiles: DotfilePolicy,
    pub reject_backslash: bool,
}

impl Default for PathPolicy {
    fn default() -> Self {
        Self {
            dotfiles: DotfilePolicy::Denied,
            reject_backslash: true,
        }
    }
}

impl PathPolicy {
    #[allow(dead_code)]
    pub fn check_dotfile(&self, component: &str) -> Result<(), PathRejection> {
        if self.dotfiles == DotfilePolicy::Denied && component.starts_with('.') {
            return Err(PathRejection::DotfileDenied);
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn check_backslash(&self, component: &str) -> Result<(), PathRejection> {
        if self.reject_backslash && component.contains('\\') {
            return Err(PathRejection::SeparatorAmbiguity);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_denies_dotfiles() {
        let policy = PathPolicy::default();
        assert_eq!(policy.dotfiles, DotfilePolicy::Denied);
    }

    #[test]
    fn default_policy_rejects_backslash() {
        let policy = PathPolicy::default();
        assert!(policy.reject_backslash);
    }

    #[test]
    fn check_dotfile_denied() {
        let policy = PathPolicy::default();
        assert_eq!(
            policy.check_dotfile(".env").unwrap_err(),
            PathRejection::DotfileDenied
        );
    }

    #[test]
    fn check_dotfile_allowed() {
        let policy = PathPolicy {
            dotfiles: DotfilePolicy::Allow,
            ..PathPolicy::default()
        };
        assert!(policy.check_dotfile(".env").is_ok());
    }

    #[test]
    fn check_backslash_rejected() {
        let policy = PathPolicy::default();
        assert_eq!(
            policy.check_backslash("foo\\bar").unwrap_err(),
            PathRejection::SeparatorAmbiguity
        );
    }

    #[test]
    fn check_backslash_allowed() {
        let policy = PathPolicy {
            reject_backslash: false,
            ..PathPolicy::default()
        };
        assert!(policy.check_backslash("foo\\bar").is_ok());
    }

    #[test]
    fn dotfile_policy_deny_rejects_hidden_component() {
        let policy = PathPolicy::default();
        assert_eq!(
            policy.check_dotfile(".hidden").unwrap_err(),
            PathRejection::DotfileDenied
        );
    }

    #[test]
    fn dotfile_policy_allow_permits_hidden_component() {
        let policy = PathPolicy {
            dotfiles: DotfilePolicy::Allow,
            ..PathPolicy::default()
        };
        assert!(policy.check_dotfile(".hidden").is_ok());
    }

    #[test]
    fn dotfile_policy_deny_rejects_nested_dotfile() {
        let policy = PathPolicy::default();
        assert_eq!(
            policy.check_dotfile(".env").unwrap_err(),
            PathRejection::DotfileDenied
        );
        assert_eq!(
            policy.check_dotfile(".gitconfig").unwrap_err(),
            PathRejection::DotfileDenied
        );
    }

    #[test]
    fn dotfile_policy_allow_permits_all_dotfiles() {
        let policy = PathPolicy {
            dotfiles: DotfilePolicy::Allow,
            ..PathPolicy::default()
        };
        assert!(policy.check_dotfile(".env").is_ok());
        assert!(policy.check_dotfile(".hidden").is_ok());
        assert!(policy.check_dotfile(".gitconfig").is_ok());
    }

    #[test]
    fn backslash_rejected_by_default() {
        let policy = PathPolicy::default();
        assert_eq!(
            policy.check_backslash("foo\\bar").unwrap_err(),
            PathRejection::SeparatorAmbiguity
        );
        assert_eq!(
            policy.check_backslash("\\").unwrap_err(),
            PathRejection::SeparatorAmbiguity
        );
        assert_eq!(
            policy.check_backslash("a\\b\\c").unwrap_err(),
            PathRejection::SeparatorAmbiguity
        );
    }

    #[test]
    fn backslash_allowed_when_policy_permits() {
        let policy = PathPolicy {
            reject_backslash: false,
            ..PathPolicy::default()
        };
        assert!(policy.check_backslash("foo\\bar").is_ok());
        assert!(policy.check_backslash("\\").is_ok());
        assert!(policy.check_backslash("a\\b\\c").is_ok());
    }

    #[test]
    fn check_dotfile_no_dot_prefix_allowed() {
        let policy = PathPolicy::default();
        assert!(policy.check_dotfile("file.txt").is_ok());
        assert!(policy.check_dotfile("normaldir").is_ok());
    }
}

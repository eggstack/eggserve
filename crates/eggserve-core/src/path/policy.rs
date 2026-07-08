use super::rejected::PathRejection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DotfilePolicy {
    Denied,
    Allow,
}

#[derive(Debug, Clone)]
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
}

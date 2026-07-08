use super::decode;
use super::platform;
use super::policy::PathPolicy;
use super::rejected::PathRejection;

pub fn validate_components(
    components: &[String],
    policy: &PathPolicy,
) -> Result<(), PathRejection> {
    for component in components {
        if component.contains('\0') {
            return Err(PathRejection::NulByte);
        }

        if component.contains('/') {
            return Err(PathRejection::SeparatorAmbiguity);
        }

        if component == "." {
            return Err(PathRejection::CurrentComponent);
        }

        if component == ".." {
            return Err(PathRejection::ParentComponent);
        }

        if policy.reject_backslash && component.contains('\\') {
            return Err(PathRejection::SeparatorAmbiguity);
        }

        if policy.dotfiles == super::policy::DotfilePolicy::Denied && component.starts_with('.') {
            return Err(PathRejection::DotfileDenied);
        }

        if let Ok(decoded) = decode::percent_decode(component) {
            if decoded == "." {
                return Err(PathRejection::CurrentComponent);
            }
            if decoded == ".." {
                return Err(PathRejection::ParentComponent);
            }
        }

        platform::check_component(component)?;
    }

    Ok(())
}

pub fn split_components(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

pub fn normalize_path(path: &str) -> String {
    if path == "/" {
        return String::new();
    }

    let stripped = path.trim_start_matches('/');

    let mut normalized = String::with_capacity(stripped.len());
    let mut prev_was_slash = false;

    for c in stripped.chars() {
        if c == '/' {
            if !prev_was_slash {
                normalized.push('/');
            }
            prev_was_slash = true;
        } else {
            normalized.push(c);
            prev_was_slash = false;
        }
    }

    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_simple() {
        assert_eq!(
            split_components("/foo/bar"),
            vec!["foo".to_string(), "bar".to_string()]
        );
    }

    #[test]
    fn split_empty() {
        assert!(split_components("/").is_empty());
    }

    #[test]
    fn split_multiple_slashes() {
        assert_eq!(
            split_components("/foo//bar///baz"),
            vec!["foo".to_string(), "bar".to_string(), "baz".to_string()]
        );
    }

    #[test]
    fn normalize_consecutive() {
        assert_eq!(normalize_path("/foo//bar"), "foo/bar");
        assert_eq!(normalize_path("/foo///bar"), "foo/bar");
        assert_eq!(normalize_path("//foo"), "foo");
        assert_eq!(normalize_path("/"), "");
        assert_eq!(normalize_path("/foo"), "foo");
    }

    #[test]
    fn reject_empty_component() {
        let comps = vec!["foo".to_string(), "".to_string(), "bar".to_string()];
        let policy = PathPolicy::default();
        assert!(validate_components(&comps, &policy).is_ok());
    }

    #[test]
    fn reject_dot() {
        let comps = vec!["foo".to_string(), ".".to_string(), "bar".to_string()];
        let policy = PathPolicy::default();
        assert_eq!(
            validate_components(&comps, &policy).unwrap_err(),
            PathRejection::CurrentComponent
        );
    }

    #[test]
    fn reject_dotdot() {
        let comps = vec!["foo".to_string(), "..".to_string(), "bar".to_string()];
        let policy = PathPolicy::default();
        assert_eq!(
            validate_components(&comps, &policy).unwrap_err(),
            PathRejection::ParentComponent
        );
    }

    #[test]
    fn reject_nul() {
        let comps = vec!["foo\0bar".to_string()];
        let policy = PathPolicy::default();
        assert_eq!(
            validate_components(&comps, &policy).unwrap_err(),
            PathRejection::NulByte
        );
    }

    #[test]
    fn reject_slash_in_component() {
        let comps = vec!["foo/bar".to_string()];
        let policy = PathPolicy::default();
        assert_eq!(
            validate_components(&comps, &policy).unwrap_err(),
            PathRejection::SeparatorAmbiguity
        );
    }

    #[test]
    fn reject_backslash() {
        let comps = vec!["foo\\bar".to_string()];
        let policy = PathPolicy::default();
        assert_eq!(
            validate_components(&comps, &policy).unwrap_err(),
            PathRejection::SeparatorAmbiguity
        );
    }

    #[test]
    fn allow_backslash_when_policy_permits() {
        let comps = vec!["foo\\bar".to_string()];
        let policy = PathPolicy {
            reject_backslash: false,
            ..PathPolicy::default()
        };
        assert!(validate_components(&comps, &policy).is_ok());
    }

    #[test]
    fn reject_dotfile() {
        let comps = vec![".env".to_string()];
        let policy = PathPolicy::default();
        assert_eq!(
            validate_components(&comps, &policy).unwrap_err(),
            PathRejection::DotfileDenied
        );
    }

    #[test]
    fn allow_dotfile_when_policy_permits() {
        let comps = vec![".env".to_string()];
        let policy = PathPolicy {
            dotfiles: crate::path::DotfilePolicy::Allow,
            ..PathPolicy::default()
        };
        assert!(validate_components(&comps, &policy).is_ok());
    }

    #[test]
    fn reject_windows_drive_in_component() {
        let comps = vec!["C:".to_string()];
        let policy = PathPolicy::default();
        assert_eq!(
            validate_components(&comps, &policy).unwrap_err(),
            PathRejection::WindowsPrefixDenied
        );
    }

    #[test]
    fn reject_windows_reserved_in_component() {
        let comps = vec!["CON".to_string()];
        let policy = PathPolicy::default();
        assert_eq!(
            validate_components(&comps, &policy).unwrap_err(),
            PathRejection::WindowsReservedNameDenied
        );
    }

    #[test]
    fn reject_windows_ads_in_component() {
        let comps = vec!["file.txt:stream".to_string()];
        let policy = PathPolicy::default();
        assert_eq!(
            validate_components(&comps, &policy).unwrap_err(),
            PathRejection::WindowsAlternateStreamDenied
        );
    }

    #[test]
    fn ok_components() {
        let comps = vec!["foo".to_string(), "bar.txt".to_string(), "a1".to_string()];
        let policy = PathPolicy::default();
        assert!(validate_components(&comps, &policy).is_ok());
    }

    #[test]
    fn reject_double_encoded_dotdot() {
        let comps = vec!["%2e%2e".to_string()];
        let policy = PathPolicy::default();
        assert_eq!(
            validate_components(&comps, &policy).unwrap_err(),
            PathRejection::ParentComponent
        );
    }

    #[test]
    fn reject_double_encoded_dot() {
        let comps = vec!["%2e".to_string()];
        let policy = PathPolicy::default();
        assert_eq!(
            validate_components(&comps, &policy).unwrap_err(),
            PathRejection::CurrentComponent
        );
    }

    #[test]
    fn reject_double_encoded_uppercase() {
        let comps = vec!["%2E%2E".to_string()];
        let policy = PathPolicy::default();
        assert_eq!(
            validate_components(&comps, &policy).unwrap_err(),
            PathRejection::ParentComponent
        );
    }
}

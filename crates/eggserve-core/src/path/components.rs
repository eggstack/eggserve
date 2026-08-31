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

        if component.chars().any(|c| c.is_ascii_control()) {
            return Err(PathRejection::ControlCharacter);
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

        // The path was already percent-decoded once by `ConfinedPath::parse`.
        // A second decode is still rejected when it would create a dot
        // segment: this prevents a double-encoded traversal component from
        // being interpreted by a later path consumer. Skip the allocation
        // entirely for the common case of a component with no `%`.
        if component.contains('%') {
            if let Ok(decoded) = decode::percent_decode(component) {
                if decoded == "." {
                    return Err(PathRejection::CurrentComponent);
                }
                if decoded == ".." {
                    return Err(PathRejection::ParentComponent);
                }
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

/// Collapse duplicate slashes and strip leading slashes.
///
/// # Safety invariant (call-order)
///
/// This function deliberately does NOT resolve `.` or `..` segments: dot
/// segments survive normalization and are only rejected by
/// [`validate_components`]. The two must always be used together, in this
/// order (`normalize_path` → `split_components` → `validate_components`),
/// as [`super::ConfinedPath::parse`] does. A caller that resolves or
/// consumes components without running `validate_components` afterwards
/// has no confinement guarantee: `..`, dotfiles, NUL, and platform
/// hazards would pass through unvalidated.
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
    fn reject_control_character() {
        let components = vec!["foo\x1fbar".to_string()];
        assert_eq!(
            validate_components(&components, &PathPolicy::default()).unwrap_err(),
            PathRejection::ControlCharacter
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

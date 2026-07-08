pub mod components;
pub mod decode;
pub mod platform;
pub mod policy;
pub mod rejected;
pub mod request_target;

pub use policy::{DotfilePolicy, PathPolicy};
pub use rejected::PathRejection;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfinedPath {
    decoded: String,
    components: Vec<String>,
}

impl ConfinedPath {
    pub fn parse(raw: &str, policy: &PathPolicy) -> Result<Self, PathRejection> {
        let path = request_target::parse_origin_form(raw)?;

        let decoded = decode::percent_decode(path)?;

        let normalized = components::normalize_path(&decoded);

        let parts = components::split_components(&normalized);

        components::validate_components(&parts, policy)?;

        Ok(Self {
            decoded,
            components: parts,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.decoded
    }

    pub fn components(&self) -> &[String] {
        &self.components
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_policy() -> PathPolicy {
        PathPolicy::default()
    }

    #[test]
    fn simple_path() {
        let p = ConfinedPath::parse("/foo/bar", &default_policy()).unwrap();
        assert_eq!(p.as_str(), "/foo/bar");
        assert_eq!(p.components(), &["foo", "bar"]);
    }

    #[test]
    fn root_path() {
        let p = ConfinedPath::parse("/", &default_policy()).unwrap();
        assert_eq!(p.as_str(), "/");
        assert_eq!(p.components().len(), 0);
    }

    #[test]
    fn path_with_query_stripped() {
        let p = ConfinedPath::parse("/foo?bar=baz", &default_policy()).unwrap();
        assert_eq!(p.as_str(), "/foo");
        assert_eq!(p.components(), &["foo"]);
    }

    #[test]
    fn reject_empty() {
        assert_eq!(
            ConfinedPath::parse("", &default_policy()).unwrap_err(),
            PathRejection::Empty
        );
    }

    #[test]
    fn reject_absolute_form() {
        assert_eq!(
            ConfinedPath::parse("http://example.com/path", &default_policy()).unwrap_err(),
            PathRejection::UnsupportedUriForm
        );
    }

    #[test]
    fn reject_asterisk_form() {
        assert_eq!(
            ConfinedPath::parse("*", &default_policy()).unwrap_err(),
            PathRejection::UnsupportedUriForm
        );
    }

    #[test]
    fn reject_authority_form() {
        assert_eq!(
            ConfinedPath::parse("example.com:443", &default_policy()).unwrap_err(),
            PathRejection::UnsupportedUriForm
        );
    }

    #[test]
    fn normalize_consecutive_slashes() {
        let p = ConfinedPath::parse("/foo//bar", &default_policy()).unwrap();
        assert_eq!(p.components(), &["foo", "bar"]);
    }

    #[test]
    fn reject_dot_component() {
        assert_eq!(
            ConfinedPath::parse("/foo/./bar", &default_policy()).unwrap_err(),
            PathRejection::CurrentComponent
        );
    }

    #[test]
    fn reject_dotdot_component() {
        assert_eq!(
            ConfinedPath::parse("/../etc/passwd", &default_policy()).unwrap_err(),
            PathRejection::ParentComponent
        );
    }

    #[test]
    fn reject_percent_encoded_dotdot() {
        assert_eq!(
            ConfinedPath::parse("/%2e%2e/etc/passwd", &default_policy()).unwrap_err(),
            PathRejection::ParentComponent
        );
    }

    #[test]
    fn reject_uppercase_percent_encoded_dotdot() {
        assert_eq!(
            ConfinedPath::parse("/%2E%2E/etc/passwd", &default_policy()).unwrap_err(),
            PathRejection::ParentComponent
        );
    }

    #[test]
    fn reject_double_encoded_dotdot() {
        assert_eq!(
            ConfinedPath::parse("/%252e%252e/etc/passwd", &default_policy()).unwrap_err(),
            PathRejection::ParentComponent
        );
    }

    #[test]
    fn reject_dotdot_in_path() {
        assert_eq!(
            ConfinedPath::parse("/foo/../../bar", &default_policy()).unwrap_err(),
            PathRejection::ParentComponent
        );
    }

    #[test]
    fn reject_percent_encoded_dotdot_in_path() {
        assert_eq!(
            ConfinedPath::parse("/foo/%2e%2e/bar", &default_policy()).unwrap_err(),
            PathRejection::ParentComponent
        );
    }

    #[test]
    fn reject_backslash() {
        assert_eq!(
            ConfinedPath::parse("/foo\\bar", &default_policy()).unwrap_err(),
            PathRejection::SeparatorAmbiguity
        );
    }

    #[test]
    fn reject_percent_encoded_backslash() {
        assert_eq!(
            ConfinedPath::parse("/%5cetc%5cpasswd", &default_policy()).unwrap_err(),
            PathRejection::SeparatorAmbiguity
        );
    }

    #[test]
    fn reject_windows_drive_prefix() {
        assert_eq!(
            ConfinedPath::parse("/C:/Windows/System32", &default_policy()).unwrap_err(),
            PathRejection::WindowsPrefixDenied
        );
    }

    #[test]
    fn reject_percent_encoded_windows_drive() {
        assert_eq!(
            ConfinedPath::parse("/c%3a/Windows/System32", &default_policy()).unwrap_err(),
            PathRejection::WindowsPrefixDenied
        );
    }

    #[test]
    fn reject_dotfile() {
        assert_eq!(
            ConfinedPath::parse("/.env", &default_policy()).unwrap_err(),
            PathRejection::DotfileDenied
        );
    }

    #[test]
    fn reject_dotfile_git_config() {
        assert_eq!(
            ConfinedPath::parse("/.git/config", &default_policy()).unwrap_err(),
            PathRejection::DotfileDenied
        );
    }

    #[test]
    fn reject_dotfile_in_subdir() {
        assert_eq!(
            ConfinedPath::parse("/foo/.secret", &default_policy()).unwrap_err(),
            PathRejection::DotfileDenied
        );
    }

    #[test]
    fn reject_windows_reserved_con() {
        assert_eq!(
            ConfinedPath::parse("/CON", &default_policy()).unwrap_err(),
            PathRejection::WindowsReservedNameDenied
        );
    }

    #[test]
    fn reject_windows_reserved_aux() {
        assert_eq!(
            ConfinedPath::parse("/AUX.txt", &default_policy()).unwrap_err(),
            PathRejection::WindowsReservedNameDenied
        );
    }

    #[test]
    fn reject_windows_reserved_com1() {
        assert_eq!(
            ConfinedPath::parse("/COM1", &default_policy()).unwrap_err(),
            PathRejection::WindowsReservedNameDenied
        );
    }

    #[test]
    fn reject_windows_ads() {
        assert_eq!(
            ConfinedPath::parse("/file.txt:stream", &default_policy()).unwrap_err(),
            PathRejection::WindowsAlternateStreamDenied
        );
    }

    #[test]
    fn reject_nul() {
        assert_eq!(
            ConfinedPath::parse("/%00", &default_policy()).unwrap_err(),
            PathRejection::NulByte
        );
    }

    #[test]
    fn reject_malformed_percent() {
        assert_eq!(
            ConfinedPath::parse("/%ZZ", &default_policy()).unwrap_err(),
            PathRejection::MalformedPercentEncoding
        );
    }

    #[test]
    fn allow_dotfile_when_policy_permits() {
        let policy = PathPolicy {
            dotfiles: DotfilePolicy::Allow,
            ..PathPolicy::default()
        };
        let p = ConfinedPath::parse("/.env", &policy).unwrap();
        assert_eq!(p.as_str(), "/.env");
    }

    #[test]
    fn allow_backslash_when_policy_permits() {
        let policy = PathPolicy {
            reject_backslash: false,
            ..PathPolicy::default()
        };
        let p = ConfinedPath::parse("/foo\\bar", &policy).unwrap();
        assert_eq!(p.as_str(), "/foo\\bar");
    }

    #[test]
    fn reject_double_slash_root() {
        let p = ConfinedPath::parse("//", &default_policy()).unwrap();
        assert_eq!(p.components().len(), 0);
    }

    #[test]
    fn reject_triple_slash() {
        let p = ConfinedPath::parse("///", &default_policy()).unwrap();
        assert_eq!(p.components().len(), 0);
    }
}

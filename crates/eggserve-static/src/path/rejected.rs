use std::fmt;

/// The reason a request target was refused before filesystem resolution.
///
/// Every variant names the specific rule the path layer enforces, so a caller
/// can distinguish a policy denial from a malformed target without parsing
/// the `Display` text. The path layer never resolves a rejected target: the
/// rejection is the answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PathRejection {
    /// The target carried no path at all.
    Empty,
    /// The target exceeded the parse-level length ceiling (8192 bytes).
    #[allow(dead_code)]
    TooLong,
    /// The target was not in the origin form the path layer accepts (for
    /// example, it contained whitespace or was not classified as a path).
    UnsupportedUriForm,
    /// A percent escape was truncated or carried a non-hexadecimal digit.
    MalformedPercentEncoding,
    /// The percent-decoded octets did not form valid UTF-8.
    InvalidUtf8,
    /// The target contained a NUL byte, literal or percent-encoded.
    NulByte,
    /// The target contained an ASCII control character.
    ControlCharacter,
    /// The path was absolute rather than root-relative.
    #[allow(dead_code)]
    AbsolutePath,
    /// A component was `..`.
    ParentComponent,
    /// A component was `.`.
    CurrentComponent,
    /// A component carried a separator (`/` or `\`), including a
    /// percent-encoded separator that would otherwise alias a distinct
    /// target.
    SeparatorAmbiguity,
    /// A component began with `.` and the dotfile policy denies dotfiles.
    DotfileDenied,
    /// A component carried a Windows drive prefix (for example `C:`).
    WindowsPrefixDenied,
    /// A component named a Windows reserved device (`CON`, `NUL`, `COM1`,
    /// …) or ended in a dot or space, which Windows trims during name
    /// resolution.
    WindowsReservedNameDenied,
    /// A component carried NTFS alternate data stream syntax (a `:`).
    WindowsAlternateStreamDenied,
    /// The resolved path was a symlink and the policy denies symlinks.
    SymlinkDenied,
    /// The canonical path of the resolved target escaped the served root.
    RootEscapeDenied,
}

impl fmt::Display for PathRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathRejection::Empty => write!(f, "empty path"),
            PathRejection::TooLong => write!(f, "path too long"),
            PathRejection::UnsupportedUriForm => write!(f, "unsupported URI form"),
            PathRejection::MalformedPercentEncoding => {
                write!(f, "malformed percent encoding")
            }
            PathRejection::InvalidUtf8 => write!(f, "invalid UTF-8"),
            PathRejection::NulByte => write!(f, "NUL byte in path"),
            PathRejection::ControlCharacter => write!(f, "control character in path"),
            PathRejection::AbsolutePath => write!(f, "absolute path"),
            PathRejection::ParentComponent => write!(f, "parent component (..)"),
            PathRejection::CurrentComponent => write!(f, "current component (.)"),
            PathRejection::SeparatorAmbiguity => write!(f, "separator ambiguity"),
            PathRejection::DotfileDenied => write!(f, "dotfile denied"),
            PathRejection::WindowsPrefixDenied => write!(f, "Windows prefix denied"),
            PathRejection::WindowsReservedNameDenied => {
                write!(f, "Windows reserved name denied")
            }
            PathRejection::WindowsAlternateStreamDenied => {
                write!(f, "Windows alternate stream denied")
            }
            PathRejection::SymlinkDenied => write!(f, "symlink denied by policy"),
            PathRejection::RootEscapeDenied => write!(f, "canonical path escapes root"),
        }
    }
}

impl std::error::Error for PathRejection {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_all_variants() {
        let cases = &[
            (PathRejection::Empty, "empty path"),
            (PathRejection::TooLong, "path too long"),
            (PathRejection::UnsupportedUriForm, "unsupported URI form"),
            (
                PathRejection::MalformedPercentEncoding,
                "malformed percent encoding",
            ),
            (PathRejection::InvalidUtf8, "invalid UTF-8"),
            (PathRejection::NulByte, "NUL byte in path"),
            (PathRejection::ControlCharacter, "control character in path"),
            (PathRejection::AbsolutePath, "absolute path"),
            (PathRejection::ParentComponent, "parent component (..)"),
            (PathRejection::CurrentComponent, "current component (.)"),
            (PathRejection::SeparatorAmbiguity, "separator ambiguity"),
            (PathRejection::DotfileDenied, "dotfile denied"),
            (PathRejection::WindowsPrefixDenied, "Windows prefix denied"),
            (
                PathRejection::WindowsReservedNameDenied,
                "Windows reserved name denied",
            ),
            (
                PathRejection::WindowsAlternateStreamDenied,
                "Windows alternate stream denied",
            ),
            (PathRejection::SymlinkDenied, "symlink denied by policy"),
            (
                PathRejection::RootEscapeDenied,
                "canonical path escapes root",
            ),
        ];
        for (rejection, expected) in cases {
            assert_eq!(rejection.to_string(), *expected);
        }
    }

    #[test]
    fn is_error() {
        let err: &dyn std::error::Error = &PathRejection::Empty;
        assert_eq!(err.to_string(), "empty path");
    }
}

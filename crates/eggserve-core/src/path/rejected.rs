use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PathRejection {
    Empty,
    TooLong,
    UnsupportedUriForm,
    MalformedPercentEncoding,
    InvalidUtf8,
    NulByte,
    AbsolutePath,
    ParentComponent,
    CurrentComponent,
    SeparatorAmbiguity,
    DotfileDenied,
    WindowsPrefixDenied,
    WindowsReservedNameDenied,
    WindowsAlternateStreamDenied,
    SymlinkDenied,
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

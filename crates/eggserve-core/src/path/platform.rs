use super::rejected::PathRejection;

pub fn check_component(component: &str) -> Result<(), PathRejection> {
    if has_windows_drive_prefix(component) {
        return Err(PathRejection::WindowsPrefixDenied);
    }

    if component.contains(':') {
        return Err(PathRejection::WindowsAlternateStreamDenied);
    }

    if is_windows_reserved_name(component) {
        return Err(PathRejection::WindowsReservedNameDenied);
    }

    Ok(())
}

pub fn has_windows_drive_prefix(component: &str) -> bool {
    let bytes = component.as_bytes();
    if bytes.len() < 2 {
        return false;
    }
    bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

pub fn is_windows_reserved_name(component: &str) -> bool {
    let base = component.split('.').next().unwrap_or("");
    let name = strip_trailing_dots(base);
    if name.is_empty() {
        return false;
    }
    matches!(
        name.to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

fn strip_trailing_dots(s: &str) -> &str {
    s.trim_end_matches('.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn reserved_con() {
        assert!(is_windows_reserved_name("CON"));
        assert!(is_windows_reserved_name("con"));
        assert!(is_windows_reserved_name("Con"));
    }

    #[test]
    fn reserved_nul() {
        assert!(is_windows_reserved_name("NUL"));
        assert!(is_windows_reserved_name("nul"));
    }

    #[test]
    fn reserved_com1() {
        assert!(is_windows_reserved_name("COM1"));
        assert!(is_windows_reserved_name("com1"));
    }

    #[test]
    fn reserved_lpt1() {
        assert!(is_windows_reserved_name("LPT1"));
        assert!(is_windows_reserved_name("lpt1"));
    }

    #[test]
    fn not_reserved() {
        assert!(!is_windows_reserved_name("foo"));
        assert!(!is_windows_reserved_name("AUX2"));
        assert!(!is_windows_reserved_name("COM0"));
        assert!(!is_windows_reserved_name("LPT0"));
    }

    #[test]
    fn reserved_with_trailing_dots() {
        assert!(is_windows_reserved_name("CON."));
        assert!(is_windows_reserved_name("NUL..."));
    }

    #[test]
    fn reserved_with_extension() {
        assert!(is_windows_reserved_name("AUX.txt"));
        assert!(is_windows_reserved_name("CON.log"));
        assert!(is_windows_reserved_name("nul.bak"));
    }

    #[test]
    fn drive_prefix() {
        assert!(has_windows_drive_prefix("C:"));
        assert!(has_windows_drive_prefix("c:"));
        assert!(has_windows_drive_prefix("Z:/path"));
    }

    #[test]
    fn not_drive_prefix() {
        assert!(!has_windows_drive_prefix(":"));
        assert!(!has_windows_drive_prefix("/C:"));
        assert!(!has_windows_drive_prefix("CC:"));
        assert!(!has_windows_drive_prefix("1:"));
    }

    #[test]
    fn ads_denied() {
        assert_eq!(
            check_component("file.txt:stream").unwrap_err(),
            PathRejection::WindowsAlternateStreamDenied
        );
    }

    #[test]
    fn drive_denied() {
        assert_eq!(
            check_component("C:").unwrap_err(),
            PathRejection::WindowsPrefixDenied
        );
    }

    #[test]
    fn reserved_denied() {
        assert_eq!(
            check_component("CON").unwrap_err(),
            PathRejection::WindowsReservedNameDenied
        );
    }

    #[test]
    fn reserved_with_ext_denied() {
        assert_eq!(
            check_component("AUX.txt").unwrap_err(),
            PathRejection::WindowsReservedNameDenied
        );
    }

    #[test]
    fn ok_component() {
        assert!(check_component("foo").is_ok());
        assert!(check_component("bar.txt").is_ok());
        assert!(check_component("a1").is_ok());
    }

    proptest::proptest! {
        #[test]
        fn has_windows_drive_prefix_never_panics(s in ".*") {
            let _ = has_windows_drive_prefix(&s);
        }

        #[test]
        fn is_windows_reserved_name_never_panics(s in ".*") {
            let _ = is_windows_reserved_name(&s);
        }

        #[test]
        fn check_component_never_panics(s in ".*") {
            let _ = check_component(&s);
        }

        #[test]
        fn reserved_name_is_always_case_insensitive(s in "[A-Za-z]{1,4}") {
            let upper = s.to_uppercase();
            let lower = s.to_lowercase();
            prop_assert_eq!(is_windows_reserved_name(&upper), is_windows_reserved_name(&lower));
        }

        #[test]
        fn drive_prefix_requires_two_bytes(s in ".*") {
            if has_windows_drive_prefix(&s) {
                prop_assert!(s.len() >= 2);
                prop_assert!(s.as_bytes()[0].is_ascii_alphabetic());
                prop_assert_eq!(s.as_bytes()[1], b':');
            }
        }

        #[test]
        fn check_component_drive_takes_precedence(s in "[A-Za-z]:(.*)") {
            if let Err(e) = check_component(&s) {
                prop_assert_eq!(e, PathRejection::WindowsPrefixDenied);
            }
        }

        #[test]
        fn check_component_no_false_positive(s in "[a-zA-Z0-9_-]+") {
            prop_assert!(check_component(&s).is_ok());
        }
    }
}

use super::rejected::PathRejection;

/// Parse an HTTP origin-form request target, returning the path component.
///
/// The path is split at the first `?` (query delimiter). A `#` character
/// has no special meaning here: well-behaved HTTP clients never send a
/// fragment in the request line (RFC 9110 origin-form has no fragment
/// component), so a literal `#` is treated as part of the path and resolved
/// as an ordinary filename character. This is safe — `#` cannot introduce
/// traversal (it stays within a single path component) — but asymmetric:
/// `/a?b#frag` strips to `/a` (fragment folded into the dropped query)
/// while `/foo#bar` keeps the literal `#` and resolves `/foo#bar`
/// (normally a 404 unless such a file exists).
pub fn parse_origin_form(raw: &str) -> Result<&str, PathRejection> {
    if raw.is_empty() {
        return Err(PathRejection::Empty);
    }

    if raw.as_bytes().contains(&0) {
        return Err(PathRejection::NulByte);
    }

    if raw
        .bytes()
        .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
    {
        return Err(PathRejection::UnsupportedUriForm);
    }

    if !raw.starts_with('/') {
        // Also rejects asterisk-form ("*"): it lacks the leading '/'.
        return Err(PathRejection::UnsupportedUriForm);
    }

    let path_end = raw.find('?').unwrap_or(raw.len());
    let path = &raw[..path_end];

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_path() {
        assert_eq!(parse_origin_form("/").unwrap(), "/");
    }

    #[test]
    fn path_with_query() {
        assert_eq!(parse_origin_form("/foo?bar=baz").unwrap(), "/foo");
    }

    #[test]
    fn path_with_at_sign() {
        assert_eq!(
            parse_origin_form("/file@name.txt").unwrap(),
            "/file@name.txt"
        );
    }

    #[test]
    fn path_without_query() {
        assert_eq!(parse_origin_form("/foo/bar").unwrap(), "/foo/bar");
    }

    #[test]
    fn reject_empty() {
        assert_eq!(parse_origin_form("").unwrap_err(), PathRejection::Empty);
    }

    #[test]
    fn reject_absolute_form() {
        assert_eq!(
            parse_origin_form("http://example.com/path").unwrap_err(),
            PathRejection::UnsupportedUriForm
        );
    }

    #[test]
    fn reject_authority_form() {
        assert_eq!(
            parse_origin_form("example.com:443").unwrap_err(),
            PathRejection::UnsupportedUriForm
        );
    }

    #[test]
    fn reject_asterisk_form() {
        assert_eq!(
            parse_origin_form("*").unwrap_err(),
            PathRejection::UnsupportedUriForm
        );
    }

    #[test]
    fn reject_scheme_without_slashes() {
        assert_eq!(
            parse_origin_form("http:path").unwrap_err(),
            PathRejection::UnsupportedUriForm
        );
    }

    #[test]
    fn path_with_multiple_query_params() {
        assert_eq!(parse_origin_form("/a?b=1&c=2").unwrap(), "/a");
    }

    #[test]
    fn path_with_empty_query() {
        assert_eq!(parse_origin_form("/a?").unwrap(), "/a");
    }

    #[test]
    fn path_with_fragment() {
        assert_eq!(parse_origin_form("/a?b#frag").unwrap(), "/a");
    }

    #[test]
    fn fragment_without_query_is_literal_path() {
        // No `?` means no query split: `#` stays in the path as an
        // ordinary filename character (safe: single component, no
        // traversal; normally resolves to 404).
        assert_eq!(parse_origin_form("/foo#bar").unwrap(), "/foo#bar");
    }

    #[test]
    fn origin_form_allows_colons_in_path() {
        assert_eq!(parse_origin_form("/foo://bar").unwrap(), "/foo://bar");
    }

    #[test]
    fn reject_raw_controls() {
        assert_eq!(
            parse_origin_form("/foo\0bar").unwrap_err(),
            PathRejection::NulByte
        );
        assert_eq!(
            parse_origin_form("/foo\rbar").unwrap_err(),
            PathRejection::UnsupportedUriForm
        );
        assert_eq!(
            parse_origin_form("/foo bar").unwrap_err(),
            PathRejection::UnsupportedUriForm
        );
        assert_eq!(
            parse_origin_form("/foo\x1fbar").unwrap_err(),
            PathRejection::UnsupportedUriForm
        );
    }
}

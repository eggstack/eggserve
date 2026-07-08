use super::rejected::PathRejection;

pub fn parse_origin_form(raw: &str) -> Result<&str, PathRejection> {
    if raw.is_empty() {
        return Err(PathRejection::Empty);
    }

    if !raw.starts_with('/') {
        return Err(PathRejection::UnsupportedUriForm);
    }

    if raw.contains("://") || raw.contains('@') {
        return Err(PathRejection::UnsupportedUriForm);
    }

    if raw == "*" {
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
}

use super::rejected::PathRejection;

pub fn percent_decode(input: &str) -> Result<String, PathRejection> {
    let bytes = input.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return Err(PathRejection::MalformedPercentEncoding);
                }
                let hi = hex_digit(bytes[i + 1]).ok_or(PathRejection::MalformedPercentEncoding)?;
                let lo = hex_digit(bytes[i + 2]).ok_or(PathRejection::MalformedPercentEncoding)?;
                let byte = (hi << 4) | lo;
                if byte == 0 {
                    return Err(PathRejection::NulByte);
                }
                result.push(byte);
                i += 3;
            }
            b => {
                result.push(b);
                i += 1;
            }
        }
    }

    String::from_utf8(result).map_err(|_| PathRejection::InvalidUtf8)
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_path() {
        assert_eq!(percent_decode("/foo/bar").unwrap(), "/foo/bar");
    }

    #[test]
    fn simple_percent() {
        assert_eq!(percent_decode("/foo%20bar").unwrap(), "/foo bar");
    }

    #[test]
    fn uppercase_hex() {
        assert_eq!(percent_decode("/%41").unwrap(), "/A");
    }

    #[test]
    fn lowercase_hex() {
        assert_eq!(percent_decode("/%41%42%43").unwrap(), "/ABC");
    }

    #[test]
    fn reject_double_encode_does_not_double_decode() {
        let result = percent_decode("/%252e%252e/etc/passwd").unwrap();
        assert_eq!(result, "/%2e%2e/etc/passwd");
    }

    #[test]
    fn reject_truncated_at_end() {
        assert_eq!(
            percent_decode("/%2").unwrap_err(),
            PathRejection::MalformedPercentEncoding
        );
    }

    #[test]
    fn reject_truncated_at_second_hex() {
        assert_eq!(
            percent_decode("/%2G").unwrap_err(),
            PathRejection::MalformedPercentEncoding
        );
    }

    #[test]
    fn reject_bad_hex() {
        assert_eq!(
            percent_decode("/%ZZ").unwrap_err(),
            PathRejection::MalformedPercentEncoding
        );
    }

    #[test]
    fn reject_nul() {
        assert_eq!(percent_decode("/%00").unwrap_err(), PathRejection::NulByte);
    }

    #[test]
    fn reject_invalid_utf8() {
        assert_eq!(
            percent_decode("/%c0%af").unwrap_err(),
            PathRejection::InvalidUtf8
        );
    }

    #[test]
    fn path_with_dot() {
        assert_eq!(percent_decode("/.").unwrap(), "/.");
    }

    #[test]
    fn percent_dot_dot() {
        assert_eq!(
            percent_decode("/%2e%2e/etc/passwd").unwrap(),
            "/../etc/passwd"
        );
    }

    #[test]
    fn percent_uppercase_dot_dot() {
        assert_eq!(
            percent_decode("/%2E%2E/etc/passwd").unwrap(),
            "/../etc/passwd"
        );
    }

    #[test]
    fn slash_slash_is_normal() {
        assert_eq!(percent_decode("//server").unwrap(), "//server");
    }
}

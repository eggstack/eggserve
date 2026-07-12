//! Response construction helpers for file streaming and error responses.

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::BodyExt;
use http_body_util::Full;
use http_body_util::StreamBody;
use hyper::body::Frame;
use hyper::{Response, StatusCode};
use std::time::SystemTime;

use crate::primitives::response::HeaderMapPlan;

pub type BoxBodyInner = BoxBody<Bytes, std::io::Error>;

pub fn text_response(status: StatusCode, body: &'static str) -> Response<BoxBodyInner> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .body(full_body(body))
        .unwrap()
}

#[allow(dead_code)]
pub fn empty_response(status: StatusCode) -> Response<BoxBodyInner> {
    Response::builder()
        .status(status)
        .body(full_body(""))
        .unwrap()
}

pub fn method_not_allowed() -> Response<BoxBodyInner> {
    Response::builder()
        .status(StatusCode::METHOD_NOT_ALLOWED)
        .header("allow", "GET, HEAD")
        .body(full_body("405 Method Not Allowed\n"))
        .unwrap()
}

pub fn not_found() -> Response<BoxBodyInner> {
    text_response(StatusCode::NOT_FOUND, "404 Not Found\n")
}

pub fn forbidden() -> Response<BoxBodyInner> {
    text_response(StatusCode::FORBIDDEN, "403 Forbidden\n")
}

pub fn bad_request() -> Response<BoxBodyInner> {
    text_response(StatusCode::BAD_REQUEST, "400 Bad Request\n")
}

pub fn payload_too_large() -> Response<BoxBodyInner> {
    text_response(StatusCode::PAYLOAD_TOO_LARGE, "413 Payload Too Large\n")
}

pub fn internal_error() -> Response<BoxBodyInner> {
    text_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "500 Internal Server Error\n",
    )
}

pub fn service_unavailable() -> Response<BoxBodyInner> {
    text_response(StatusCode::SERVICE_UNAVAILABLE, "503 Service Unavailable\n")
}

pub fn file_response(
    file: tokio::fs::File,
    len: u64,
    mime: &'static str,
    last_modified: Option<SystemTime>,
    etag: Option<String>,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> Response<BoxBodyInner> {
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header("content-length", len.to_string())
        .header("content-type", mime)
        .header("x-content-type-options", "nosniff")
        .header("accept-ranges", "bytes");

    if let Some(mtime) = last_modified {
        if let Ok(secs) = mtime.duration_since(SystemTime::UNIX_EPOCH) {
            let formatted = httpdate::fmt_http_date(SystemTime::UNIX_EPOCH + secs);
            builder = builder.header("last-modified", formatted);
        }
    }

    if let Some(tag) = etag {
        builder = builder.header("etag", tag);
    }

    let stream = futures_util::stream::unfold((file, permit), |(mut file, permit)| async move {
        let mut buf = vec![0u8; 8192];
        match tokio::io::AsyncReadExt::read(&mut file, &mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((
                    Ok::<_, std::io::Error>(Frame::data(Bytes::from(buf))),
                    (file, permit),
                ))
            }
            Err(_) => None,
        }
    });

    let body = StreamBody::new(stream);
    let body: BoxBodyInner = BodyExt::boxed(body);

    builder.body(body.boxed()).unwrap()
}

pub fn planned_response(status: StatusCode, headers: &HeaderMapPlan) -> Response<BoxBodyInner> {
    let mut builder = Response::builder().status(status);
    for header in headers.iter() {
        builder = builder.header(&header.name, &header.value);
    }
    builder.body(full_body("")).unwrap()
}

pub async fn file_response_range(
    mut file: tokio::fs::File,
    start: u64,
    end_inclusive: u64,
    status: StatusCode,
    headers: &HeaderMapPlan,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> Response<BoxBodyInner> {
    use std::io::SeekFrom;
    use tokio::io::AsyncSeekExt;

    let mut builder = Response::builder().status(status);
    for header in headers.iter() {
        builder = builder.header(&header.name, &header.value);
    }

    let len = end_inclusive - start + 1;
    if file.seek(SeekFrom::Start(start)).await.is_err() {
        return planned_response(StatusCode::INTERNAL_SERVER_ERROR, headers);
    }

    let stream = futures_util::stream::unfold(
        (file, permit, len),
        |(mut file, permit, remaining)| async move {
            if remaining == 0 {
                return None;
            }
            let mut buf = vec![0u8; (remaining as usize).min(8192)];
            match tokio::io::AsyncReadExt::read(&mut file, &mut buf).await {
                Ok(0) => None,
                Ok(n) => {
                    let n = (n as u64).min(remaining) as usize;
                    buf.truncate(n);
                    let remaining = remaining.saturating_sub(n as u64);
                    Some((
                        Ok::<_, std::io::Error>(Frame::data(Bytes::from(buf))),
                        (file, permit, remaining),
                    ))
                }
                Err(_) => None,
            }
        },
    );

    let body = StreamBody::new(stream);
    let body: BoxBodyInner = BodyExt::boxed(body);

    builder.body(body.boxed()).unwrap()
}

pub fn directory_listing_response(
    entries: &[(String, bool)],
    is_head: bool,
) -> Response<BoxBodyInner> {
    let mut html = String::from(
        "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>Directory listing</title>\n</head>\n<body>\n<h1>Directory listing</h1>\n<ul>\n",
    );

    for (name, is_dir) in entries {
        let visible = html_escape(name);
        let href = html_escape(&percent_encode_path_segment(name));
        if *is_dir {
            html.push_str(&format!(
                "<li><a href=\"{}/\">{}/</a></li>\n",
                href, visible
            ));
        } else {
            html.push_str(&format!("<li><a href=\"{}\">{}</a></li>\n", href, visible));
        }
    }

    html.push_str("</ul>\n</body>\n</html>\n");

    let body_bytes = html.into_bytes();
    let len = body_bytes.len();

    if is_head {
        return Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "text/html; charset=utf-8")
            .header("content-length", len.to_string())
            .header("x-content-type-options", "nosniff")
            .header(
                "content-security-policy",
                "default-src 'none'; base-uri 'none'; form-action 'none'",
            )
            .header("referrer-policy", "no-referrer")
            .body(full_body(""))
            .unwrap();
    }

    let body = Full::new(Bytes::from(body_bytes));
    let body = body.map_err(|never| match never {});

    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/html; charset=utf-8")
        .header("content-length", len.to_string())
        .header("x-content-type-options", "nosniff")
        .header(
            "content-security-policy",
            "default-src 'none'; base-uri 'none'; form-action 'none'",
        )
        .header("referrer-policy", "no-referrer")
        .body(body.boxed())
        .unwrap()
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            _ => {
                // Skip control characters to prevent terminal injection
                if !c.is_control() {
                    out.push(c);
                }
            }
        }
    }
    out
}

fn percent_encode_path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.as_bytes() {
        let unreserved = matches!(
            byte,
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~'
        );
        if unreserved {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("%{:02X}", byte));
        }
    }
    out
}

fn full_body(s: &'static str) -> BoxBodyInner {
    Full::new(Bytes::from(s))
        .map_err(|never| match never {})
        .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn get_returns_200_with_text_content_type() {
        let resp = text_response(StatusCode::OK, "hello\n");
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "text/plain; charset=utf-8"
        );
    }

    #[test]
    fn head_returns_200_empty_body() {
        let resp = empty_response(StatusCode::OK);
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn post_returns_405_with_allow_header() {
        let resp = method_not_allowed();
        assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(resp.headers().get("allow").unwrap(), "GET, HEAD");
    }

    #[test]
    fn html_escape_escapes_special_chars() {
        assert_eq!(html_escape("foo"), "foo");
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("a&b"), "a&amp;b");
        assert_eq!(html_escape("\"quoted\""), "&quot;quoted&quot;");
        assert_eq!(html_escape("it's"), "it&#x27;s");
    }

    #[test]
    fn percent_encode_path_segment_encodes_url_significant_chars() {
        assert_eq!(percent_encode_path_segment("a b.txt"), "a%20b.txt");
        assert_eq!(percent_encode_path_segment("a?b.txt"), "a%3Fb.txt");
        assert_eq!(percent_encode_path_segment("a#b.txt"), "a%23b.txt");
        assert_eq!(percent_encode_path_segment("a%b.txt"), "a%25b.txt");
        assert_eq!(percent_encode_path_segment("plain.txt"), "plain.txt");
        assert_eq!(percent_encode_path_segment("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn html_escape_strips_control_chars() {
        assert_eq!(html_escape("a\0b"), "ab");
        assert_eq!(html_escape("a\tb"), "ab");
        assert_eq!(html_escape("a\nb"), "ab");
    }

    #[test]
    fn directory_listing_contains_entries() {
        let entries = vec![
            ("file.txt".to_string(), false),
            ("subdir".to_string(), true),
        ];
        let resp = directory_listing_response(&entries, false);
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "text/html; charset=utf-8"
        );
    }

    #[test]
    fn directory_listing_head_has_no_body() {
        let entries = vec![("file.txt".to_string(), false)];
        let resp = directory_listing_response(&entries, true);
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn directory_listing_has_security_headers() {
        let entries = vec![];
        let resp = directory_listing_response(&entries, false);
        assert_eq!(
            resp.headers().get("content-security-policy").unwrap(),
            "default-src 'none'; base-uri 'none'; form-action 'none'"
        );
        assert_eq!(
            resp.headers().get("referrer-policy").unwrap(),
            "no-referrer"
        );
        assert_eq!(
            resp.headers().get("x-content-type-options").unwrap(),
            "nosniff"
        );
    }

    #[test]
    fn property_html_escape_no_script_injection() {
        let malicious = vec![
            "<script>alert(1)</script>",
            "javascript:alert(1)",
            "<img src=x onerror=alert(1)>",
            "\"><script>alert(1)</script>",
            "'-alert(1)-'",
            "<svg onload=alert(1)>",
            "<<script>alert(1)//<</script>",
        ];
        for input in malicious {
            let escaped = html_escape(input);
            assert!(
                !escaped.contains("<script>"),
                "html_escape did not escape <script> in {:?}",
                input
            );
            assert!(
                !escaped.contains("<img"),
                "html_escape did not escape <img tag in {:?}",
                input
            );
            assert!(
                !escaped.contains("<svg"),
                "html_escape did not escape <svg tag in {:?}",
                input
            );
        }
    }

    #[test]
    fn property_html_escape_no_control_chars() {
        let inputs = vec![
            "a\x00b", "a\x01b", "a\x1fb", "a\x7fb", "a\nb", "a\rb", "a\tb",
        ];
        for input in inputs {
            let escaped = html_escape(input);
            assert!(
                !escaped.contains('\0'),
                "NUL in escaped output for {:?}",
                input
            );
            assert!(
                !escaped.contains('\n'),
                "LF in escaped output for {:?}",
                input
            );
            assert!(
                !escaped.contains('\r'),
                "CR in escaped output for {:?}",
                input
            );
        }
    }

    #[test]
    fn property_html_escape_preserves_safe_content() {
        let safe = vec![
            "hello world",
            "foo123",
            "path/to/file.txt",
            "abc-DEF_123",
            "café",
            "日本語",
        ];
        for input in safe {
            let escaped = html_escape(input);
            assert_eq!(
                escaped, input,
                "html_escape modified safe content: {:?}",
                input
            );
        }
    }

    #[test]
    fn property_percent_encode_no_special_chars() {
        let inputs = vec![
            "file.txt",
            "path/to/file",
            "a-b_c.d~e",
            "hello world",
            "a?b",
            "a#b",
            "a%b",
            "a&b=c",
            "a+b",
        ];
        for input in inputs {
            let encoded = percent_encode_path_segment(input);
            // Encoded output must not contain unencoded special chars
            assert!(!encoded.contains('?'), "unencoded ? in {:?}", encoded);
            assert!(!encoded.contains('#'), "unencoded # in {:?}", encoded);
            // Every % must be followed by exactly two hex digits
            let bytes = encoded.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'%' {
                    assert!(
                        i + 2 < bytes.len(),
                        "truncated percent-encoding at end of {:?}",
                        encoded
                    );
                    assert!(
                        bytes[i + 1].is_ascii_hexdigit(),
                        "non-hex digit after %% in {:?}",
                        encoded
                    );
                    assert!(
                        bytes[i + 2].is_ascii_hexdigit(),
                        "non-hex digit after %% in {:?}",
                        encoded
                    );
                    i += 3;
                } else {
                    i += 1;
                }
            }
            // Unreserved chars should be preserved
            for c in input.chars() {
                if matches!(c, 'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~') {
                    assert!(
                        encoded.contains(c),
                        "unreserved char {} lost in encoding: {:?}",
                        c,
                        encoded
                    );
                }
            }
        }
    }

    #[test]
    fn property_directory_listing_well_formed_html() {
        let entries = vec![
            ("file.txt".to_string(), false),
            ("subdir".to_string(), true),
            ("<script>".to_string(), false),
            ("file with spaces.txt".to_string(), false),
        ];
        let resp = directory_listing_response(&entries, false);
        assert_eq!(resp.status(), StatusCode::OK);

        // Security headers present
        assert!(resp.headers().get("content-security-policy").is_some());
        assert!(resp.headers().get("referrer-policy").is_some());
        assert!(resp.headers().get("x-content-type-options").is_some());
        assert_eq!(
            resp.headers().get("x-content-type-options").unwrap(),
            "nosniff"
        );
    }

    proptest::proptest! {
        #[test]
        fn html_escape_never_panics(s in ".*") {
            let _ = html_escape(&s);
        }

        #[test]
        fn html_escape_no_raw_angle_brackets(s in "[<>]+") {
            let escaped = html_escape(&s);
            prop_assert!(!escaped.contains('<'), "raw < in escaped: {:?}", escaped);
            prop_assert!(!escaped.contains('>'), "raw > in escaped: {:?}", escaped);
        }

        #[test]
        fn percent_encode_never_panics(s in ".*") {
            let _ = percent_encode_path_segment(&s);
        }

        #[test]
        fn percent_encode_no_raw_question_or_hash(s in "[?#]+") {
            let encoded = percent_encode_path_segment(&s);
            prop_assert!(!encoded.contains('?'), "raw ? in {:?}", encoded);
            prop_assert!(!encoded.contains('#'), "raw # in {:?}", encoded);
        }
    }
}

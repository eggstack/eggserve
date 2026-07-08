use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::BodyExt;
use http_body_util::Full;
use http_body_util::StreamBody;
use hyper::body::Frame;
use hyper::{Response, StatusCode};
use std::time::SystemTime;

pub type BoxBodyInner = BoxBody<Bytes, std::convert::Infallible>;

pub fn text_response(status: StatusCode, body: &'static str) -> Response<BoxBodyInner> {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .body(full_body(body))
        .unwrap()
}

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
        .header("x-content-type-options", "nosniff");

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
                    Ok::<_, std::convert::Infallible>(Frame::data(Bytes::from(buf))),
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

pub fn file_response_head(
    len: u64,
    mime: &'static str,
    last_modified: Option<SystemTime>,
    etag: Option<String>,
) -> Response<BoxBodyInner> {
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header("content-length", len.to_string())
        .header("content-type", mime)
        .header("x-content-type-options", "nosniff");

    if let Some(mtime) = last_modified {
        if let Ok(secs) = mtime.duration_since(SystemTime::UNIX_EPOCH) {
            let formatted = httpdate::fmt_http_date(SystemTime::UNIX_EPOCH + secs);
            builder = builder.header("last-modified", formatted);
        }
    }

    if let Some(tag) = etag {
        builder = builder.header("etag", tag);
    }

    builder.body(full_body("")).unwrap()
}

pub fn directory_listing_response(
    entries: &[(String, bool)],
    is_head: bool,
) -> Response<BoxBodyInner> {
    let mut html = String::from(
        "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>Directory listing</title>\n</head>\n<body>\n<h1>Directory listing</h1>\n<ul>\n",
    );

    for (name, is_dir) in entries {
        let escaped = html_escape(name);
        if *is_dir {
            html.push_str(&format!(
                "<li><a href=\"{}/\">{}/</a></li>\n",
                escaped, escaped
            ));
        } else {
            html.push_str(&format!(
                "<li><a href=\"{}\">{}</a></li>\n",
                escaped, escaped
            ));
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

fn full_body(s: &'static str) -> BoxBodyInner {
    Full::new(Bytes::from(s))
        .map_err(|never| match never {})
        .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

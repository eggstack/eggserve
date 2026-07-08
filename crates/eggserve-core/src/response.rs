use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use http_body_util::BodyExt;
use http_body_util::Full;
use hyper::{Response, StatusCode};

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
}

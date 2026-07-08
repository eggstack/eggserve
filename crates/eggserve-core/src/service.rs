use hyper::{Method, Request, Response, StatusCode};

use crate::response::BoxBodyInner;
use crate::response::{empty_response, method_not_allowed, text_response};

pub fn handle_request<B>(req: Request<B>) -> Response<BoxBodyInner> {
    match *req.method() {
        Method::GET => text_response(StatusCode::OK, "eggserve placeholder response\n"),
        Method::HEAD => empty_response(StatusCode::OK),
        _ => method_not_allowed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::Empty;
    use hyper::body::Bytes;

    fn empty_req(method: Method) -> Request<Empty<Bytes>> {
        Request::builder()
            .method(method)
            .body(Empty::new())
            .unwrap()
    }

    #[test]
    fn handle_get_returns_200() {
        let resp = handle_request(empty_req(Method::GET));
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn handle_head_returns_200() {
        let resp = handle_request(empty_req(Method::HEAD));
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn handle_post_returns_405() {
        let resp = handle_request(empty_req(Method::POST));
        assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[test]
    fn handle_put_returns_405() {
        let resp = handle_request(empty_req(Method::PUT));
        assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    }
}

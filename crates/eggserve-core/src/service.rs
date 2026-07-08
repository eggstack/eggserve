use hyper::{Method, Request, Response, StatusCode};

use crate::config::ServeConfig;
use crate::path::{ConfinedPath, PathPolicy};
use crate::response::BoxBodyInner;
use crate::response::{empty_response, method_not_allowed, text_response};

pub fn handle_request<B>(req: Request<B>, config: &ServeConfig) -> Response<BoxBodyInner> {
    match *req.method() {
        Method::GET | Method::HEAD => {
            let uri = req.uri();
            let path_str = uri.path();

            let path_policy = PathPolicy {
                dotfiles: match config.static_policy.dotfiles {
                    crate::policy::DotfilePolicy::Denied => PathPolicy::default().dotfiles,
                    crate::policy::DotfilePolicy::Serve => crate::path::DotfilePolicy::Allow,
                },
                reject_backslash: true,
            };

            match ConfinedPath::parse(path_str, &path_policy) {
                Ok(_confined) => {
                    if *req.method() == Method::HEAD {
                        empty_response(StatusCode::OK)
                    } else {
                        text_response(StatusCode::OK, "eggserve placeholder response\n")
                    }
                }
                Err(rejection) => {
                    let is_malformed = matches!(
                        rejection,
                        crate::path::PathRejection::MalformedPercentEncoding
                            | crate::path::PathRejection::InvalidUtf8
                            | crate::path::PathRejection::NulByte
                            | crate::path::PathRejection::Empty
                            | crate::path::PathRejection::UnsupportedUriForm
                            | crate::path::PathRejection::TooLong
                    );

                    if is_malformed {
                        text_response(StatusCode::BAD_REQUEST, "400 Bad Request\n")
                    } else {
                        text_response(StatusCode::FORBIDDEN, "403 Forbidden\n")
                    }
                }
            }
        }
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

    fn req_with_path(method: Method, path: &str) -> Request<Empty<Bytes>> {
        Request::builder()
            .method(method)
            .uri(path)
            .body(Empty::new())
            .unwrap()
    }

    #[test]
    fn handle_get_returns_200() {
        let config = ServeConfig::default();
        let resp = handle_request(empty_req(Method::GET), &config);
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn handle_head_returns_200() {
        let config = ServeConfig::default();
        let resp = handle_request(empty_req(Method::HEAD), &config);
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn handle_post_returns_405() {
        let config = ServeConfig::default();
        let resp = handle_request(empty_req(Method::POST), &config);
        assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[test]
    fn handle_put_returns_405() {
        let config = ServeConfig::default();
        let resp = handle_request(empty_req(Method::PUT), &config);
        assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[test]
    fn handle_get_dotfile_returns_403() {
        let config = ServeConfig::default();
        let resp = handle_request(req_with_path(Method::GET, "/.env"), &config);
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn handle_get_windows_reserved_returns_403() {
        let config = ServeConfig::default();
        let resp = handle_request(req_with_path(Method::GET, "/CON"), &config);
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn handle_get_malformed_percent_returns_400() {
        let config = ServeConfig::default();
        let resp = handle_request(req_with_path(Method::GET, "/%ZZ"), &config);
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}

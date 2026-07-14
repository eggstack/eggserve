//! Compile-time fixture: no Hyper type in the public primitives API.
//!
//! This test verifies that downstream code can use the canonical request
//! types without importing or depending on Hyper. The only exception is
//! `RequestHead::try_from_hyper`, which is the intentional conversion
//! boundary.

use eggserve_core::primitives::header_block::{HeaderBlock, HeaderName, HeaderValue};
use eggserve_core::primitives::method::Method;
use eggserve_core::primitives::request_head::RequestHead;
use eggserve_core::primitives::request_target::RequestTarget;
use eggserve_core::primitives::version::HttpVersion;

#[test]
fn method_construct_and_inspect() {
    let m = Method::new("PURGE").unwrap();
    assert_eq!(m.as_str(), "PURGE");
    assert!(!m.is_safe());
}

#[test]
fn http_version_construct_and_inspect() {
    let v = HttpVersion::Http11;
    assert_eq!(v.as_str(), "HTTP/1.1");
    assert_eq!(v.major(), 1);
    assert_eq!(v.minor(), 1);
}

#[test]
fn header_block_construct_and_inspect() {
    let mut hb = HeaderBlock::new();
    hb.push(
        HeaderName::new("X-Custom").unwrap(),
        HeaderValue::new("value").unwrap(),
    );
    assert!(hb.contains("x-custom"));
    assert_eq!(hb.get_first("X-Custom").unwrap().as_str(), "value");
}

#[test]
fn request_target_construct_and_inspect() {
    let rt = RequestTarget::parse("/path?key=val").unwrap();
    assert_eq!(rt.path(), "/path");
    assert_eq!(rt.query(), Some("key=val"));
}

#[test]
fn request_head_construct_without_hyper() {
    let head = RequestHead::new(
        Method::get(),
        RequestTarget::parse("/").unwrap(),
        HttpVersion::Http11,
        HeaderBlock::new(),
    );
    assert!(head.is_get());
    assert_eq!(head.version(), HttpVersion::Http11);
}

#[test]
fn request_head_from_hyper_is_fallible() {
    let hyper_req = hyper::Request::builder()
        .method("GET")
        .uri("/test?q=1")
        .body(())
        .unwrap();
    let head = RequestHead::try_from_hyper(&hyper_req).unwrap();
    assert_eq!(head.method().as_str(), "GET");
    assert_eq!(head.target().path(), "/test");
    assert_eq!(head.target().query(), Some("q=1"));
}

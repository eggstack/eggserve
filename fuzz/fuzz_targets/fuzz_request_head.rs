#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::primitives::header_block::{HeaderBlock, HeaderName, HeaderValue};
use eggserve_core::primitives::method::Method;
use eggserve_core::primitives::request_head::RequestHead;
use eggserve_core::primitives::request_target::RequestTarget;
use eggserve_core::primitives::version::HttpVersion;

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 {
        return;
    }
    let method_byte = data[0];
    let version_byte = data[1];
    let header_count = (data[2] as usize) % 8;
    let target_byte = data[3];

    let methods = ["GET", "HEAD", "POST", "PUT", "DELETE", "PATCH", "OPTIONS", "TRACE", "CONNECT", "PURGE"];
    let method_idx = method_byte as usize % methods.len();
    let method = match Method::new(methods[method_idx]) {
        Ok(m) => m,
        Err(_) => return,
    };

    let version = match version_byte % 2 {
        0 => HttpVersion::Http10,
        _ => HttpVersion::Http11,
    };

    let target_str = format!("/path-{}", target_byte);
    let target = match RequestTarget::parse(&target_str) {
        Ok(t) => t,
        Err(_) => return,
    };

    let mut headers = HeaderBlock::new();
    for i in 0..header_count {
        let name = match HeaderName::new(&format!("x-h-{}", i)) {
            Ok(n) => n,
            Err(_) => continue,
        };
        let value = match HeaderValue::new(&format!("v-{}", i)) {
            Ok(v) => v,
            Err(_) => continue,
        };
        headers.push(name, value);
    }

    let head = RequestHead::new(method, target, version, headers);

    assert_eq!(head.method().as_str(), methods[method_idx]);
    assert!(!head.target().path().is_empty());
    assert!(head.version() == HttpVersion::Http10 || head.version() == HttpVersion::Http11);
    assert_eq!(head.headers().len(), header_count);

    let cloned = head.clone();
    assert_eq!(cloned.method().as_str(), head.method().as_str());
    assert_eq!(cloned.target().path(), head.target().path());
});

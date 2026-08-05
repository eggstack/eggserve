#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::path::{ConfinedPath, PathPolicy};
use eggserve_core::primitives::header_block::{HeaderBlock, HeaderName, HeaderValue};
use eggserve_core::primitives::http::validate_request_target;
use eggserve_core::primitives::method::Method;
use eggserve_core::primitives::request_head::RequestHead;
use eggserve_core::primitives::request_target::RequestTarget;
use eggserve_core::primitives::version::HttpVersion;

fuzz_target!(|data: &[u8]| {
    if data.len() < 5 {
        return;
    }

    // Fuzz ConfinedPath::parse (request_target coverage)
    if let Ok(s) = std::str::from_utf8(data) {
        let policy = PathPolicy::default();
        if let Ok(confined) = ConfinedPath::parse(s, &policy) {
            for comp in confined.components() {
                assert!(!comp.contains('\0'), "NUL in component: {:?}", comp);
                assert_ne!(comp, "..", "parent component accepted");
                assert_ne!(comp, ".", "current component accepted");
                assert!(!comp.contains('/'), "slash in component: {:?}", comp);
                assert!(!comp.contains('\\'), "backslash in component: {:?}", comp);
            }
            let s = confined.as_str();
            if !s.is_empty() {
                assert!(s.starts_with('/'));
            }
        }

        // Fuzz validate_request_target
        let result = validate_request_target(s);
        if let Ok(()) = result {
            assert!(s.starts_with('/'), "valid target does not start with /: {:?}", s);
            assert!(!s.is_empty(), "empty target passed validation");
            assert!(!s.contains(char::is_whitespace), "whitespace in valid target: {:?}", s);
        }
    }

    // Fuzz RequestHead construction (fuzz_request_head coverage)
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

#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::primitives::canonical::{Response, ResponseBody, StatusCode};
use eggserve_core::primitives::header_block::{HeaderName, HeaderValue};

fuzz_target!(|data: &[u8]| {
    if data.len() < 3 {
        return;
    }
    let status_byte = data[0];
    let header_count = (data[1] as usize) % 8;
    let body_byte = data[2];

    let raw_status = (status_byte as u16 % 899) + 100;
    let status = match StatusCode::new(raw_status) {
        Ok(s) => s,
        Err(_) => return,
    };

    let mut builder = Response::builder().status(status);

    for i in 0..header_count {
        let name = match HeaderName::new(&format!("x-h-{}", i)) {
            Ok(n) => n,
            Err(_) => continue,
        };
        let value = match HeaderValue::new(&format!("v-{}", i)) {
            Ok(v) => v,
            Err(_) => continue,
        };
        builder = builder.push_header(name, value);
    }

    let body = if body_byte % 3 == 0 {
        ResponseBody::Empty
    } else {
        ResponseBody::Bytes(vec![body_byte; body_byte as usize % 64])
    };

    if let Ok(resp) = builder.body(body) {
        assert_eq!(resp.status().as_u16(), raw_status);
        let _ = resp.headers();
    }
});

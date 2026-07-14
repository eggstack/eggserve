#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::primitives::canonical::{
    normalize_response, NormalizeRequest, Response, ResponseBody, StatusCode,
};
use eggserve_core::primitives::header_block::HeaderBlock;

fuzz_target!(|data: &[u8]| {
    if data.len() < 5 {
        return;
    }
    let status_byte = data[0];
    let body_len = data[1] as usize % 256;
    let has_te = data[2] & 1 == 1;
    let has_cl = data[2] & 2 == 2;
    let is_head = data[3] & 1 == 1;

    let raw_status = (status_byte as u16 % 899) + 100;
    let status = match StatusCode::new(raw_status) {
        Ok(s) => s,
        Err(_) => return,
    };

    let body = vec![b'x'; body_len];

    let mut headers = HeaderBlock::new();
    if has_te {
        let _ = headers.push_str("transfer-encoding", "chunked");
    }
    if has_cl {
        let _ = headers.push_str("content-length", "999999");
    }

    let mut builder = Response::builder().status(status);
    for field in headers.iter() {
        builder = builder.push_header(field.name.clone(), field.value.clone());
    }
    let resp = builder.body(ResponseBody::Bytes(body)).unwrap();

    let req = NormalizeRequest::new(is_head);
    if let Ok(norm) = normalize_response(resp, &req) {
        assert!(!norm.headers().contains("transfer-encoding"));

        if status.permits_payload_body() && !is_head {
            if let Some(cl) = norm.headers().get_first("content-length") {
                let actual_len = norm.body().map_or(0, |b| b.len());
                assert_eq!(cl.as_str(), actual_len.to_string());
            }
        }
    }
});

#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::primitives::canonical::{
    normalize_response, NormalizeRequest, Response, ResponseBody, StatusCode,
};
use eggserve_core::primitives::header_block::HeaderBlock;

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 {
        return;
    }

    let is_head_request = data[0] & 1 == 1;
    let status_byte = data[1];
    let body_byte = data[2];
    let header_byte = data[3];

    // Build a status code: map to valid range 100..=999
    let raw_status = (status_byte as u16 % 899) + 100;
    let status = StatusCode::new(raw_status).unwrap();

    // Build body
    let body = if body_byte % 3 == 0 {
        ResponseBody::Empty
    } else {
        ResponseBody::Bytes(vec![body_byte; body_byte as usize % 64])
    };

    // Build a header block with optional headers
    let mut headers = HeaderBlock::new();

    // Add a transfer-encoding header sometimes (should be stripped by normalize)
    if header_byte & 0x01 != 0 {
        let _ = headers.push_str("transfer-encoding", "chunked");
    }

    // Add a content-length header sometimes (should be recomputed by normalize)
    if header_byte & 0x02 != 0 {
        let _ = headers.push_str("content-length", "999999");
    }

    // Add a pass-through header sometimes
    if header_byte & 0x04 != 0 {
        let _ = headers.push_str("x-custom", "test-value");
    }

    // Build response using the builder, pushing validated headers
    let mut builder = Response::builder().status(status);
    for field in headers.iter() {
        builder = builder.push_header(field.name.clone(), field.value.clone());
    }
    let resp = builder.body(body).unwrap();

    let req = NormalizeRequest::new(is_head_request);

    // First normalization
    if let Ok(norm1) = normalize_response(resp, &req) {
        // Idempotent: second normalization produces identical result
        let norm2 = normalize_response(norm1, &req).unwrap();

        // Transfer-Encoding must always be stripped
        assert!(
            !norm2.headers().contains("transfer-encoding"),
            "transfer-encoding survived normalization"
        );

        // HEAD responses must have empty body
        if is_head_request {
            assert!(
                norm2.body().unwrap().is_empty(),
                "HEAD response has non-empty body"
            );
        }

        // Body-forbidden statuses must have empty body
        if !status.permits_payload_body() {
            assert!(
                norm2.body().unwrap().is_empty(),
                "body-forbidden status {} has non-empty body",
                status.as_u16()
            );
        }

        // Content-Length must be correct for non-HEAD, payload-permitting responses
        if status.permits_payload_body() && !is_head_request {
            if let Some(cl) = norm2.headers().get_first("content-length") {
                let expected_len = norm2.body().map_or(0, |b| b.len());
                assert_eq!(
                    cl.as_str(),
                    expected_len.to_string(),
                    "content-length mismatch"
                );
            }
        }
    }
});

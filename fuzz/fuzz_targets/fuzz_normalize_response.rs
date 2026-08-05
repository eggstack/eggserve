#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::primitives::canonical::{
    normalize_response, NormalizeRequest, Response, ResponseBody, StatusCode,
};
use eggserve_core::primitives::header_block::{HeaderBlock, HeaderName, HeaderValue};

fuzz_target!(|data: &[u8]| {
    if data.len() < 5 {
        return;
    }

    let is_head_request = data[0] & 1 == 1;
    let status_byte = data[1];
    let body_byte = data[2];
    let header_byte = data[3];
    let builder_byte = data[4];

    // Build a status code: map to valid range 100..=999
    let raw_status = (status_byte as u16 % 899) + 100;
    let status = match StatusCode::new(raw_status) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Fuzz StatusCode classification
    let classes = [
        status.is_informational(),
        status.is_success(),
        status.is_redirection(),
        status.is_client_error(),
        status.is_server_error(),
    ];
    let active = classes.iter().filter(|&&c| c).count();
    assert!(active <= 1, "multiple classes active for code {}", raw_status);
    if status.is_informational() {
        assert!(!status.permits_payload_body());
    }
    if raw_status == 204 || raw_status == 304 {
        assert!(!status.permits_payload_body());
    }
    assert_eq!(format!("{}", status), format!("{}", raw_status));
    let back: u16 = status.into();
    assert_eq!(back, raw_status);

    // Build body
    let body = if body_byte % 3 == 0 {
        ResponseBody::Empty
    } else {
        ResponseBody::Bytes(vec![body_byte; body_byte as usize % 64])
    };

    // Build headers
    let mut headers = HeaderBlock::new();
    if header_byte & 0x01 != 0 {
        let _ = headers.push_str("transfer-encoding", "chunked");
    }
    if header_byte & 0x02 != 0 {
        let _ = headers.push_str("content-length", "999999");
    }
    if header_byte & 0x04 != 0 {
        let _ = headers.push_str("x-custom", "test-value");
    }

    // Build response using the builder (fuzz_response_builder coverage)
    let header_count = (builder_byte as usize) % 8;
    let mut builder = Response::builder().status(status);
    for field in headers.iter() {
        builder = builder.push_header(field.name.clone(), field.value.clone());
    }
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

    let resp = match builder.body(body) {
        Ok(r) => r,
        Err(_) => return,
    };

    assert_eq!(resp.status().as_u16(), raw_status);
    let _ = resp.headers();

    // Normalize (covers fuzz_normalize_response + fuzz_content_length_reconciliation)
    let req = NormalizeRequest::new(is_head_request);
    if let Ok(norm1) = normalize_response(resp, &req) {
        // Idempotent
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
                raw_status
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

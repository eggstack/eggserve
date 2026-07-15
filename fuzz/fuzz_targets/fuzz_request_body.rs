#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::primitives::request_body::RequestBody;

fuzz_target!(|data: &[u8]| {
    // Test fixed-length body with arbitrary data and limits.
    let max_bytes = if data.len() > 10000 {
        10000
    } else {
        data.len() as u64 + 1
    };
    let body = RequestBody::from_bytes(data.to_vec(), max_bytes);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _ = rt.block_on(body.read_all());

    // Test with a strict limit.
    let body = RequestBody::from_bytes(data.to_vec(), 100);
    let result = rt.block_on(body.read_all());
    if data.len() > 100 {
        assert!(result.is_err());
    }

    // Test empty body.
    let body = RequestBody::empty();
    let result = rt.block_on(body.read_all());
    assert!(result.unwrap().is_empty());

    // Test zero-length body.
    let body = RequestBody::from_bytes(Vec::new(), u64::MAX);
    let result = rt.block_on(body.read_all());
    assert!(result.unwrap().is_empty());

    // Test consumption tracking.
    let body = RequestBody::from_bytes(data.to_vec(), u64::MAX);
    assert!(!body.was_fully_consumed());
    let _ = rt.block_on(body.read_all());
    // After read_all, consumed flag should be set if all bytes received.
});

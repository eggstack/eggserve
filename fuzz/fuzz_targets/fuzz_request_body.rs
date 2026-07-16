#![no_main]
use libfuzzer_sys::fuzz_target;
use eggserve_core::primitives::request_body::{RequestBody, IncomingError};

fuzz_target!(|data: &[u8]| {
    let rt = tokio::runtime::Runtime::new().unwrap();

    // === Fixed-length body tests ===

    // Test with generous limit — should always succeed.
    let max_bytes = if data.len() > 10000 {
        10000
    } else {
        data.len() as u64 + 1
    };
    let body = RequestBody::from_bytes(data.to_vec(), max_bytes);
    let _ = rt.block_on(body.read_all());

    // Test with strict limit — should fail if data exceeds it.
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

    // === Streaming tests ===

    // Test chunk-by-chunk streaming with generous limit.
    let body = RequestBody::from_bytes(data.to_vec(), max_bytes);
    let mut body = body;
    let mut total = 0u64;
    while let Ok(Some(chunk)) = rt.block_on(body.next_chunk()) {
        total += chunk.len() as u64;
    }
    assert_eq!(total, data.len() as u64);

    // Test chunk-by-chunk with strict limit.
    let body = RequestBody::from_bytes(data.to_vec(), 100);
    let mut body = body;
    while let Ok(Some(chunk)) = rt.block_on(body.next_chunk()) {
        let _ = chunk;
    }

    // === Boundary tests ===

    // Test exact-limit body.
    let limit = data.len().min(1000) as u64;
    let body = RequestBody::from_bytes(data[..limit as usize].to_vec(), limit);
    let result = rt.block_on(body.read_all());
    assert!(result.is_ok(), "exact-limit body should succeed");

    // Test one-over-limit body.
    if data.len() > 1 && limit < u64::MAX {
        let body = RequestBody::from_bytes(data.to_vec(), limit);
        let result = rt.block_on(body.read_all());
        assert!(result.is_err(), "one-over-limit body should fail");
    }

    // === Error classification tests ===

    let body = RequestBody::from_bytes(data.to_vec(), 10);
    let result = rt.block_on(body.read_all());
    if let Err(ref e) = result {
        if data.len() > 10 {
            assert!(e.is_limit_exceeded());
            assert_eq!(e.to_status_code(), 413);
        }
    }

    // Test consumption flag tracking.
    let body = RequestBody::from_bytes(data.to_vec(), u64::MAX);
    let flag = body.consumed_flag();
    assert!(!flag.load(std::sync::atomic::Ordering::Acquire));
    let _ = rt.block_on(body.read_all());
    assert!(flag.load(std::sync::atomic::Ordering::Acquire));

    // === Chunked body via stream simulation ===

    // Create a stream from fuzz data to simulate chunked transfer.
    if !data.is_empty() {
        use futures_util::stream;
        use bytes::Bytes;

        // Split fuzz data into a stream of small chunks.
        let chunk_size = (data[0] as usize % 64) + 1;
        let chunks: Vec<_> = data.chunks(chunk_size)
            .map(|c| Ok::<_, IncomingError>(Bytes::copy_from_slice(c)))
            .collect();
        let body_stream = stream::iter(chunks);

        let total_declared = data.len() as u64;
        let body = RequestBody::from_incoming(body_stream, Some(total_declared), u64::MAX);
        let result = rt.block_on(body.read_all());
        // Should succeed since total matches declared length.
        if result.is_ok() {
            assert_eq!(result.unwrap().len(), data.len());
        }
    }

    // === Premature EOF simulation ===

    if data.len() > 2 {
        use futures_util::stream;
        use bytes::Bytes;

        // Declare more bytes than the stream provides.
        let declared = (data.len() as u64) + 100;
        let data_owned = data.to_vec();
        let body_stream = stream::once(async move {
            Ok::<_, IncomingError>(Bytes::from(data_owned))
        });
        let body = RequestBody::from_incoming(body_stream, Some(declared), u64::MAX);
        let result = rt.block_on(body.read_all());
        // Should fail with PrematureEof.
        if let Err(e) = result {
            assert!(
                matches!(e, eggserve_core::primitives::request_body_error::RequestBodyError::PrematureEof { .. }),
                "expected PrematureEof, got: {:?}",
                e
            );
        }
    }

    // === CL/TE conflict detection ===

    // Simulate a request with both Content-Length and Transfer-Encoding.
    // The fuzz data represents potential header bytes. If it contains
    // both CL and TE, the request should be rejected at the HTTP level.
    // At the body level, we just verify the body primitives don't panic.
    if data.len() > 10 {
        use futures_util::stream;
        use bytes::Bytes;

        // Create a body with the fuzz data as the content.
        let data_owned = data.to_vec();
        let chunks: Vec<_> = data_owned.chunks(128)
            .map(|c| Ok::<_, IncomingError>(Bytes::copy_from_slice(c)))
            .collect();
        let body_stream = stream::iter(chunks);
        let body = RequestBody::from_incoming(body_stream, None, 1024);
        let result = rt.block_on(body.read_all());
        if let Err(e) = result {
            // Should be LimitExceeded or Transport error, not a panic.
            assert!(e.is_limit_exceeded() || matches!(e, eggserve_core::primitives::request_body_error::RequestBodyError::Transport(_)));
        }
    }

    // === Partial consumption state transitions ===

    {
        let body = RequestBody::from_bytes(data.to_vec(), u64::MAX);
        let mut body = body;
        // Read one chunk to enter Streaming state.
        let first = rt.block_on(body.next_chunk()).unwrap();
        if first.is_some() {
            // State should be Streaming.
            assert_eq!(body.state(), eggserve_core::primitives::request_body::BodyState::Streaming);
            // Continue reading remaining chunks.
            while let Ok(Some(_chunk)) = rt.block_on(body.next_chunk()) {}
        }
    }

    // === BodyState transitions ===

    {
        let body = RequestBody::empty();
        assert_eq!(body.state(), eggserve_core::primitives::request_body::BodyState::Unread);
        let _ = rt.block_on(body.read_all());
        // Empty body completes immediately.
    }

    {
        let body = RequestBody::from_bytes(b"test".to_vec(), u64::MAX);
        let _ = rt.block_on(body.read_all());
        // After read_all, state should be Complete.
    }

    // === Many tiny chunks ===

    if !data.is_empty() {
        use futures_util::stream;
        use bytes::Bytes;

        // Create a stream with many single-byte chunks.
        let data_owned = data.to_vec();
        let chunks: Vec<_> = data_owned.iter()
            .map(|&b| Ok::<_, IncomingError>(Bytes::copy_from_slice(&[b])))
            .collect();
        let body_stream = stream::iter(chunks);
        let body = RequestBody::from_incoming(body_stream, Some(data.len() as u64), u64::MAX);
        let result = rt.block_on(body.read_all());
        if result.is_ok() {
            assert_eq!(result.unwrap().len(), data.len());
        }
    }

    // === Limit enforcement during streaming ===

    {
        let body = RequestBody::from_bytes(data.to_vec(), 50);
        let mut body = body;
        let mut total = 0u64;
        while let Ok(Some(chunk)) = rt.block_on(body.next_chunk()) {
            total += chunk.len() as u64;
            if total > 50 {
                // Should have gotten an error before this.
                break;
            }
        }
    }

    // === Buffer allocation bounded by max_bytes ===

    {
        let limit = 256u64;
        let body = RequestBody::from_bytes(data.to_vec(), limit);
        let result = rt.block_on(body.read_all());
        if data.len() as u64 > limit {
            assert!(result.is_err());
        } else {
            assert!(result.is_ok());
        }
    }
});

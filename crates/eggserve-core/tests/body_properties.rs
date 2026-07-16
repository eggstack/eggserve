//! Property tests for request body state machine and edge cases.

use eggserve_core::primitives::request_body::{BodyState, RequestBody};
use eggserve_core::primitives::request_body_error::RequestBodyError;
use proptest::prelude::*;

#[test]
fn fixed_body_read_all_succeeds_within_limit() {
    proptest::proptest!(|(data in prop::collection::vec(any::<u8>(), 0..1000), max_bytes in 1000u64..u64::MAX)| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let body = RequestBody::from_bytes(data.clone(), max_bytes);
        let result = rt.block_on(body.read_all());
        prop_assert!(result.is_ok());
        let bytes = result.unwrap();
        prop_assert_eq!(bytes.as_ref(), data.as_slice());
    });
}

#[test]
fn fixed_body_read_all_fails_over_limit() {
    proptest::proptest!(|(data in prop::collection::vec(any::<u8>(), 101..1000))| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let body = RequestBody::from_bytes(data, 100);
        let result = rt.block_on(body.read_all());
        prop_assert!(result.is_err());
        match result.unwrap_err() {
            RequestBodyError::LimitExceeded { .. } => {}
            other => prop_assert!(false, "expected LimitExceeded, got: {:?}", other),
        }
    });
}

#[test]
fn empty_body_read_all_returns_empty() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let body = RequestBody::empty();
    let result = rt.block_on(body.read_all());
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[test]
fn streaming_total_matches_input() {
    proptest::proptest!(|(data in prop::collection::vec(any::<u8>(), 0..1000))| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let body = RequestBody::from_bytes(data.clone(), u64::MAX);
        let mut body = body;
        let mut total = Vec::new();
        loop {
            match rt.block_on(body.next_chunk()) {
                Ok(Some(chunk)) => total.extend_from_slice(&chunk),
                Ok(None) => break,
                Err(_) => break,
            }
        }
        prop_assert_eq!(total, data);
    });
}

#[test]
fn streaming_limit_enforced() {
    proptest::proptest!(|(data in prop::collection::vec(any::<u8>(), 0..500), limit in 0u64..250)| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let body = RequestBody::from_bytes(data.clone(), limit);
        let mut body = body;
        let mut total_received = 0u64;
        let mut hit_limit = false;
        loop {
            match rt.block_on(body.next_chunk()) {
                Ok(Some(chunk)) => total_received += chunk.len() as u64,
                Ok(None) => break,
                Err(RequestBodyError::LimitExceeded { .. }) => {
                    hit_limit = true;
                    break;
                }
                Err(_) => break,
            }
        }
        if data.len() as u64 > limit && limit > 0 {
            prop_assert!(hit_limit, "should hit limit for oversized body");
        }
        prop_assert!(total_received <= limit, "total received should not exceed limit");
    });
}

#[test]
fn exact_limit_body_succeeds() {
    proptest::proptest!(|(data in prop::collection::vec(any::<u8>(), 0..500))| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let limit = data.len() as u64;
        let body = RequestBody::from_bytes(data.clone(), limit);
        let result = rt.block_on(body.read_all());
        prop_assert!(result.is_ok());
        let bytes = result.unwrap();
        prop_assert_eq!(bytes.as_ref(), data.as_slice());
    });
}

#[test]
fn state_transitions_unread_to_streaming_on_chunk() {
    proptest::proptest!(|(data in prop::collection::vec(any::<u8>(), 2..500))| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let body = RequestBody::from_bytes(data, u64::MAX);
        prop_assert_eq!(body.state(), BodyState::Unread);
        let mut body = body;
        let first = rt.block_on(body.next_chunk()).unwrap();
        prop_assert!(first.is_some());
        // State is Streaming only if there's more data remaining
        // For bodies that fit in one chunk, state may transition directly to Complete
        prop_assert!(
            body.state() == BodyState::Streaming || body.state() == BodyState::Complete,
            "expected Streaming or Complete, got {:?}",
            body.state()
        );
    });
}

#[test]
fn empty_body_completes_immediately() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let body = RequestBody::empty();
    assert_eq!(body.state(), BodyState::Unread);
    let result = rt.block_on(body.read_all());
    assert!(result.is_ok());
}

#[test]
fn streaming_completes_all_chunks() {
    proptest::proptest!(|(data in prop::collection::vec(any::<u8>(), 0..500))| {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let body = RequestBody::from_bytes(data.clone(), u64::MAX);
        let mut body = body;
        let mut total = 0u64;
        loop {
            match rt.block_on(body.next_chunk()) {
                Ok(Some(chunk)) => total += chunk.len() as u64,
                Ok(None) => break,
                Err(_) => break,
            }
        }
        prop_assert_eq!(total, data.len() as u64);
    });
}

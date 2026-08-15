use std::fs;
use std::path::Path;

use eggserve_core::primitives::canonical::{
    normalize_response, NormalizeRequest, Response, ResponseBody, StatusCode,
};
use eggserve_core::primitives::header_block::{HeaderBlock, HeaderName, HeaderValue};
use eggserve_core::primitives::http::{validate_method, validate_request_body};
use eggserve_core::primitives::planner::{evaluate_if_none_match, evaluate_range_header};
use eggserve_core::primitives::response::RangeRequestOutcome;
use eggserve_core::primitives::{
    check_component, has_windows_drive_prefix, is_windows_reserved_name, percent_decode,
    ConfinedPath, PathPolicy,
};

const CORPUS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../fuzz/corpus");

fn read_corpus(target: &str) -> Vec<(String, Vec<u8>)> {
    let dir = Path::new(CORPUS_DIR).join(target);
    let mut inputs = Vec::new();
    if !dir.exists() {
        return inputs;
    }
    for entry in fs::read_dir(&dir).expect("read corpus dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let data = fs::read(&path).expect("read corpus file");
        inputs.push((name, data));
    }
    inputs.sort_by(|a, b| a.0.cmp(&b.0));
    inputs
}

#[test]
fn corpus_replay_percent_decode() {
    for (name, data) in read_corpus("percent_decode") {
        let s = match std::str::from_utf8(&data) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if let Ok(decoded) = percent_decode(s) {
            assert!(
                !decoded.contains('\0'),
                "[percent_decode/{name}] NUL byte in decoded output"
            );
            assert!(
                std::str::from_utf8(decoded.as_bytes()).is_ok(),
                "[percent_decode/{name}] output is not valid UTF-8"
            );
            assert!(
                decoded.len() <= s.len() + 1,
                "[percent_decode/{name}] decoded length {} exceeds input length {}",
                decoded.len(),
                s.len()
            );
        }
    }
}

#[test]
fn corpus_replay_request_target() {
    let policy = PathPolicy::default();
    for (name, data) in read_corpus("request_target") {
        let s = match std::str::from_utf8(&data) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if let Ok(confined) = ConfinedPath::parse(s, &policy) {
            for comp in confined.components() {
                assert!(
                    !comp.contains('\0'),
                    "[request_target/{name}] NUL in component: {comp:?}"
                );
                assert_ne!(
                    comp, "..",
                    "[request_target/{name}] parent component accepted"
                );
                assert_ne!(
                    comp, ".",
                    "[request_target/{name}] current component accepted"
                );
                assert!(
                    !comp.contains('/'),
                    "[request_target/{name}] slash in component: {comp:?}"
                );
                assert!(
                    !comp.contains('\\'),
                    "[request_target/{name}] backslash in component: {comp:?}"
                );
            }
            let path = confined.as_str();
            if !path.is_empty() {
                assert!(
                    path.starts_with('/'),
                    "[request_target/{name}] path does not start with /: {path:?}"
                );
            }
        }
    }
}

#[test]
fn corpus_replay_path_components() {
    let policy = PathPolicy::default();
    for (name, data) in read_corpus("path_components") {
        let s = match std::str::from_utf8(&data) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if let Ok(confined) = ConfinedPath::parse(s, &policy) {
            assert!(
                std::str::from_utf8(confined.as_str().as_bytes()).is_ok(),
                "[path_components/{name}] as_str is not valid UTF-8"
            );
            for comp in confined.components() {
                assert!(
                    comp != "..",
                    "[path_components/{name}] parent component accepted: {comp:?}"
                );
                assert!(
                    comp != ".",
                    "[path_components/{name}] current component accepted: {comp:?}"
                );
                assert!(
                    !comp.contains('\0'),
                    "[path_components/{name}] NUL in component: {comp:?}"
                );
                assert!(
                    !comp.contains('/'),
                    "[path_components/{name}] slash in component: {comp:?}"
                );
            }
            let path = confined.as_str();
            if !path.is_empty() {
                assert!(
                    path.starts_with('/'),
                    "[path_components/{name}] path does not start with /: {path:?}"
                );
            }
        }
    }
}

#[test]
fn corpus_replay_validate_method() {
    for (name, data) in read_corpus("validate_method") {
        let s = match std::str::from_utf8(&data) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if let Ok(method) = validate_method(s) {
            assert!(
                method == eggserve_core::primitives::ReadOnlyMethod::Get
                    || method == eggserve_core::primitives::ReadOnlyMethod::Head,
                "[validate_method/{name}] unexpected method: {method:?}"
            );
            assert!(
                method.as_str() == s,
                "[validate_method/{name}] method.as_str() != input"
            );
        }
        let _ = validate_request_body(Some(s), None, 1024);
        let _ = validate_request_body(None, Some(s), 1024);
        let _ = validate_request_body(Some(s), Some(s), 1024);
    }
}

#[test]
fn corpus_replay_if_none_match() {
    for (name, data) in read_corpus("if_none_match") {
        let s = match std::str::from_utf8(&data) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let current_etag = "W/\"100-1234\"";
        let matched = evaluate_if_none_match(s, current_etag);
        if matched {
            if s.trim() == "*" {
                continue;
            }
            let inner = "100-1234";
            let has_match = s.split(',').any(|etag| {
                let etag = etag.trim();
                let etag_inner = etag.strip_prefix("W/").unwrap_or(etag);
                etag_inner == inner
            });
            assert!(has_match, "[if_none_match/{name}] evaluate_if_none_match returned true but no matching token found");
        }
    }
}

#[test]
fn corpus_replay_range_header() {
    for (name, data) in read_corpus("range_header") {
        let s = match std::str::from_utf8(&data) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let file_size = if data.is_empty() {
            100
        } else {
            data[0] as u64 + 1
        };
        let outcome = evaluate_range_header(s, file_size);
        match outcome {
            RangeRequestOutcome::Satisfiable(range) => {
                assert!(
                    range.start < file_size,
                    "[range_header/{name}] start {} beyond file_size {}",
                    range.start,
                    file_size
                );
                assert!(
                    range.end_inclusive < file_size,
                    "[range_header/{name}] end {} beyond file_size {}",
                    range.end_inclusive,
                    file_size
                );
                assert!(
                    range.start <= range.end_inclusive,
                    "[range_header/{name}] start {} > end {}",
                    range.start,
                    range.end_inclusive
                );
                assert!(
                    !range.is_empty(),
                    "[range_header/{name}] Content-Length is zero"
                );
                assert!(
                    range.len() <= file_size,
                    "[range_header/{name}] Content-Length {} exceeds file_size {}",
                    range.len(),
                    file_size
                );
            }
            RangeRequestOutcome::NotSatisfiable => {}
            RangeRequestOutcome::MalformedOrUnsupported => {}
            RangeRequestOutcome::MultipleRanges => {}
        }
    }
}

#[test]
fn corpus_replay_platform_component() {
    for (name, data) in read_corpus("platform_component") {
        let s = match std::str::from_utf8(&data) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let _ = check_component(s);
        let drive = has_windows_drive_prefix(s);
        let reserved = is_windows_reserved_name(s);

        if s.len() < 2 {
            assert!(
                !drive,
                "[platform_component/{name}] drive prefix on short input"
            );
        } else {
            let bytes = s.as_bytes();
            if drive {
                assert!(
                    bytes[0].is_ascii_alphabetic(),
                    "[platform_component/{name}] drive prefix non-alpha first byte"
                );
                assert_eq!(
                    bytes[1], b':',
                    "[platform_component/{name}] drive prefix second byte is not colon"
                );
            }
        }

        if reserved {
            let base = s.split('.').next().unwrap_or("");
            let name_str = base.trim_end_matches('.');
            assert!(
                !name_str.is_empty(),
                "[platform_component/{name}] reserved name with empty base"
            );
            let upper = name_str.to_ascii_uppercase();
            assert!(
                matches!(
                    upper.as_str(),
                    "CON"
                        | "PRN"
                        | "AUX"
                        | "NUL"
                        | "COM1"
                        | "COM2"
                        | "COM3"
                        | "COM4"
                        | "COM5"
                        | "COM6"
                        | "COM7"
                        | "COM8"
                        | "COM9"
                        | "LPT1"
                        | "LPT2"
                        | "LPT3"
                        | "LPT4"
                        | "LPT5"
                        | "LPT6"
                        | "LPT7"
                        | "LPT8"
                        | "LPT9"
                ),
                "[platform_component/{name}] reserved_name returned true for non-reserved: {upper}"
            );
        }
    }
}

#[test]
fn corpus_replay_header_block() {
    for (name, data) in read_corpus("fuzz_header_block") {
        if data.len() < 4 {
            continue;
        }
        let count = (data[0] as usize) % 16;
        let key_byte = data[1];
        let val_byte = data[2];
        let lookup_byte = data[3];

        let mut block = HeaderBlock::new();
        for i in 0..count {
            let name_str = format!("x-{}-{}", key_byte, i);
            let value_str = format!("v-{}-{}", val_byte, i);
            if let (Ok(name), Ok(value)) =
                (HeaderName::new(&name_str), HeaderValue::new(&value_str))
            {
                block.push(name, value);
            }
        }

        let lookup_name = format!("x-{}-0", lookup_byte);
        let first = block.get_first(&lookup_name);
        let all = block.get_all(&lookup_name);
        assert_eq!(block.contains(&lookup_name), !all.is_empty());
        assert_eq!(first, all.first().copied());
        match all.len() {
            0 => assert!(block.get_unique(&lookup_name).unwrap().is_none()),
            1 => assert_eq!(block.get_unique(&lookup_name).unwrap(), first),
            _ => assert!(block.get_unique(&lookup_name).is_err()),
        }

        let mut prev_index = None;
        for (idx, field) in block.iter().enumerate() {
            if let Some(prev) = prev_index {
                assert!(idx > prev);
            }
            prev_index = Some(idx);
            assert!(
                !field.name.as_str().is_empty(),
                "[fuzz_header_block/{name}] empty header name at index {idx}"
            );
        }
    }
}

#[test]
fn corpus_replay_request_body() {
    for (name, data) in read_corpus("fuzz_request_body") {
        let rt = tokio::runtime::Runtime::new().unwrap();

        let body = eggserve_core::primitives::RequestBody::from_bytes(data.clone(), u64::MAX);
        assert_eq!(
            body.state(),
            eggserve_core::primitives::BodyState::Unread,
            "[fuzz_request_body/{name}] initial state not Unread"
        );
        let result = rt.block_on(body.read_all());
        assert!(
            result.is_ok(),
            "[fuzz_request_body/{name}] read_all failed: {:?}",
            result.err()
        );
        let bytes = result.unwrap();
        assert_eq!(
            bytes.len(),
            data.len(),
            "[fuzz_request_body/{name}] read_all length mismatch"
        );

        let body = eggserve_core::primitives::RequestBody::empty();
        let result = rt.block_on(body.read_all());
        assert!(
            result.is_ok(),
            "[fuzz_request_body/{name}] empty read_all failed"
        );
        assert!(
            result.unwrap().is_empty(),
            "[fuzz_request_body/{name}] empty body not empty"
        );

        let max_bytes = if data.len() > 10000 {
            10000
        } else {
            data.len() as u64 + 1
        };
        let body = eggserve_core::primitives::RequestBody::from_bytes(data.clone(), max_bytes);
        let mut body = body;
        let mut total = 0u64;
        while let Ok(Some(chunk)) = rt.block_on(body.next_chunk()) {
            total += chunk.len() as u64;
        }
        assert_eq!(
            total,
            data.len() as u64,
            "[fuzz_request_body/{name}] streaming total mismatch"
        );

        if !data.is_empty() {
            let limit = data.len().min(1000) as u64;
            let body = eggserve_core::primitives::RequestBody::from_bytes(
                data[..limit as usize].to_vec(),
                limit,
            );
            let result = rt.block_on(body.read_all());
            assert!(
                result.is_ok(),
                "[fuzz_request_body/{name}] exact-limit body failed: {:?}",
                result.err()
            );

            let body = eggserve_core::primitives::RequestBody::from_bytes(
                data.clone(),
                data.len() as u64 - 1,
            );
            assert!(
                rt.block_on(body.read_all()).is_err(),
                "[fuzz_request_body/{name}] one-byte-under limit unexpectedly succeeded"
            );
        }

        let body = eggserve_core::primitives::RequestBody::from_bytes(data.clone(), u64::MAX);
        let result = rt.block_on(body.read_all());
        if let Err(ref e) = result {
            assert!(
                e.to_status_code() >= 400 && e.to_status_code() < 600,
                "[fuzz_request_body/{name}] error status code out of range: {}",
                e.to_status_code()
            );
        }
    }
}

#[test]
fn corpus_replay_normalize_response() {
    for (name, data) in read_corpus("fuzz_normalize_response") {
        if data.len() < 4 {
            continue;
        }

        let is_head_request = data[0] & 1 == 1;
        let status_byte = data[1];
        let body_byte = data[2];
        let header_byte = data[3];

        let raw_status = (status_byte as u16 % 899) + 100;
        let status = StatusCode::new(raw_status).unwrap();

        let body = if body_byte % 3 == 0 {
            ResponseBody::Empty
        } else {
            ResponseBody::Bytes(vec![body_byte; body_byte as usize % 64])
        };

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

        let mut builder = Response::builder().status(status);
        for field in headers.iter() {
            builder = builder.push_header(field.name.clone(), field.value.clone());
        }
        let resp = builder.body(body).unwrap();

        let req = NormalizeRequest::new(is_head_request);

        if let Ok(norm1) = normalize_response(resp, &req) {
            let norm2 = normalize_response(norm1, &req).unwrap();

            assert!(
                !norm2.headers().contains("transfer-encoding"),
                "[fuzz_normalize_response/{name}] transfer-encoding survived normalization"
            );

            if is_head_request {
                assert!(
                    norm2.body().unwrap().is_empty(),
                    "[fuzz_normalize_response/{name}] HEAD response has non-empty body"
                );
            }

            if !status.permits_payload_body() {
                assert!(
                    norm2.body().unwrap().is_empty(),
                    "[fuzz_normalize_response/{name}] body-forbidden status {} has non-empty body",
                    status.as_u16()
                );
            }

            if status.permits_payload_body() && !is_head_request {
                if let Some(cl) = norm2.headers().get_first("content-length") {
                    let expected_len = norm2.body().map_or(0, |b| b.len());
                    assert_eq!(
                        cl.as_str(),
                        expected_len.to_string(),
                        "[fuzz_normalize_response/{name}] content-length mismatch"
                    );
                }
            }
        }
    }
}

use std::fs;
use std::path::Path;

use eggserve_core::primitives::http::{
    validate_method, validate_request_body, validate_request_target,
};
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
fn corpus_replay_validate_request_target() {
    for (name, data) in read_corpus("validate_request_target") {
        let s = match std::str::from_utf8(&data) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if let Ok(()) = validate_request_target(s) {
            assert!(
                s.starts_with('/'),
                "[validate_request_target/{name}] valid target does not start with /: {s:?}"
            );
            assert!(
                !s.is_empty(),
                "[validate_request_target/{name}] empty target passed validation"
            );
            assert!(
                !s.contains(char::is_whitespace),
                "[validate_request_target/{name}] whitespace in valid target: {s:?}"
            );
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
#[cfg(feature = "client")]
fn corpus_replay_url_parse() {
    use eggserve_core::primitives::client::{ParsedUrl, Scheme};

    for (name, data) in read_corpus("url_parse") {
        let s = match std::str::from_utf8(&data) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if let Ok(url) = ParsedUrl::parse(s) {
            assert!(
                url.scheme == Scheme::Http || url.scheme == Scheme::Https,
                "[url_parse/{name}] unsupported scheme"
            );
            assert!(!url.host.is_empty(), "[url_parse/{name}] empty host");
            assert!(url.port > 0, "[url_parse/{name}] zero port");
            assert!(
                url.path.starts_with('/'),
                "[url_parse/{name}] path does not start with /"
            );
            assert!(
                !url.path.contains('#'),
                "[url_parse/{name}] fragment in path"
            );

            let authority = url.authority();
            if url.host.contains(':') {
                assert!(
                    authority.starts_with('['),
                    "[url_parse/{name}] IPv6 authority missing brackets"
                );
                assert!(
                    authority.ends_with(']'),
                    "[url_parse/{name}] IPv6 authority missing closing bracket"
                );
            }
            if url.port == url.scheme.default_port() {
                assert!(
                    !authority.contains(':'),
                    "[url_parse/{name}] default port in authority"
                );
            } else {
                assert!(
                    authority.contains(':'),
                    "[url_parse/{name}] non-default port missing from authority"
                );
            }
            assert_eq!(
                url.is_https(),
                url.scheme == Scheme::Https,
                "[url_parse/{name}] is_https inconsistency"
            );
        }
    }
}

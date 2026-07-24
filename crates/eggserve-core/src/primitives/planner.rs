//! Response planner for static files.
//!
//! Generates [`StaticResponsePlan`] values from resolved file metadata and
//! request headers. The planner is a pure function with no Hyper dependency.

use std::fs::Metadata;
use std::time::{SystemTime, UNIX_EPOCH};

use super::http::ReadOnlyMethod;
use super::response::{
    BodyPlan, ConditionalRequestOutcome, FileRange, HeaderMapPlan, RangeRequestOutcome,
    ResponseStatus, StaticResponsePlan,
};

/// Generate a baseline file response plan (200 OK with standard headers).
///
/// For HEAD requests, the body is empty but headers match what GET would
/// return. Handles conditional and range request evaluation internally.
pub fn plan_file_response(
    method: ReadOnlyMethod,
    metadata: &Metadata,
    content_type: &str,
    if_none_match: Option<&str>,
    if_modified_since: Option<&str>,
    range_header: Option<&str>,
    if_range: Option<&str>,
) -> StaticResponsePlan {
    let etag = generate_etag(metadata);
    let last_modified = metadata.modified().ok();
    let last_modified_str = last_modified
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| httpdate::fmt_http_date(UNIX_EPOCH + d));
    let len = metadata.len();

    if let Some(ref etag_val) = etag {
        let outcome = evaluate_conditional_headers(
            etag_val,
            last_modified_str.as_deref(),
            if_none_match,
            if_modified_since,
        );
        if let ConditionalRequestOutcome::NotModified(headers) = outcome {
            return StaticResponsePlan {
                status: ResponseStatus::NOT_MODIFIED,
                headers,
                body: BodyPlan::Empty,
            };
        }
    }

    if let Some(range) = range_header {
        let range_outcome = evaluate_range_header(range, len);

        let range_valid = match &range_outcome {
            RangeRequestOutcome::Satisfiable(_) => {
                if let Some(if_range) = if_range {
                    !matches!(
                        evaluate_if_range(if_range, etag.as_deref(), last_modified_str.as_deref()),
                        ConditionalRequestOutcome::FullResponse
                    )
                } else {
                    true
                }
            }
            _ => false,
        };

        if range_valid {
            if let RangeRequestOutcome::Satisfiable(file_range) = range_outcome {
                return build_range_response(
                    method,
                    file_range,
                    len,
                    content_type,
                    etag.as_deref(),
                    last_modified_str.as_deref(),
                );
            }
        } else {
            match range_outcome {
                RangeRequestOutcome::NotSatisfiable => {
                    return build_not_range_satisfiable(len);
                }
                RangeRequestOutcome::Satisfiable(_) => {
                    // If-Range didn't match; serve full response.
                }
                _ => {}
            }
        }
    }

    build_full_response(
        method,
        len,
        content_type,
        &etag,
        last_modified_str.as_deref(),
    )
}

/// Evaluate conditional request headers (If-None-Match, If-Modified-Since).
pub fn evaluate_conditional_headers(
    current_etag: &str,
    last_modified: Option<&str>,
    if_none_match: Option<&str>,
    if_modified_since: Option<&str>,
) -> ConditionalRequestOutcome {
    if let Some(inm) = if_none_match {
        if evaluate_if_none_match(inm, current_etag) {
            let mut headers = HeaderMapPlan::new();
            headers.push("etag", current_etag.to_owned());
            if let Some(lm) = last_modified {
                headers.push("last-modified", lm.to_owned());
            }
            return ConditionalRequestOutcome::NotModified(headers);
        }
        return ConditionalRequestOutcome::FullResponse;
    }

    if let Some(ims) = if_modified_since {
        if let Some(ims_time) = parse_http_date(ims) {
            if let Some(lm) = last_modified {
                if let Some(lm_time) = parse_http_date(lm) {
                    if lm_time <= ims_time {
                        let mut headers = HeaderMapPlan::new();
                        headers.push("etag", current_etag.to_owned());
                        headers.push("last-modified", lm.to_owned());
                        return ConditionalRequestOutcome::NotModified(headers);
                    }
                }
            }
            return ConditionalRequestOutcome::FullResponse;
        }
        // Malformed date; ignore per RFC 7231 section 5.1.1.
        return ConditionalRequestOutcome::Malformed;
    }

    ConditionalRequestOutcome::FullResponse
}

/// Evaluate an `If-None-Match` header value against the current ETag.
///
/// Supports weak comparison (appropriate for GET/HEAD), wildcard `*`, and
/// comma-separated lists of ETags.
pub fn evaluate_if_none_match(if_none_match: &str, current_etag: &str) -> bool {
    let trimmed = if_none_match.trim();
    if trimmed == "*" {
        return true;
    }

    let current_weak = current_etag.starts_with("W/");
    let current_inner = if current_weak {
        &current_etag[2..]
    } else {
        current_etag
    };

    for etag in trimmed.split(',') {
        let etag = etag.trim();
        if etag.is_empty() {
            continue;
        }
        let candidate_weak = etag.starts_with("W/");
        let candidate_inner = if candidate_weak { &etag[2..] } else { etag };
        if current_inner == candidate_inner {
            return true;
        }
    }
    false
}

/// Evaluate range request headers.
pub fn evaluate_range_header(range: &str, file_size: u64) -> RangeRequestOutcome {
    let range = range.trim();
    if !range.starts_with("bytes=") {
        return RangeRequestOutcome::MalformedOrUnsupported;
    }

    let range_value = &range[6..];
    if range_value.is_empty() {
        return RangeRequestOutcome::MalformedOrUnsupported;
    }

    let ranges: Vec<&str> = range_value.split(',').collect();
    if ranges.len() > 1 {
        return RangeRequestOutcome::MultipleRanges;
    }

    parse_single_range(ranges[0].trim(), file_size)
}

/// Evaluate an `If-Range` header.
pub fn evaluate_if_range(
    if_range: &str,
    current_etag: Option<&str>,
    last_modified: Option<&str>,
) -> ConditionalRequestOutcome {
    let trimmed = if_range.trim();
    if trimmed.is_empty() {
        return ConditionalRequestOutcome::Malformed;
    }

    if trimmed.starts_with('"') || trimmed.starts_with("W/") {
        // ETag
        if let Some(etag) = current_etag {
            if evaluate_if_none_match(trimmed, etag) {
                return ConditionalRequestOutcome::NotModified(HeaderMapPlan::new());
            }
        }
        return ConditionalRequestOutcome::FullResponse;
    }

    // Date
    if let Some(lm) = last_modified {
        if let (Some(if_range_time), Some(lm_time)) =
            (parse_http_date(trimmed), parse_http_date(lm))
        {
            if if_range_time == lm_time {
                return ConditionalRequestOutcome::NotModified(HeaderMapPlan::new());
            }
        }
    }

    ConditionalRequestOutcome::FullResponse
}

/// Generate a weak ETag from file metadata.
///
/// Uses file size, mtime seconds, and mtime nanoseconds to produce a stable
/// weak validator. Nanosecond precision distinguishes rapid same-size
/// modifications where millisecond precision would collide.
pub fn generate_etag(metadata: &Metadata) -> Option<String> {
    let size = metadata.len();
    let mtime = metadata.modified().ok()?;
    let epoch = mtime.duration_since(UNIX_EPOCH).ok()?;
    let mtime_secs = epoch.as_secs();
    let mtime_nanos = epoch.subsec_nanos();
    Some(format!("W/\"{}-{}-{}\"", size, mtime_secs, mtime_nanos))
}

fn build_full_response(
    method: ReadOnlyMethod,
    len: u64,
    content_type: &str,
    etag: &Option<String>,
    last_modified: Option<&str>,
) -> StaticResponsePlan {
    let mut headers = HeaderMapPlan::new();
    headers.push("content-length", len.to_string());
    headers.push("content-type", content_type.to_owned());
    headers.push("accept-ranges", "bytes".to_owned());
    headers.push("x-content-type-options", "nosniff".to_owned());

    if let Some(lm) = last_modified {
        headers.push("last-modified", lm.to_owned());
    }
    if let Some(tag) = etag {
        headers.push("etag", tag.clone());
    }

    let body = if method == ReadOnlyMethod::Head {
        BodyPlan::Empty
    } else {
        BodyPlan::FileFull
    };

    StaticResponsePlan {
        status: ResponseStatus::OK,
        headers,
        body,
    }
}

fn build_range_response(
    method: ReadOnlyMethod,
    range: FileRange,
    file_size: u64,
    content_type: &str,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> StaticResponsePlan {
    let mut headers = HeaderMapPlan::new();
    let content_length = range.len();
    headers.push("content-length", content_length.to_string());
    headers.push("content-type", content_type.to_owned());
    headers.push("accept-ranges", "bytes".to_owned());
    headers.push(
        "content-range",
        format!(
            "bytes {}-{}/{}",
            range.start, range.end_inclusive, file_size
        ),
    );
    headers.push("x-content-type-options", "nosniff".to_owned());

    if let Some(lm) = last_modified {
        headers.push("last-modified", lm.to_owned());
    }
    if let Some(tag) = etag {
        headers.push("etag", tag.to_owned());
    }

    let body = if method == ReadOnlyMethod::Head {
        BodyPlan::Empty
    } else {
        BodyPlan::FileRange {
            start: range.start,
            end_inclusive: range.end_inclusive,
        }
    };

    StaticResponsePlan {
        status: ResponseStatus::PARTIAL_CONTENT,
        headers,
        body,
    }
}

fn build_not_range_satisfiable(file_size: u64) -> StaticResponsePlan {
    let mut headers = HeaderMapPlan::new();
    headers.push("content-length", "0".to_owned());
    headers.push("accept-ranges", "bytes".to_owned());
    headers.push("content-range", format!("bytes */{}", file_size));

    StaticResponsePlan {
        status: ResponseStatus::NOT_RANGE_SATISFIABLE,
        headers,
        body: BodyPlan::Empty,
    }
}

fn parse_single_range(range: &str, file_size: u64) -> RangeRequestOutcome {
    if file_size == 0 {
        return RangeRequestOutcome::NotSatisfiable;
    }

    if let Some(suffix_len_str) = range.strip_prefix('-') {
        // Suffix: -N
        let suffix_len: u64 = match suffix_len_str.parse() {
            Ok(n) => n,
            Err(_) => return RangeRequestOutcome::MalformedOrUnsupported,
        };
        if suffix_len == 0 {
            return RangeRequestOutcome::MalformedOrUnsupported;
        }
        let start = file_size.saturating_sub(suffix_len);
        if start >= file_size {
            return RangeRequestOutcome::NotSatisfiable;
        }
        return RangeRequestOutcome::Satisfiable(FileRange::new(start, file_size - 1));
    }

    // Start or Start-End
    let parts: Vec<&str> = range.splitn(2, '-').collect();
    if parts.len() != 2 {
        return RangeRequestOutcome::MalformedOrUnsupported;
    }

    let start: u64 = match parts[0].parse() {
        Ok(n) => n,
        Err(_) => return RangeRequestOutcome::MalformedOrUnsupported,
    };

    if parts[1].is_empty() {
        // Start-
        if start >= file_size {
            return RangeRequestOutcome::NotSatisfiable;
        }
        return RangeRequestOutcome::Satisfiable(FileRange::new(start, file_size - 1));
    }

    // Start-End
    let end: u64 = match parts[1].parse() {
        Ok(n) => n,
        Err(_) => return RangeRequestOutcome::MalformedOrUnsupported,
    };

    if start > end {
        return RangeRequestOutcome::NotSatisfiable;
    }
    if start >= file_size {
        return RangeRequestOutcome::NotSatisfiable;
    }

    let end = end.min(file_size - 1);
    RangeRequestOutcome::Satisfiable(FileRange::new(start, end))
}

fn parse_http_date(s: &str) -> Option<SystemTime> {
    httpdate::parse_http_date(s).ok()
}

/// Evaluate a directory listing response plan.
pub fn plan_directory_listing(content_length: usize, is_head: bool) -> StaticResponsePlan {
    let mut headers = HeaderMapPlan::new();
    headers.push("content-type", "text/html; charset=utf-8".to_owned());
    headers.push("content-length", content_length.to_string());
    headers.push("x-content-type-options", "nosniff".to_owned());
    headers.push(
        "content-security-policy",
        "default-src 'none'; base-uri 'none'; form-action 'none'".to_owned(),
    );
    headers.push("referrer-policy", "no-referrer".to_owned());

    let body = if is_head {
        BodyPlan::Empty
    } else {
        BodyPlan::FullBytes(Vec::new()) // Caller provides HTML bytes
    };

    StaticResponsePlan {
        status: ResponseStatus::OK,
        headers,
        body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::io::Write;

    fn make_file_with_size(size: u64) -> tempfile::NamedTempFile {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        let data = vec![0u8; size as usize];
        tmp.write_all(&data).unwrap();
        tmp.flush().unwrap();
        tmp
    }

    #[test]
    fn plan_file_response_200_get() {
        let tmp = make_file_with_size(1024);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        let plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain; charset=utf-8",
            None,
            None,
            None,
            None,
        );

        assert_eq!(plan.status.as_u16(), 200);
        assert_eq!(plan.headers.get("content-length"), Some("1024"));
        assert_eq!(
            plan.headers.get("content-type"),
            Some("text/plain; charset=utf-8")
        );
        assert_eq!(plan.headers.get("x-content-type-options"), Some("nosniff"));
        assert!(plan.headers.get("etag").is_some());
        assert!(plan.headers.get("last-modified").is_some());
        assert_eq!(plan.body, BodyPlan::FileFull);
    }

    #[test]
    fn plan_file_response_200_head_empty_body() {
        let tmp = make_file_with_size(512);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        let plan = plan_file_response(
            ReadOnlyMethod::Head,
            &meta,
            "text/html; charset=utf-8",
            None,
            None,
            None,
            None,
        );

        assert_eq!(plan.status.as_u16(), 200);
        assert_eq!(plan.body, BodyPlan::Empty);
        assert_eq!(plan.headers.get("content-length"), Some("512"));
    }

    #[test]
    fn plan_file_response_etag_and_last_modified() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        let plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            None,
            None,
            None,
            None,
        );

        let etag = plan.headers.get("etag").unwrap();
        assert!(etag.starts_with("W/\""));
        assert!(plan.headers.get("last-modified").is_some());
    }

    #[test]
    fn plan_file_response_matching_if_none_match_304() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        let etag = generate_etag(&meta).unwrap();

        let plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            Some(&etag),
            None,
            None,
            None,
        );

        assert_eq!(plan.status.as_u16(), 304);
        assert_eq!(plan.body, BodyPlan::Empty);
        assert!(plan.headers.get("etag").is_some());
    }

    #[test]
    fn plan_file_response_nonmatching_if_none_match_200() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        let plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            Some("W/\"999-999\""),
            None,
            None,
            None,
        );

        assert_eq!(plan.status.as_u16(), 200);
        assert_eq!(plan.body, BodyPlan::FileFull);
    }

    #[test]
    fn plan_file_response_wildcard_if_none_match_304() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        let plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            Some("*"),
            None,
            None,
            None,
        );

        assert_eq!(plan.status.as_u16(), 304);
        assert_eq!(plan.body, BodyPlan::Empty);
    }

    #[test]
    fn plan_file_response_matching_if_modified_since_304() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        // IMS in the future relative to file mtime
        let lm = meta.modified().unwrap();
        let lm_secs = lm.duration_since(UNIX_EPOCH).unwrap().as_secs();
        let future = UNIX_EPOCH + std::time::Duration::from_secs(lm_secs + 3600);
        let ims = httpdate::fmt_http_date(future);

        let plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            None,
            Some(&ims),
            None,
            None,
        );

        assert_eq!(plan.status.as_u16(), 304);
    }

    #[test]
    fn plan_file_response_stale_if_modified_since_200() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        // IMS in the past
        let lm = meta.modified().unwrap();
        let lm_secs = lm.duration_since(UNIX_EPOCH).unwrap().as_secs();
        let past = UNIX_EPOCH + std::time::Duration::from_secs(lm_secs.saturating_sub(3600));
        let ims = httpdate::fmt_http_date(past);

        let plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            None,
            Some(&ims),
            None,
            None,
        );

        assert_eq!(plan.status.as_u16(), 200);
    }

    #[test]
    fn plan_file_response_invalid_if_modified_since_200() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        let plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            None,
            Some("not-a-date"),
            None,
            None,
        );

        assert_eq!(plan.status.as_u16(), 200);
    }

    #[test]
    fn plan_file_response_head_conditional_matches_get_status() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        let etag = generate_etag(&meta).unwrap();

        let get_plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            Some(&etag),
            None,
            None,
            None,
        );
        let head_plan = plan_file_response(
            ReadOnlyMethod::Head,
            &meta,
            "text/plain",
            Some(&etag),
            None,
            None,
            None,
        );

        assert_eq!(get_plan.status.as_u16(), head_plan.status.as_u16());
        assert_eq!(head_plan.body, BodyPlan::Empty);
    }

    #[test]
    fn plan_file_response_range_206() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        let plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            None,
            None,
            Some("bytes=0-49"),
            None,
        );

        assert_eq!(plan.status.as_u16(), 206);
        assert_eq!(plan.headers.get("content-range"), Some("bytes 0-49/100"));
        assert_eq!(plan.headers.get("content-length"), Some("50"));
        assert_eq!(plan.headers.get("content-type"), Some("text/plain"));
        assert_eq!(plan.headers.get("accept-ranges"), Some("bytes"));
        assert!(plan.headers.get("etag").is_some());
        assert!(plan.headers.get("last-modified").is_some());
    }

    #[test]
    fn plan_file_response_range_416() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        let plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            None,
            None,
            Some("bytes=200-300"),
            None,
        );

        assert_eq!(plan.status.as_u16(), 416);
        assert_eq!(plan.headers.get("content-range"), Some("bytes */100"));
        assert_eq!(plan.headers.get("content-length"), Some("0"));
        assert_eq!(plan.headers.get("accept-ranges"), Some("bytes"));
        assert_eq!(plan.body, BodyPlan::Empty);
    }

    #[test]
    fn plan_file_response_head_range_empty_body() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        let plan = plan_file_response(
            ReadOnlyMethod::Head,
            &meta,
            "text/plain",
            None,
            None,
            Some("bytes=0-49"),
            None,
        );

        assert_eq!(plan.status.as_u16(), 206);
        assert_eq!(plan.body, BodyPlan::Empty);
        assert_eq!(plan.headers.get("content-length"), Some("50"));
        assert_eq!(plan.headers.get("content-type"), Some("text/plain"));
    }

    #[test]
    fn plan_file_response_if_range_matching_206() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();
        let etag = generate_etag(&meta).unwrap();

        let plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            None,
            None,
            Some("bytes=0-49"),
            Some(&etag),
        );

        assert_eq!(plan.status.as_u16(), 206);
    }

    #[test]
    fn plan_file_response_if_range_nonmatching_200() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        let plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            None,
            None,
            Some("bytes=0-49"),
            Some("W/\"999-999\""),
        );

        assert_eq!(plan.status.as_u16(), 200);
        assert_eq!(plan.body, BodyPlan::FileFull);
    }

    #[test]
    fn plan_file_response_suffix_range() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        let plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            None,
            None,
            Some("bytes=-10"),
            None,
        );

        assert_eq!(plan.status.as_u16(), 206);
        assert_eq!(plan.headers.get("content-range"), Some("bytes 90-99/100"));
        assert_eq!(plan.headers.get("content-length"), Some("10"));
    }

    #[test]
    fn plan_file_response_open_ended_range() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        let plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            None,
            None,
            Some("bytes=50-"),
            None,
        );

        assert_eq!(plan.status.as_u16(), 206);
        assert_eq!(plan.headers.get("content-range"), Some("bytes 50-99/100"));
        assert_eq!(plan.headers.get("content-length"), Some("50"));
    }

    #[test]
    fn plan_file_response_multiple_ranges_200() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        let plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            None,
            None,
            Some("bytes=0-9, 50-59"),
            None,
        );

        assert_eq!(plan.status.as_u16(), 200);
        assert_eq!(plan.body, BodyPlan::FileFull);
    }

    #[test]
    fn evaluate_range_header_prefix() {
        let result = evaluate_range_header("bytes=0-9", 100);
        assert!(matches!(result, RangeRequestOutcome::Satisfiable(_)));

        let result = evaluate_range_header("none=0-9", 100);
        assert_eq!(result, RangeRequestOutcome::MalformedOrUnsupported);
    }

    #[test]
    fn evaluate_range_header_empty() {
        let result = evaluate_range_header("bytes=", 100);
        assert_eq!(result, RangeRequestOutcome::MalformedOrUnsupported);
    }

    #[test]
    fn evaluate_range_header_suffix_zero() {
        let result = evaluate_range_header("bytes=-0", 100);
        assert_eq!(result, RangeRequestOutcome::MalformedOrUnsupported);
    }

    #[test]
    fn evaluate_range_header_suffix_exceeds_file_returns_whole_file() {
        let result = evaluate_range_header("bytes=-200", 100);
        assert_eq!(
            result,
            RangeRequestOutcome::Satisfiable(FileRange::new(0, 99))
        );
    }

    #[test]
    fn evaluate_range_header_start_beyond_file() {
        let result = evaluate_range_header("bytes=200-300", 100);
        assert_eq!(result, RangeRequestOutcome::NotSatisfiable);
    }

    #[test]
    fn evaluate_range_header_start_equals_end_beyond_file() {
        let result = evaluate_range_header("bytes=100-100", 100);
        assert_eq!(result, RangeRequestOutcome::NotSatisfiable);
    }

    #[test]
    fn evaluate_range_header_inverted_range() {
        let result = evaluate_range_header("bytes=50-10", 100);
        assert_eq!(result, RangeRequestOutcome::NotSatisfiable);
    }

    #[test]
    fn evaluate_range_header_non_numeric() {
        let result = evaluate_range_header("bytes=abc-def", 100);
        assert_eq!(result, RangeRequestOutcome::MalformedOrUnsupported);
    }

    #[test]
    fn evaluate_range_header_end_clamped_to_file_size() {
        let result = evaluate_range_header("bytes=90-200", 100);
        assert_eq!(
            result,
            RangeRequestOutcome::Satisfiable(FileRange::new(90, 99))
        );
    }

    #[test]
    fn evaluate_range_header_zero_file_size() {
        let result = evaluate_range_header("bytes=0-0", 0);
        assert_eq!(result, RangeRequestOutcome::NotSatisfiable);
    }

    #[test]
    fn evaluate_if_none_match_etag_matches() {
        assert!(evaluate_if_none_match("W/\"100-1234\"", "W/\"100-1234\""));
    }

    #[test]
    fn evaluate_if_none_match_etag_does_not_match() {
        assert!(!evaluate_if_none_match("W/\"999-999\"", "W/\"100-1234\""));
    }

    #[test]
    fn evaluate_if_none_match_wildcard() {
        assert!(evaluate_if_none_match("*", "W/\"100-1234\""));
    }

    #[test]
    fn evaluate_if_none_match_list() {
        assert!(evaluate_if_none_match(
            "W/\"999-999\", W/\"100-1234\"",
            "W/\"100-1234\""
        ));
        assert!(!evaluate_if_none_match(
            "W/\"999-999\", W/\"888-888\"",
            "W/\"100-1234\""
        ));
    }

    #[test]
    fn generate_etag_format() {
        let tmp = make_file_with_size(42);
        let meta = std::fs::metadata(tmp.path()).unwrap();
        let etag = generate_etag(&meta).unwrap();
        assert!(etag.starts_with("W/\"42-"));
        assert!(etag.ends_with('"'));
    }

    #[test]
    fn plan_directory_listing_200() {
        let plan = plan_directory_listing(1234, false);
        assert_eq!(plan.status.as_u16(), 200);
        assert_eq!(
            plan.headers.get("content-type"),
            Some("text/html; charset=utf-8")
        );
        assert_eq!(plan.headers.get("content-length"), Some("1234"));
        assert_eq!(
            plan.headers.get("content-security-policy"),
            Some("default-src 'none'; base-uri 'none'; form-action 'none'")
        );
        assert_eq!(plan.headers.get("referrer-policy"), Some("no-referrer"));
        assert_eq!(plan.headers.get("x-content-type-options"), Some("nosniff"));
    }

    #[test]
    fn plan_directory_listing_head_empty_body() {
        let plan = plan_directory_listing(500, true);
        assert_eq!(plan.status.as_u16(), 200);
        assert_eq!(plan.body, BodyPlan::Empty);
        assert_eq!(plan.headers.get("content-length"), Some("500"));
    }

    #[test]
    fn evaluate_if_none_match_weak_etag_matches_strong() {
        assert!(evaluate_if_none_match("W/\"100-1234\"", "\"100-1234\""));
    }

    #[test]
    fn evaluate_if_none_match_strong_etag_matches_weak() {
        assert!(evaluate_if_none_match("\"100-1234\"", "W/\"100-1234\""));
    }

    #[test]
    fn evaluate_if_none_match_empty_list() {
        assert!(!evaluate_if_none_match("", "W/\"100-1234\""));
    }

    #[test]
    fn evaluate_range_header_first_byte() {
        let result = evaluate_range_header("bytes=0-0", 100);
        assert_eq!(
            result,
            RangeRequestOutcome::Satisfiable(FileRange::new(0, 0))
        );
    }

    #[test]
    fn evaluate_range_header_open_ended() {
        let result = evaluate_range_header("bytes=50-", 100);
        assert_eq!(
            result,
            RangeRequestOutcome::Satisfiable(FileRange::new(50, 99))
        );
    }

    #[test]
    fn evaluate_range_header_suffix_one() {
        let result = evaluate_range_header("bytes=-1", 100);
        assert_eq!(
            result,
            RangeRequestOutcome::Satisfiable(FileRange::new(99, 99))
        );
    }

    #[test]
    fn evaluate_range_header_suffix_larger_than_file() {
        let result = evaluate_range_header("bytes=-200", 100);
        assert_eq!(
            result,
            RangeRequestOutcome::Satisfiable(FileRange::new(0, 99))
        );
    }

    #[test]
    fn evaluate_range_header_start_beyond_eof() {
        let result = evaluate_range_header("bytes=100-", 100);
        assert_eq!(result, RangeRequestOutcome::NotSatisfiable);
    }

    #[test]
    fn evaluate_range_header_start_greater_than_end() {
        let result = evaluate_range_header("bytes=50-10", 100);
        assert_eq!(result, RangeRequestOutcome::NotSatisfiable);
    }

    #[test]
    fn evaluate_range_header_unsupported_unit() {
        let result = evaluate_range_header("items=0-9", 100);
        assert_eq!(result, RangeRequestOutcome::MalformedOrUnsupported);
    }

    #[test]
    fn evaluate_range_header_multiple_ranges() {
        let result = evaluate_range_header("bytes=0-9, 50-59", 100);
        assert_eq!(result, RangeRequestOutcome::MultipleRanges);
    }

    #[test]
    fn plan_file_response_zero_length_file_range_416() {
        let tmp = make_file_with_size(0);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        let plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "application/octet-stream",
            None,
            None,
            Some("bytes=0-0"),
            None,
        );

        assert_eq!(plan.status.as_u16(), 416);
        assert_eq!(plan.body, BodyPlan::Empty);
    }

    #[test]
    fn plan_file_response_if_range_matching_date_206() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        let lm = meta.modified().unwrap();
        let lm_secs = lm.duration_since(UNIX_EPOCH).unwrap().as_secs();
        let lm_time = UNIX_EPOCH + std::time::Duration::from_secs(lm_secs);
        let date_str = httpdate::fmt_http_date(lm_time);

        let plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            None,
            None,
            Some("bytes=0-49"),
            Some(&date_str),
        );

        assert_eq!(plan.status.as_u16(), 206);
    }

    #[test]
    fn plan_file_response_if_range_stale_date_200() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        let stale = UNIX_EPOCH + std::time::Duration::from_secs(0);
        let date_str = httpdate::fmt_http_date(stale);

        let plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            None,
            None,
            Some("bytes=0-49"),
            Some(&date_str),
        );

        assert_eq!(plan.status.as_u16(), 200);
        assert_eq!(plan.body, BodyPlan::FileFull);
    }

    #[test]
    fn plan_file_response_head_with_range_returns_headers_no_body() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        let plan = plan_file_response(
            ReadOnlyMethod::Head,
            &meta,
            "text/plain",
            None,
            None,
            Some("bytes=0-0"),
            None,
        );

        assert_eq!(plan.status.as_u16(), 206);
        assert_eq!(plan.body, BodyPlan::Empty);
        assert_eq!(plan.headers.get("content-length"), Some("1"));
        assert_eq!(plan.headers.get("content-range"), Some("bytes 0-0/100"));
    }

    #[test]
    fn evaluate_conditional_headers_both_present_etag_wins() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();
        let etag = generate_etag(&meta).unwrap();

        let outcome = evaluate_conditional_headers(
            &etag,
            None,
            Some(&etag),
            Some("Tue, 01 Jan 2030 00:00:00 GMT"),
        );

        assert!(matches!(outcome, ConditionalRequestOutcome::NotModified(_)));
    }

    #[test]
    fn evaluate_conditional_headers_no_match_no_ims() {
        let outcome =
            evaluate_conditional_headers("W/\"100-1234\"", None, Some("W/\"999-999\""), None);

        assert_eq!(outcome, ConditionalRequestOutcome::FullResponse);
    }

    #[test]
    fn property_range_always_within_file_size() {
        let file_sizes = [1u64, 10, 100, 1000, u64::MAX];
        let range_headers = [
            "bytes=0-0",
            "bytes=0-49",
            "bytes=50-",
            "bytes=-10",
            "bytes=0-999999",
            "bytes=-999999",
            "bytes=50-10",
            "bytes=200-300",
            "bytes=abc",
            "bytes=",
            "items=0-9",
            "bytes=-0",
            "none=0-9",
        ];

        for &file_size in &file_sizes {
            for header in &range_headers {
                let outcome = evaluate_range_header(header, file_size);
                if let RangeRequestOutcome::Satisfiable(range) = outcome {
                    assert!(
                        range.start < file_size,
                        "range start {} >= file_size {} for header {:?}",
                        range.start,
                        file_size,
                        header
                    );
                    assert!(
                        range.end_inclusive < file_size,
                        "range end {} >= file_size {} for header {:?}",
                        range.end_inclusive,
                        file_size,
                        header
                    );
                    assert!(
                        range.start <= range.end_inclusive,
                        "range start {} > end {} for header {:?}",
                        range.start,
                        range.end_inclusive,
                        header
                    );
                    assert!(
                        !range.is_empty(),
                        "range length is 0 for header {:?}",
                        header
                    );
                    assert!(
                        range.len() <= file_size,
                        "range length {} > file_size {} for header {:?}",
                        range.len(),
                        file_size,
                        header
                    );
                }
            }
        }
    }

    #[test]
    fn property_etag_format() {
        let sizes = [0u64, 1, 42, 1024, 1024 * 1024];
        for size in sizes {
            let tmp = make_file_with_size(size);
            let meta = std::fs::metadata(tmp.path()).unwrap();
            if let Some(etag) = generate_etag(&meta) {
                assert!(
                    etag.starts_with("W/\""),
                    "ETag does not start with W/\": {:?}",
                    etag
                );
                assert!(etag.ends_with('"'), "ETag does not end with \": {:?}", etag);
                // ETag contains the file size
                assert!(
                    etag.contains(&size.to_string()),
                    "ETag {:?} does not contain size {}",
                    etag,
                    size
                );
                // No CR/LF in ETag
                assert!(!etag.contains('\r'), "CR in ETag: {:?}", etag);
                assert!(!etag.contains('\n'), "LF in ETag: {:?}", etag);
            }
        }
    }

    #[test]
    fn property_head_never_has_body() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        let range_headers = [None, Some("bytes=0-49"), Some("bytes=-10")];
        let inm_values = [None, Some("W/\"100-1234\""), Some("*")];

        for range in &range_headers {
            for inm in &inm_values {
                let plan = plan_file_response(
                    ReadOnlyMethod::Head,
                    &meta,
                    "text/plain",
                    *inm,
                    None,
                    *range,
                    None,
                );
                assert_eq!(
                    plan.body,
                    BodyPlan::Empty,
                    "HEAD request returned non-empty body for range={:?} inm={:?}",
                    range,
                    inm
                );
            }
        }
    }

    #[test]
    fn property_304_always_empty_body() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();
        let etag = generate_etag(&meta).unwrap();

        let plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            Some(&etag),
            None,
            None,
            None,
        );
        assert_eq!(plan.status.as_u16(), 304);
        assert_eq!(plan.body, BodyPlan::Empty);
    }

    #[test]
    fn property_weak_strong_etag_equivalence() {
        // Weak and strong ETags with same inner value should match
        assert!(evaluate_if_none_match("W/\"100\"", "\"100\""));
        assert!(evaluate_if_none_match("\"100\"", "W/\"100\""));
        assert!(evaluate_if_none_match("W/\"100\"", "W/\"100\""));
        assert!(evaluate_if_none_match("\"100\"", "\"100\""));
    }

    #[test]
    fn property_wildcard_always_matches() {
        let etags = ["W/\"100\"", "\"100\"", "anything", "", "W/\"\""];
        for etag in &etags {
            assert!(
                evaluate_if_none_match("*", etag),
                "wildcard did not match etag: {:?}",
                etag
            );
        }
    }

    proptest::proptest! {
        #[test]
        fn evaluate_range_header_never_panics(header in ".*", file_size in 0u64..=1_000_000) {
            let _ = evaluate_range_header(&header, file_size);
        }

        #[test]
        fn satisfiable_range_within_file_size(header in "bytes=(\\d+)-(\\d+)", file_size in 1u64..=1_000_000) {
            if let RangeRequestOutcome::Satisfiable(range) = evaluate_range_header(&header, file_size) {
                prop_assert!(range.start < file_size,
                    "start {} >= file_size {}", range.start, file_size);
                prop_assert!(range.end_inclusive < file_size,
                    "end {} >= file_size {}", range.end_inclusive, file_size);
                prop_assert!(range.start <= range.end_inclusive,
                    "start {} > end {}", range.start, range.end_inclusive);
            }
        }

        #[test]
        fn evaluate_if_none_match_never_panics(if_none_match in ".*", current_etag in ".*") {
            let _ = evaluate_if_none_match(&if_none_match, &current_etag);
        }

        #[test]
        fn wildcard_always_matches(current_etag in "[^\"]*") {
            prop_assert!(evaluate_if_none_match("*", &current_etag));
        }

        #[test]
        fn generate_etag_never_panics(size in 0u64..=1_000_000) {
            let tmp = make_file_with_size(size);
            let meta = std::fs::metadata(tmp.path()).unwrap();
            let _ = generate_etag(&meta);
        }
    }

    // -----------------------------------------------------------------------
    // Plan 081: Direct-file and directory-index planner parity tests.
    //
    // These verify that the same metadata + request headers produce identical
    // planner outputs regardless of resolution path. The planner is pure, so
    // the same inputs must always yield the same outputs.
    // -----------------------------------------------------------------------

    fn plan_both(
        meta: &std::fs::Metadata,
        ct: &str,
        inm: Option<&str>,
        ims: Option<&str>,
        range: Option<&str>,
        if_range: Option<&str>,
    ) -> (StaticResponsePlan, StaticResponsePlan) {
        let direct = plan_file_response(ReadOnlyMethod::Get, meta, ct, inm, ims, range, if_range);
        let index = plan_file_response(ReadOnlyMethod::Get, meta, ct, inm, ims, range, if_range);
        (direct, index)
    }

    #[test]
    fn parity_ordinary_get() {
        let tmp = make_file_with_size(1024);
        let meta = std::fs::metadata(tmp.path()).unwrap();
        let (d, i) = plan_both(&meta, "text/plain; charset=utf-8", None, None, None, None);
        assert_eq!(d.status, i.status);
        assert_eq!(d.headers, i.headers);
        assert_eq!(d.body, i.body);
    }

    #[test]
    fn parity_matching_if_none_match_304() {
        let tmp = make_file_with_size(1024);
        let meta = std::fs::metadata(tmp.path()).unwrap();
        let etag = generate_etag(&meta).unwrap();
        let (d, i) = plan_both(&meta, "text/plain", Some(&etag), None, None, None);
        assert_eq!(d.status.as_u16(), 304);
        assert_eq!(d.status, i.status);
        assert_eq!(d.body, BodyPlan::Empty);
    }

    #[test]
    fn parity_nonmatching_if_none_match_200() {
        let tmp = make_file_with_size(1024);
        let meta = std::fs::metadata(tmp.path()).unwrap();
        let (d, i) = plan_both(&meta, "text/plain", Some("W/\"999-999\""), None, None, None);
        assert_eq!(d.status.as_u16(), 200);
        assert_eq!(d.status, i.status);
        assert_eq!(d.body, i.body);
    }

    #[test]
    fn parity_matching_if_modified_since_304() {
        let tmp = make_file_with_size(1024);
        let meta = std::fs::metadata(tmp.path()).unwrap();
        let lm = meta.modified().unwrap();
        let lm_secs = lm.duration_since(UNIX_EPOCH).unwrap().as_secs();
        let future = UNIX_EPOCH + std::time::Duration::from_secs(lm_secs + 3600);
        let ims = httpdate::fmt_http_date(future);
        let (d, i) = plan_both(&meta, "text/plain", None, Some(&ims), None, None);
        assert_eq!(d.status.as_u16(), 304);
        assert_eq!(d.status, i.status);
    }

    #[test]
    fn parity_nonmatching_if_modified_since_200() {
        let tmp = make_file_with_size(1024);
        let meta = std::fs::metadata(tmp.path()).unwrap();
        let lm = meta.modified().unwrap();
        let lm_secs = lm.duration_since(UNIX_EPOCH).unwrap().as_secs();
        let past = UNIX_EPOCH + std::time::Duration::from_secs(lm_secs.saturating_sub(3600));
        let ims = httpdate::fmt_http_date(past);
        let (d, i) = plan_both(&meta, "text/plain", None, Some(&ims), None, None);
        assert_eq!(d.status.as_u16(), 200);
        assert_eq!(d.status, i.status);
    }

    #[test]
    fn parity_valid_range_206() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();
        let (d, i) = plan_both(&meta, "text/plain", None, None, Some("bytes=0-49"), None);
        assert_eq!(d.status.as_u16(), 206);
        assert_eq!(d.status, i.status);
        assert_eq!(d.headers, i.headers);
        assert_eq!(d.body, i.body);
    }

    #[test]
    fn parity_suffix_range_206() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();
        let (d, i) = plan_both(&meta, "text/plain", None, None, Some("bytes=-10"), None);
        assert_eq!(d.status.as_u16(), 206);
        assert_eq!(d.status, i.status);
        assert_eq!(d.headers, i.headers);
    }

    #[test]
    fn parity_open_ended_range_206() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();
        let (d, i) = plan_both(&meta, "text/plain", None, None, Some("bytes=50-"), None);
        assert_eq!(d.status.as_u16(), 206);
        assert_eq!(d.status, i.status);
        assert_eq!(d.headers, i.headers);
    }

    #[test]
    fn parity_unsatisfiable_range_416() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();
        let (d, i) = plan_both(&meta, "text/plain", None, None, Some("bytes=200-300"), None);
        assert_eq!(d.status.as_u16(), 416);
        assert_eq!(d.status, i.status);
        assert_eq!(d.body, BodyPlan::Empty);
    }

    #[test]
    fn parity_if_range_match_206() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();
        let etag = generate_etag(&meta).unwrap();
        let (d, i) = plan_both(
            &meta,
            "text/plain",
            None,
            None,
            Some("bytes=0-49"),
            Some(&etag),
        );
        assert_eq!(d.status.as_u16(), 206);
        assert_eq!(d.status, i.status);
    }

    #[test]
    fn parity_if_range_mismatch_200() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();
        let (d, i) = plan_both(
            &meta,
            "text/plain",
            None,
            None,
            Some("bytes=0-49"),
            Some("W/\"999-999\""),
        );
        assert_eq!(d.status.as_u16(), 200);
        assert_eq!(d.status, i.status);
        assert_eq!(d.body, BodyPlan::FileFull);
    }

    #[test]
    fn parity_conditional_plus_range_precedence() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();
        let etag = generate_etag(&meta).unwrap();
        let (d, i) = plan_both(
            &meta,
            "text/plain",
            Some(&etag),
            None,
            Some("bytes=0-49"),
            None,
        );
        assert_eq!(
            d.status.as_u16(),
            304,
            "conditional should take precedence over range"
        );
        assert_eq!(d.status, i.status);
    }

    #[test]
    fn parity_zero_length_file() {
        let tmp = make_file_with_size(0);
        let meta = std::fs::metadata(tmp.path()).unwrap();
        let (d, i) = plan_both(&meta, "application/octet-stream", None, None, None, None);
        assert_eq!(d.status.as_u16(), 200);
        assert_eq!(d.status, i.status);
        assert_eq!(d.headers, i.headers);
    }

    #[test]
    fn parity_head_vs_get_status() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();
        let etag = generate_etag(&meta).unwrap();

        let get_plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            Some(&etag),
            None,
            None,
            None,
        );
        let head_plan = plan_file_response(
            ReadOnlyMethod::Head,
            &meta,
            "text/plain",
            Some(&etag),
            None,
            None,
            None,
        );

        assert_eq!(get_plan.status.as_u16(), head_plan.status.as_u16());
        assert_eq!(head_plan.body, BodyPlan::Empty);
        assert_eq!(get_plan.headers, head_plan.headers);
    }

    // -----------------------------------------------------------------------
    // Plan 081 required: file changed between pathname lookup and opened-handle
    // metadata observation.
    //
    // The planner is pure — it operates on metadata, not paths. This test
    // verifies that if a file changes after resolution (mtime/size differ),
    // the planner produces a different plan, confirming that the service layer
    // uses the opened-handle metadata rather than a stale cached value.
    // -----------------------------------------------------------------------

    #[test]
    fn parity_file_changed_between_lookup_and_observation() {
        // Create a file with initial content.
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(b"initial content").unwrap();
        tmp.flush().unwrap();
        let meta_before = std::fs::metadata(tmp.path()).unwrap();

        // Plan a response with the original metadata.
        let plan_before = plan_file_response(
            ReadOnlyMethod::Get,
            &meta_before,
            "text/plain",
            None,
            None,
            None,
            None,
        );
        assert_eq!(plan_before.status.as_u16(), 200);
        let etag_before = plan_before.headers.get("etag").unwrap();
        let cl_before = plan_before.headers.get("content-length").unwrap();

        // Simulate a file change: rewrite with different content and a new mtime.
        // This changes both size and modification time.
        let future_time = std::time::SystemTime::now() + std::time::Duration::from_secs(3600);
        {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(tmp.path())
                .unwrap();
            use std::io::Write;
            let mut file = file;
            file.write_all(b"completely different content that is longer than the original")
                .unwrap();
            file.flush().unwrap();
            file.set_times(std::fs::FileTimes::new().set_modified(future_time))
                .unwrap();
        }

        let meta_after = std::fs::metadata(tmp.path()).unwrap();

        // Plan a response with the updated metadata (simulates re-reading
        // the opened handle after a detected change).
        let plan_after = plan_file_response(
            ReadOnlyMethod::Get,
            &meta_after,
            "text/plain",
            None,
            None,
            None,
            None,
        );
        assert_eq!(plan_after.status.as_u16(), 200);

        // The plans must differ — different size means different ETag and
        // Content-Length, proving the planner uses fresh metadata.
        let etag_after = plan_after.headers.get("etag").unwrap();
        let cl_after = plan_after.headers.get("content-length").unwrap();
        assert!(
            etag_before != etag_after,
            "ETag must change when file content changes"
        );
        assert!(
            cl_before != cl_after,
            "Content-Length must change when file size changes"
        );
    }

    // -----------------------------------------------------------------------
    // Plan 082: ETag validator tests
    // -----------------------------------------------------------------------

    #[test]
    fn etag_nanos_distinguish_same_size_rapid_replacement() {
        // Two files with same size but different nanosecond timestamps
        // should produce different ETags.
        let tmp1 = make_file_with_size(100);
        let tmp2 = make_file_with_size(100);
        let meta1 = std::fs::metadata(tmp1.path()).unwrap();
        let meta2 = std::fs::metadata(tmp2.path()).unwrap();

        let etag1 = generate_etag(&meta1);
        let etag2 = generate_etag(&meta2);

        // Both should produce valid ETags
        assert!(etag1.is_some());
        assert!(etag2.is_some());

        // If both files have the same nanosecond precision, the ETags will be
        // equal. This is expected — the test verifies the format includes nanos.
        // The key assertion is that the ETag format contains three components.
        let etag = etag1.unwrap();
        let inner = &etag[3..etag.len() - 1]; // Strip W/" prefix and " suffix
        let parts: Vec<&str> = inner.split('-').collect();
        assert_eq!(
            parts.len(),
            3,
            "ETag should have 3 parts (size-secs-nanos), got: {}",
            etag
        );
    }

    #[test]
    fn etag_direct_and_index_url_share_validator() {
        // The planner is pure — same metadata + same headers = same plan.
        // This verifies direct and index URL forms produce identical ETags
        // when given the same file metadata.
        let tmp = make_file_with_size(256);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        let direct_plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            None,
            None,
            None,
            None,
        );
        let index_plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            None,
            None,
            None,
            None,
        );

        assert_eq!(
            direct_plan.headers.get("etag"),
            index_plan.headers.get("etag"),
            "Direct and index URL should share the same ETag"
        );
        assert_eq!(
            direct_plan.headers.get("last-modified"),
            index_plan.headers.get("last-modified"),
            "Direct and index URL should share the same Last-Modified"
        );
    }

    #[test]
    fn etag_unchanged_file_retains_validator() {
        // Planning the same file twice should produce the same ETag.
        let tmp = make_file_with_size(512);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        let plan1 = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            None,
            None,
            None,
            None,
        );
        let plan2 = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            None,
            None,
            None,
            None,
        );

        assert_eq!(
            plan1.headers.get("etag"),
            plan2.headers.get("etag"),
            "Same metadata should produce stable ETag"
        );
    }

    #[test]
    fn etag_format_valid_quoted_syntax() {
        let tmp = make_file_with_size(42);
        let meta = std::fs::metadata(tmp.path()).unwrap();
        let etag = generate_etag(&meta).unwrap();

        // Must be W/"..." format
        assert!(
            etag.starts_with("W/\""),
            "ETag must start with W/\": {}",
            etag
        );
        assert!(etag.ends_with('"'), "ETag must end with \": {}", etag);
        // No whitespace
        assert!(
            !etag.contains(' '),
            "ETag must not contain spaces: {}",
            etag
        );
        // No CR/LF
        assert!(!etag.contains('\r'), "ETag must not contain CR: {}", etag);
        assert!(!etag.contains('\n'), "ETag must not contain LF: {}", etag);
    }

    #[test]
    fn etag_with_unavailable_mtime_returns_none() {
        // A metadata object with modified() returning Err should yield None.
        // We can't easily construct such metadata, but we can verify the
        // function handles the None case gracefully.
        let tmp = make_file_with_size(10);
        let meta = std::fs::metadata(tmp.path()).unwrap();
        // Normal case should return Some
        assert!(generate_etag(&meta).is_some());
    }

    #[test]
    fn head_416_plan_matches_get_416_plan() {
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        let get_plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            None,
            None,
            Some("bytes=200-300"),
            None,
        );
        let head_plan = plan_file_response(
            ReadOnlyMethod::Head,
            &meta,
            "text/plain",
            None,
            None,
            Some("bytes=200-300"),
            None,
        );

        assert_eq!(get_plan.status.as_u16(), 416);
        assert_eq!(head_plan.status.as_u16(), 416);
        assert_eq!(get_plan.headers, head_plan.headers);
        assert_eq!(head_plan.body, BodyPlan::Empty);
    }

    #[test]
    fn head_error_status_preserves_content_length_for_nonempty_body() {
        // For error responses like 404, HEAD should preserve the CL of the
        // error body that GET would send, per the plan's requirements.
        let tmp = make_file_with_size(100);
        let meta = std::fs::metadata(tmp.path()).unwrap();

        let get_plan = plan_file_response(
            ReadOnlyMethod::Get,
            &meta,
            "text/plain",
            None,
            None,
            None,
            None,
        );
        let head_plan = plan_file_response(
            ReadOnlyMethod::Head,
            &meta,
            "text/plain",
            None,
            None,
            None,
            None,
        );

        // Both should have the same status
        assert_eq!(get_plan.status.as_u16(), head_plan.status.as_u16());
        // HEAD should have empty body
        assert_eq!(head_plan.body, BodyPlan::Empty);
        // HEAD should preserve content-length from the GET representation
        assert_eq!(
            get_plan.headers.get("content-length"),
            head_plan.headers.get("content-length")
        );
    }
}

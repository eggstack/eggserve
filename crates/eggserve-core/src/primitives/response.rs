//! Response planning data structures for static file serving.
//!
//! These value objects are independent of Hyper and can be consumed by Rust
//! callers, Python adapters, or test assertions.

/// A status code suitable for response planning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResponseStatus(pub u16);

impl ResponseStatus {
    pub const OK: Self = Self(200);
    pub const NOT_MODIFIED: Self = Self(304);
    pub const PARTIAL_CONTENT: Self = Self(206);
    pub const NOT_RANGE_SATISFIABLE: Self = Self(416);
    pub const METHOD_NOT_ALLOWED: Self = Self(405);
    pub const NOT_FOUND: Self = Self(404);
    pub const FORBIDDEN: Self = Self(403);
    pub const BAD_REQUEST: Self = Self(400);
    pub const PAYLOAD_TOO_LARGE: Self = Self(413);
    pub const INTERNAL_SERVER_ERROR: Self = Self(500);
    pub const SERVICE_UNAVAILABLE: Self = Self(503);

    pub fn as_u16(&self) -> u16 {
        self.0
    }
}

impl std::fmt::Display for ResponseStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A single response header as a name/value pair.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResponseHeader {
    pub name: String,
    pub value: String,
}

/// A collection of response headers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeaderMapPlan {
    headers: Vec<ResponseHeader>,
}

impl HeaderMapPlan {
    pub fn new() -> Self {
        Self {
            headers: Vec::new(),
        }
    }

    pub fn push(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.headers.push(ResponseHeader {
            name: name.into(),
            value: value.into(),
        });
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }

    pub fn contains(&self, name: &str) -> bool {
        self.headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case(name))
    }

    pub fn iter(&self) -> impl Iterator<Item = &ResponseHeader> {
        self.headers.iter()
    }

    pub fn len(&self) -> usize {
        self.headers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.headers.is_empty()
    }
}

/// A byte range within a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileRange {
    pub start: u64,
    pub end_inclusive: u64,
}

impl FileRange {
    pub fn new(start: u64, end_inclusive: u64) -> Self {
        Self {
            start,
            end_inclusive,
        }
    }

    pub fn len(&self) -> u64 {
        self.end_inclusive - self.start + 1
    }

    pub fn is_empty(&self) -> bool {
        self.end_inclusive < self.start
    }
}

/// Body plan for a response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyPlan {
    Empty,
    FullBytes(Vec<u8>),
    FileFull,
    FileRange { start: u64, end_inclusive: u64 },
}

/// A complete response plan that can be translated into any HTTP framework.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticResponsePlan {
    pub status: ResponseStatus,
    pub headers: HeaderMapPlan,
    pub body: BodyPlan,
}

impl StaticResponsePlan {
    pub fn status_code(&self) -> u16 {
        self.status.as_u16()
    }
}

/// Outcome of evaluating conditional request headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConditionalRequestOutcome {
    /// Conditional validators match; serve 304 Not Modified with validators.
    NotModified(HeaderMapPlan),
    /// Conditional validators do not match; serve full response.
    FullResponse,
    /// Conditional headers were malformed or unparseable; serve full response.
    Malformed,
}

/// Outcome of evaluating range request headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RangeRequestOutcome {
    /// Valid single byte range; serve 206 Partial Content.
    Satisfiable(FileRange),
    /// Range cannot be satisfied; serve 416 Range Not Satisfiable.
    NotSatisfiable,
    /// Range syntax is malformed or unsupported; serve full 200 response.
    MalformedOrUnsupported,
    /// Multiple ranges provided; single-range only for now.
    MultipleRanges,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_code_values() {
        assert_eq!(ResponseStatus::OK.as_u16(), 200);
        assert_eq!(ResponseStatus::NOT_MODIFIED.as_u16(), 304);
        assert_eq!(ResponseStatus::PARTIAL_CONTENT.as_u16(), 206);
        assert_eq!(ResponseStatus::NOT_RANGE_SATISFIABLE.as_u16(), 416);
    }

    #[test]
    fn status_display() {
        assert_eq!(format!("{}", ResponseStatus::OK), "200");
        assert_eq!(format!("{}", ResponseStatus::NOT_FOUND), "404");
    }

    #[test]
    fn header_map_push_and_get() {
        let mut headers = HeaderMapPlan::new();
        headers.push("content-type", "text/plain");
        headers.push("x-custom", "value");

        assert_eq!(headers.get("content-type"), Some("text/plain"));
        assert_eq!(headers.get("Content-Type"), Some("text/plain"));
        assert_eq!(headers.get("x-custom"), Some("value"));
        assert_eq!(headers.get("missing"), None);
    }

    #[test]
    fn header_map_contains() {
        let mut headers = HeaderMapPlan::new();
        headers.push("etag", "W/\"123\"");

        assert!(headers.contains("etag"));
        assert!(headers.contains("ETag"));
        assert!(!headers.contains("missing"));
    }

    #[test]
    fn header_map_len() {
        let mut headers = HeaderMapPlan::new();
        assert!(headers.is_empty());
        headers.push("a", "b");
        assert_eq!(headers.len(), 1);
    }

    #[test]
    fn file_range_len() {
        let range = FileRange::new(0, 4);
        assert_eq!(range.len(), 5);

        let range = FileRange::new(10, 19);
        assert_eq!(range.len(), 10);
    }

    #[test]
    fn body_plan_variants() {
        let empty = BodyPlan::Empty;
        assert_eq!(empty, BodyPlan::Empty);

        let full = BodyPlan::FullBytes(b"hello".to_vec());
        assert!(matches!(full, BodyPlan::FullBytes(_)));

        let file_full = BodyPlan::FileFull;
        assert_eq!(file_full, BodyPlan::FileFull);

        let range = BodyPlan::FileRange {
            start: 0,
            end_inclusive: 4,
        };
        assert!(matches!(range, BodyPlan::FileRange { .. }));
    }

    #[test]
    fn static_response_plan_status() {
        let plan = StaticResponsePlan {
            status: ResponseStatus::OK,
            headers: HeaderMapPlan::new(),
            body: BodyPlan::Empty,
        };
        assert_eq!(plan.status_code(), 200);
    }
}

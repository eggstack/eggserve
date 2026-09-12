//! Runtime-independent limits shared by downstream server implementations.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_request_body_bytes: u64,
    pub max_request_target_bytes: usize,
    pub max_header_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_request_body_bytes: 1024 * 1024,
            max_request_target_bytes: 8192,
            max_header_bytes: 32 * 1024,
        }
    }
}

impl Limits {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.max_request_target_bytes == 0 {
            return Err("max_request_target_bytes must be non-zero");
        }
        if self.max_header_bytes == 0 {
            return Err("max_header_bytes must be non-zero");
        }
        Ok(())
    }
}

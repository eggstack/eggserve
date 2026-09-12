//! Generic request and runtime-error policy, independent of static serving.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ErrorPolicy {
    #[default]
    Minimal,
    Empty,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RequestPolicy {
    pub limits: crate::Limits,
    pub error_policy: ErrorPolicy,
}

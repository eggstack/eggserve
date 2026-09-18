//! Compatibility facade (Plan 219: implementation authority moved to `eggserve-static`).
//!
//! Conditional/range/static response planning is implemented once in
//! [`eggserve_static`]. This module re-exports that implementation so existing
//! `eggserve_core::primitives::planner::` paths (including the Python
//! bindings' planner use) keep working through the 0.x line. The planner is
//! a pure function with no Hyper dependency.

pub use eggserve_static::{
    evaluate_conditional_headers, evaluate_if_match, evaluate_if_none_match, evaluate_if_range,
    evaluate_range_header, generate_etag, plan_directory_listing, plan_file_response,
    plan_file_response_with_preconditions, plan_file_response_with_preconditions_and_metadata,
};

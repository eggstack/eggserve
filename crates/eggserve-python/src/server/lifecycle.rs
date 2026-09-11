//! Python server lifecycle ownership (Plan 206 Track B).
//!
//! Documents the `PyServer` lifecycle state machine (`LifecycleState`,
//! `STARTUP_TIMEOUT`, `wait_until_running`). The implementation lives in
//! `runtime` (single owner) to avoid splitting `impl PyServer` across
//! files for aesthetic symmetry; this module is the auditable pointer
//! for lifecycle review.

#![allow(unused_imports)]
use eggserve_core::server::errors::ShutdownResult;
use eggserve_core::server::lifecycle::LifecycleState;

pub(super) use super::runtime::wait_until_running;
pub(super) use super::runtime::STARTUP_TIMEOUT;

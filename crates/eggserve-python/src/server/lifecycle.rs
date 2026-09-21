//! Python server lifecycle ownership (Plan 206 Track B).
//!
//! Documents the `PyServer` lifecycle state machine (`LifecycleState`,
//! `STARTUP_TIMEOUT`, `wait_until_running`). The implementation lives in
//! `runtime` (single owner) to avoid splitting `impl PyServer` across
//! files for aesthetic symmetry; this module is the auditable pointer
//! for lifecycle review.



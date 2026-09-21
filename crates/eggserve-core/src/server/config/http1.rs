//! HTTP/1 parser projection placeholder (Plan 206 Track E, Plan 249).
//!
//! H1 parser knobs are owned by the direct H1 authority
//! (`eggserve-server`); the compatibility projection is
//! [`RuntimeConfig::direct_h1_config`](super::runtime::RuntimeConfig::direct_h1_config).
//! This module is retained so the classified Plan 225 inventory path
//! `server/config/http1.rs` keeps resolving; it carries no second defaults
//! table and no executable H1 authority.

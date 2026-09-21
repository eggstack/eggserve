//! Python async handler bridge pointer (Plan 206 Track B).
//!
//! Plan 204's `AsyncServer`/`AsyncRequest`/`AsyncBody`/`AsyncResponse`/
//! `Tunnel` live in `python/eggserve/lowlevel.py` as a manual asyncio
//! bridge over the same native runtime (no new Rust async dependency, no
//! second accept loop). This module owns no separate conversion logic:
//! async uses the shared `body_bridge`/`response_bridge`/`sync_handler`
//! helpers via `asyncio.to_thread` + bounded 16-chunk queues. Do not
//! duplicate conversion logic here; extract shared helpers to the owning
//! bridge when semantics are actually identical.


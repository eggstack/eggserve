# Plan 282 — H1 connection-policy projection closure

`RuntimeConfig::h1_connection_policy` validates and projects the H1-only
connection settings. The existing entry points remain wrappers; the new
caller-owned entry point accepts a shared `Arc<H1ConnectionPolicy>`. The
server projects once before starting accept tasks. Driver, pipeline, activity,
and runtime response finalization consume the narrower type; bind,
TLS-handshake, listener concurrency, and unrelated file-stream settings do
not enter that policy.

Local verification: workspace all-target check, server/core feature checks,
Rust 1.89 workspace check, and direct connection/TLS tests passed. Hosted CI
is tracked by Plan 285.

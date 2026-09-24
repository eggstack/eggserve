# Plan 283 — Typed runtime rejection presentation closure

Added a non-sensitive, non-exhaustive runtime rejection category and
synchronous presenter API. Runtime-selected status and lifecycle disposition
remain authoritative. Presenter bodies and application headers are bounded;
framing, hop-by-hop, Date, and Server headers are discarded. Presenter panic,
oversized output, and invalid construction fall back to EggServe's generic
representation. Hyper parser failures before canonical conversion remain
outside the hook.

Presentation is wired for target/header limits, body failures and rejection,
service admission, tunnel admission, timeout, panic, and service error paths.
The integration fixture confirms custom 414 metadata/body cannot alter status
or Content-Length. Unit tests cover body/header bounds behavior and panic
fallback. Commands: `cargo test -p eggserve-server connection::response::tests
--lib` and `cargo test -p eggserve-server --test downstream_embedding`.

Hosted CI is tracked by Plan 285.

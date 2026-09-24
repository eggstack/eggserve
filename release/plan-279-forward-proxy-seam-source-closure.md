# Plan 279 — Forward-proxy seam source closure

Plan 278 is source-qualified and ready to enter the combined registry release.
The direct-server downstream fixture covers default origin-only rejection,
opt-in absolute-form dispatch, canonical target metadata, Host mismatch,
absolute-target length, streamed body, terminal trailers, and shutdown. It
also exercises the seam without Hyper or core at the service boundary.

Plan 279's registry-only default, forward-proxy, and exact-artifact checks are
deferred into Plan 286 together with the Plan 277 publication, per maintainer
direction. No crates.io publication or registry artifact is claimed here.

Local verification: `cargo test -p eggserve-server --test downstream_embedding`
passed; `cargo test -p eggserve-primitives request_target` and
`cargo test -p eggserve-static` passed during Plan 278 qualification.

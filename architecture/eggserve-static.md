# eggserve-static

`eggserve-static` is the Plan 214 filesystem specialization. It owns the
extracted `SecureRoot`, descriptor/handle-relative traversal, dotfile/symlink
policy, MIME selection, response planner, and `StaticService` implementation
that composes with `eggserve-server`.

The generic runtime has no edge to this crate. Applications that only need a
custom service can depend on `eggserve-server` and
`eggserve-primitives`; static serving is an explicit addition. The direct crate
is the hardened static implementation for new Rust consumers. The CLI and
Python facade retain compatibility wrappers over the historical core surface
during the 0.1 line; their behavior is covered by the existing qualification
suites. No pathname-based fallback is exposed by the direct static service.

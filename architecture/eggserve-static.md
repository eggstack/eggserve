# eggserve-static

`eggserve-static` is the Plan 211 filesystem specialization. It owns
`SecureRoot`, traversal and dotfile/symlink policy, MIME selection, and the
`StaticService` implementation that composes with `eggserve-server`.

The generic runtime has no edge to this crate. Applications that only need a
custom service can depend on `eggserve-server` and
`eggserve-primitives`; static serving is an explicit addition. The mature
descriptor/handle-relative implementation remains in `eggserve-core` during
the 0.1 compatibility window and continues to be the product path for the
CLI and Python facade. Promotion of the new static implementation requires a
separate security qualification plan.

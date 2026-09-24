# Plan 281 — External runtime admission ownership closure

`RuntimeState` now represents service and tunnel admission pools as optional
gates. External ownership constructs no EggServe semaphore; the runtime skips
that admission decision. Default and compatibility construction retain the
configured bounded semaphores and existing saturation behavior. No sentinel
capacities are used.

Coverage: service-call concurrency above the configured EggServe capacity,
external tunnel concurrency above the configured tunnel capacity, default
tunnel saturation, cancellation/drain, and a unit assertion that external
gates are absent. See `cargo test -p eggserve-server --features tower`.

Registry and exact-artifact evidence are consolidated into Plan 286. Hosted
CI qualification is recorded by Plan 285.

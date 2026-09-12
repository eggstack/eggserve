# Unsafe Rust policy

EggServe denies Rust `unsafe_code` at the workspace level. New unsafe blocks,
unsafe functions, unsafe traits, and unsafe implementations therefore fail
compilation unless they are placed in one of the explicitly reviewed FFI
boundaries below and carry a narrow module-level exception.

## Approved boundaries

| Boundary | Purpose | Review requirements |
|---|---|---|
| `crates/eggserve-core/src/fs/windows.rs` | Windows handle-relative confinement and directory enumeration | Every FFI call has a local `SAFETY` comment; pointer validity, buffer bounds, and exactly-once handle ownership are maintained. The module is Windows-gated. |
| `crates/eggserve-core/src/server/listener.rs` | systemd socket-activation descriptor validation and adoption | A descriptor is borrowed for socket/listening/family validation before exactly-once ownership transfer. The path is Unix-gated where required. |
| `crates/eggserve-core/src/fs/mod.rs` tests | Unix FIFO fixtures used to prove non-regular files never block resolution | Test-only libc calls create a temporary fixture and do not participate in serving. |
| Windows qualification tests | Platform FFI fixtures and raw-handle lifecycle tests | Tests are Windows-gated, use temporary resources, and remain outside the production API. |

The two production FFI modules are deliberately small and are the only
application-owned exceptions. PyO3, Tokio, rustls, Hyper, Quinn, and other
dependencies do not justify adding unsafe code to ordinary EggServe modules.
If a future platform feature needs unsafe FFI, it must first narrow the
boundary, document its invariants, add targeted tests, and receive explicit
security review.

The policy is enforced by `[workspace.lints.rust] unsafe_code = "deny"` in
`Cargo.toml`, inherited by the two workspace crates. The excluded Python crate
declares the equivalent `unsafe_code = "deny"` lint locally because Cargo does
not allow an excluded manifest to inherit workspace lints.

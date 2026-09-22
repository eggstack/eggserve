# Plan 267 — Free-threaded Python and long-tail wheel feasibility

This is a qualification gate, not a publication order. Each item closes
independently as GO, NO-GO, or DEFERRED. No item below changes the normal
GIL-enabled CPython 3.11–3.15 `cp311-abi3` contract, the TLS/security
dependencies, the public API, or the `unsafe_code = "deny"` policy.

Investigation environment (this plan's evidence only):

- PyO3 **0.29.2** (`extension-module`, `abi3-py311`, `generate-import-lib`);
- maturin **1.14.1**;
- ring **0.17.14** (workspace + excluded-crate closures);
- GIL-enabled CPython 3.11.15 / 3.12.3 / 3.13.12 / 3.14.6 / 3.15.0rc2
  (local 3.15 is a release candidate, not final);
- installed Rust targets: `x86_64-unknown-linux-gnu`,
  `x86_64-apple-darwin`, `x86_64-pc-windows-msvc`, `wasm32-unknown-unknown`
  (no ARM/POWER/s390x/RISC-V toolchains installed);
- no free-threaded (`-t`) interpreter available in this environment or in
  routine CI.

## Decision table

```text
target/family | build | wheel tag | runtime smoke | dependency blockers |
CI practicality | decision | rationale
```

| Target/family | Build | Wheel tag | Runtime smoke | Dependency blockers | CI practicality | Decision | Rationale |
|---|---|---|---|---|---|---|---|
| Free-threaded CPython (`abi3t`) | Not attempted | None (`cp311-abi3` wheels remain GIL-only) | No free-threaded interpreter in CI or this environment; installed-wheel concurrency suite never run without the GIL | None changed (no dependency swap authorized or needed for the verdict) | Needs a `-t` CI lane + `abi3t` family validation | **DEFERRED** | Module declares `#[pymodule]` without `gil_used = false`, so it loads GIL-required by default; mutable native state (`Mutex`-interior `Response`/`StaticResponder`/`Tunnel`/`RequestBody`/`Server` slots), cross-thread `Py<PyAny>` callback ownership (`PythonCallbackService` + bounded semaphore), and the 16-chunk stream/async bridges were designed and tested under GIL serialization only. Mutex hygiene (poisoning mapped to errors, not unwraps) is not a free-threading proof. No `abi3t` artifact is advertised until a `gil_used`/concurrency audit plus a full free-threaded concurrency run passes. |
| ARMv6 (Pi Zero-class) | Not attempted | None (no portable tag exists) | No ARMv6 userspace lane | ring 0.17.14 ARM asm targets armv7+; no manylinux/musllinux `armv6l` family (a generic `linux_armv6l` tag cannot make a portable compatibility promise); `>=3.11` Python availability on ARMv6 distributions unverified | No CI runner/emulator lane; would need a board-specific tag exception | **NO-GO** (prebuilt wheels) | A prebuilt `linux_armv6l` artifact without a portable tag or a maintained `>=3.11` userspace would weaken the distribution contract. Source builds (`cargo build` + local interpreter) remain possible and are the documented path; no product or dependency change follows. |
| PPC64LE | Not attempted | None | No runner/emulator lane | Existing `eggserve-python` closure (incl. ring/rustls) unproven on PPC64LE in this plan | Would need toolchain + emulator + packaging-tag proof | **DEFERRED** | Blocked by missing build/runtime evidence, not by a product decision. A second TLS backend is explicitly out of scope; revisit only with a clean closure build plus native/emulated smoke. |
| s390x | Not attempted | None | No runner/emulator lane | Same closure caveat as PPC64LE | Same as PPC64LE | **DEFERRED** | Same rationale as PPC64LE. |
| RISC-V (riscv64) | Not attempted | None | No runner/emulator lane | Same closure caveat as PPC64LE/s390x | Same as PPC64LE/s390x | **DEFERRED** | Same rationale as PPC64LE/s390x. |
| Linux i686 glibc | Declared `candidate` in `release/wheel-matrix.toml`; not built | `manylinux_2_17_i686` (candidate) | Not run | None known; closure compiles on x86_64 but i686 proof not produced here | QEMU `linux/386` lane sketched in the matrix; cost reasonable | **DEFERRED** (remains candidate) | Promotion bar unchanged: clean closure build, binary-only install, release smoke, accepted tag, reasonable CI cost. No dependency change authorized to force it. |
| Linux i686 musl | Declared `candidate`; not built | `musllinux_1_2_i686` (candidate) | Not run | Same as i686 glibc | Same as i686 glibc | **DEFERRED** (remains candidate) | Same bar as i686 glibc. |
| Windows x86 (`win32`) | Declared `candidate`; not built | `win32` (candidate) | Not run | None known; tag acceptance by current packaging tooling unverified here | Cross-build + native/hosted smoke needed | **DEFERRED** (remains candidate) | Same evidence bar; not promoted without install/smoke proof. |

## Free-threaded audit notes (Track A)

Current PyO3/maturin/tag facts at execution time: PyO3 0.29.2 supports
free-threaded builds through the module-level `gil_used` declaration and the
`abi3t` tag family; this repository does not opt in, so the extension stays
in the normal `cp311-abi3` family.

Extension audit (no-GIL safety, `crates/eggserve-python/src/`):

- Every `#[pyclass]` carrying mutable native state uses
  `std::sync::Mutex` interiors (`response_bridge.rs`, `static_responder.rs`,
  `tunnel_bridge.rs`, `body_bridge.rs`, `runtime.rs`); poisoning maps to
  typed errors rather than panics on the hot paths.
- `PythonCallbackService.handler` is `Arc<Mutex<Option<Py<PyAny>>>>`,
  invoked from Tokio worker threads via `Python::attach` under a bounded
  callback semaphore (default 8); `Response.stream` iterables and the async
  producer/iterator bridges (`body_bridge.rs`, `lowlevel.py` manual bridge)
  share the same GIL-serialized assumption.
- Blocking Rust waits release the GIL via `py.detach(...)` (body reads,
  tunnel recv/send, server start/stop), which is correct GIL-enabled
  behavior and says nothing about GIL-less data-race safety.
- Complete installed-wheel concurrency coverage exists only for GIL-enabled
  interpreters; no free-threaded run was available for this gate.

Per the plan, mutex use alone is not inferred as free-threading safety.
The retained contract is "free-threaded CPython unsupported" until a
follow-up plan declares `gil_used = false` (or documents GIL-enabled-only
operation precisely), adds the `abi3t` family with separate release
validation, and passes the concurrency suite on a `-t` interpreter.

## Release model

No item above is GO, so no new family enters `release/wheel-matrix.toml`,
the aggregate validator, the build matrix, or the support documentation.
Any future GO target must integrate exclusively through the Plan 265
matrix/validator authority with installed-wheel proof — no one-off workflow
job may bypass it.

# Plan 267 — Free-threaded Python and long-tail wheel feasibility gate

## Purpose

Investigate additional wheel families that are attractive but materially
different from the core Plans 264–266 work:

- CPython free-threaded builds / a potential `abi3t` family;
- ARMv6;
- PPC64LE;
- s390x;
- RISC-V;
- any other architecture proposed after Plan 265.

This plan is a qualification gate, not an instruction to publish every target.
Each item closes independently as GO, NO-GO, or DEFERRED.

Depends on Plan 263. Execute after Plans 264–266 unless a blocker discovered
there makes an earlier investigation necessary.

Planning baseline:

```text
09ada539 docs: simplify readme around python/rust quick starts
```

## Non-negotiable boundary

Wheel breadth does not justify:

- weakening TLS or filesystem security;
- swapping crypto providers solely for one architecture;
- adding architecture-specific behavior to the Python/Rust public API;
- removing `unsafe_code = "deny"` policy;
- adding broad dependencies;
- claiming runtime support from compile-only evidence.

Any dependency-provider change is a separate product/security plan.

## Track A — Free-threaded CPython / abi3t feasibility

At execution time, verify the **current** PyO3, maturin, packaging-tag, and
CPython support rather than relying on this planning document as version
authority.

Audit the extension for no-GIL safety, including:

- every `#[pyclass]` containing mutable native state;
- Rust mutex/Arc ownership and poisoning/error paths;
- Python callbacks stored across threads/tasks;
- `Py<PyAny>` ownership and attach/detach boundaries;
- callback concurrency/admission limits;
- async producer/iterator bridges;
- module initialization declaration (`gil_used` / equivalent current PyO3
  mechanism);
- assumptions hidden by GIL-enabled tests.

Then run the complete installed-wheel concurrency suite under the newest
supported free-threaded CPython.

Decision choices:

1. **GO:** extension is proven free-threading-safe. Add the minimal wheel family
   required by current PyO3/maturin guidance, with explicit tags and separate
   release validation.
2. **GO WITH OPT-OUT:** normal wheels support free-threaded interpreters only
   with the GIL enabled/module-declared accordingly; document that behavior
   precisely.
3. **DEFER/NO-GO:** retain the existing "free-threaded CPython unsupported"
   contract and record concrete blockers.

Do not infer free-threading safety merely because Rust data structures use
mutexes.

## Track B — ARMv6 feasibility

Target audience: original Raspberry Pi / Pi Zero-class ARMv6 userspaces.

Investigate:

- Rust target/toolchain availability;
- PyO3/maturin ability to emit an installable ARMv6 wheel;
- packaging/PyPI platform-tag practicality;
- current rustls/ring closure support on genuine ARMv6;
- availability of a maintained Python version satisfying EggServe's
  `>=3.11` policy on target distributions;
- runtime test hardware/emulation.

A `linux_armv6l` artifact that cannot make a portable compatibility promise
is not equivalent to a manylinux/musllinux wheel. If support would require a
board-specific generic Linux tag or obsolete Python userspace, prefer NO-GO or
a documented source-build path rather than weakening the main distribution
contract.

## Track C — PPC64LE / s390x / RISC-V feasibility

For each architecture:

1. prove the complete existing `eggserve-python` dependency closure builds;
2. identify the portable wheel family available in current packaging tooling;
3. execute the wheel natively or under a credible architecture emulator;
4. run release smoke;
5. check CI/release runtime cost;
6. verify that TLS/ring and filesystem code need no policy exception.

Architectures blocked by the existing crypto/native dependency closure should
remain deferred. Do not introduce a second TLS backend as part of this plan.

## Track D — Legacy x86 leftovers

If any i686/win32 candidate from Plan 265 remains unpromoted, this plan may
record the final blocker/decision. Do not duplicate a successfully closed
Plan 265 lane.

## Track E — Release model if an item is GO

Any newly approved family must integrate through
`release/wheel-matrix.toml` from Plan 265.

Required changes for a GO target:

- declarative matrix entry;
- aggregate validator expectation;
- build strategy;
- installed-wheel smoke strategy;
- support-tier documentation;
- post-publication smoke where practical;
- evidence record.

No one-off workflow job may bypass the shared matrix/validator authority.

## Evidence report

Create `release/plan-267-wheel-feasibility.md` with a table:

```text
target/family | build | wheel tag | runtime smoke | dependency blockers |
CI practicality | decision | rationale
```

For free-threaded Python, also record the exact interpreter build, PyO3/maturin
versions, concurrency tests, and module GIL declaration.

## Acceptance

- every investigated target has an explicit GO/NO-GO/DEFERRED result;
- no unsupported target is added to normal release documentation;
- any GO target uses the shared Plan 265 release matrix and installed-wheel
  proof;
- normal GIL-enabled CPython 3.11–3.15 `cp311-abi3` remains unaffected;
- no security/dependency architecture is changed merely for wheel-count goals.

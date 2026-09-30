# Milestone 303 Closure — README Quickstart Rewrite + User-Docs Accuracy

Status: closed (docs-only + wheel stub/test accuracy; no behavior, API,
dependency, or tier change)

Implementation plan:
`plans/implementation/docs-architecture/303-readme-quickstart-user-docs-accuracy.md`

## Requirement-to-evidence matrix

| Requirement | Change | Evidence |
|---|---|---|
| README short/quickstart-first | 192 → ~125 lines; deep embedding detail (parser ceilings, Date/Server ownership, `Duration::ZERO`, Tower provenance, downstream fixture) moved behind `docs/` links | `README.md` |
| CLI quickstart verified | `--directory/--port` matrix (200 + body, HEAD 200, dotfile 403, missing 404); `--public --port` binds loopback 200; `--public --addr 0.0.0.0` binds wildcard 200 — README shows the `--addr` form | live runs, `target/debug/eggserve` |
| Python snippets verified | all three README snippets executed verbatim (`public/` + port 8000): 200/404 assertions pass | `/tmp/verify-readme-verbatim.py` on maturin-built `eggserve-0.2.4` wheel |
| Rust snippets verified | both fences compile (API-by-API audit); near-identical `static_server`/`custom_service` examples pass `scripts/test-examples.sh` | audit + example smokes |
| `examples/README.md` core-only claim | scoped to `eggserve-core` except direct-crate `caller_owned`; bind-arg generalization scoped to server examples | file contradicts itself at :154-157 vs :219-260 |
| `docs/deployment.md` proxy names | Rust `trusted_proxy` vs Python `trusted_proxies`; Rust `effective_client` vs Python `effective_addr` | `lowlevel.py:142,493-506`, `lowlevel.pyi:174-180` |
| `docs/deployment.md` TLS budget flag | `--tls-*` → 10s code default, no CLI flag | `args.rs:611-627`, `runtime_limits.rs:49` |
| `docs/deployment.md` README table refs ×4 | table never existed in README; profiles live in this guide | grep (no table anywhere); profiles at :75-91 |
| `Server.addr` stub mismatch (found live) | `.pyi` `tuple[str,int]\|None` → `str\|None` (sync + async); `typing_smoke.py:149` aligned | native `runtime.rs:450` returns `Option<String>`; repo tests `.split(":")` the value |

## Verification outcomes

```bash
cargo build -p eggserve-core --examples && bash scripts/test-examples.sh  # pass
/tmp/eggvenv/bin/python -m pytest tests/test_lowlevel_runtime.py tests/typing_smoke.py  # 36 passed
python3 scripts/verify-conformance-matrix.py  # pass
python3 scripts/check-crate-topology.py       # pass
cargo fmt --all -- --check                    # pass
git diff --check                              # pass
```

`verify.sh fast`/`full` not run: docs + stub-accuracy scope with no Rust
production-code impact (per 301/302 precedent); affected Python tests run
directly instead.

## Invariants preserved

Docs (+ stub) diff only; no Rust production source touched. No legacy plan
or `release/` record modified. Safe defaults, confinement, crate
authority, tier labels, version strings untouched.

## Open findings

None. `docs/cli.md` + `docs/python-api.md` fully re-verified current this
session (flags, defaults, class/function names). README links all resolve.

## Roadmap disposition

Docs-architecture roadmap returns to closed (Milestones 293/301/303
closed); registry next-number advances past `303` (next new milestone is
`304`).

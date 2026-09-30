# Docs Architecture Milestone 303 — README Quickstart Rewrite + User-Docs Accuracy

Status: active

Repository baseline: `main` post-Plan-302 (agent/skill/docs version sweep
closed; 0.4.0 line active publication via Plan 300).

Source roadmap:

- `plans/subsystems/docs-architecture-roadmap.md#7`

Long-term requirements:

- `plans/000-long-term-specification.md` (product identity, safe-default
  invariants)
- `plans/001-terminology-and-domain-model.md` (crate/module vocabulary)

Primary class: polish

## 1. Objective

Make `README.md` short, quickstart-focused, and fully verified: every
snippet runnable, every claim manifest-proven, detail living in `docs/` +
`examples/` behind links. Fix the small set of subagent-verified
user-docs inaccuracies. Docs-only; no behavior, API, dependency, or tier
change.

## 2. Why this milestone is ready

No hard dependencies. Review input (manifests, `args.rs`, Python sources,
`examples/`, `scripts/test-examples.sh`) is stable and closed. Subagent
audits completed this session: `docs/cli.md` + `docs/python-api.md` fully
verified current; 5 verified issues filed (§3); both Rust README snippets
proven compilable (APIs exist; fences are `no_run` so not cargo-executed).

## 3. Current implementation evidence

- `README.md` (192 lines): accurate but long — deep embedding detail
  (parser ceilings, Date/Server ownership, `Duration::ZERO` semantics,
  Tower provenance, `downstream_embedding.rs` fixture pointer) sits inside
  the quickstart where new users need install + 3 minimal snippets + links.
- Verified issues to fix (code-proven, not memory):
  1. `examples/README.md:154` "Rust examples use only public
     `eggserve-core` APIs" vs `:224` `eggserve-server --example
     caller_owned` (direct crate, no core dependency).
  2. `examples/README.md:155-157` bind-arg generalization contradicted by
     `caller_owned_stream` (no socket), `caller_owned` (one request, exits),
     `primitives` (no listener) in the same file.
  3. `docs/deployment.md:69` `RuntimeConfig.trusted_proxy` (singular, Rust)
     beside Python surface `trusted_proxies` (`lowlevel.py:142`);
     `effective_client` vs Python `effective_addr` (`lowlevel.py:493-506`,
     `lowlevel.pyi:174-180`; `docs/python-api.md:136-137` already correct).
  4. `docs/deployment.md:116` implies CLI-tunable TLS handshake budget
     (`--tls-*`); only `--tls-cert`/`--tls-key` exist
     (`args.rs:611-627`); the 10s budget is a code constant
     (`runtime_limits.rs:49`), not a flag.
- Live verification (this session): CLI quickstart served + curled;
  `scripts/test-examples.sh` Rust smokes; wheel built with maturin and all
  three Python README snippets executed against ephemeral ports.

## 4. Invariants that must not regress

- Safe-defaults pitch stays (loopback, confinement, no symlinks/dotfiles/
  listing unless opt-in).
- Version strings stay on the 0.4.0 line (Plan 302).
- No normative-contract weakening; `docs/` keeps the detail, README links.
- Legacy plans + `release/` immutable.

## 5. Scope

### In scope

- Rewrite `README.md` (~100 lines): pitch + safe defaults, install, CLI /
  Python / Rust quickstarts (each snippet executed this session),
  examples pointer, grouped docs links.
- Fix the four §3 issues in `examples/README.md` + `docs/deployment.md`.
- Registry + roadmap status + closure record.

### Explicitly out of scope

- Normative rewrites of `docs/` contracts beyond the §3 claim fixes.
- Code, API, dependency, tier, or plan-history changes.
- New examples or new test fixtures (existing `test-examples.sh` +
  wheel smoke tests are the evidence).

## 6. Required production changes

None (docs-only).

## 7. Ordered work packages

### Work package A — Live verification (main agent)

CLI serve + curl matrix; `cargo build -p eggserve-core --examples` +
`test-examples.sh`; maturin wheel build + three Python README snippets on
ephemeral ports.

### Work package B — README rewrite + §3 doc fixes (main agent)

Apply, keeping every new snippet identical to an executed one.

### Work package C — Plan trace + gates

Registry 303 row, roadmap status, closure record, `fmt --check` +
`diff --check` + link check, commit.

## 8. Failure semantics

Docs-only. Any snippet that fails live execution is cut or fixed to the
executed form — never shipped unverified.

## 9. Compatibility and migration

No compatibility effect.

## 10. Required tests

None (docs-only; live execution evidence recorded in closure).

## 11. Required verification commands

```bash
cargo fmt --all -- --check
git diff --check
```

Plus the work-package-A live runs (not gating CI).

## 12. Documentation updates

- `README.md` (rewrite).
- `examples/README.md`, `docs/deployment.md` (§3 fixes).
- `plans/subsystems/docs-architecture-roadmap.md` (303 milestone).
- `plans/registry.md` (register 303).
- `plans/closure/docs-architecture/303-readme-quickstart-user-docs-accuracy.md`.

## 13. Acceptance criteria

- README ≤ ~110 lines, quickstart-first, every snippet executed this
  session, detail behind `docs/`/`examples/` links.
- All §3 issues fixed with code evidence; nothing else touched.
- Gates pass; registry + roadmap + closure complete.

## 14. Stop conditions

Stop and report when a snippet fails live (cut it), when a doc fix needs a
normative decision, or when scope expands into code.

## 15. Closure evidence required

Executed-command log (CLI curl matrix, example smokes, Python snippet
runs); requirement-to-evidence matrix; gate outcomes.

## 16. Handoff notes

Work only in `/home/sugarwookie/projects/eggserve`.

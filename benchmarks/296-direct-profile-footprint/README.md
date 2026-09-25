# Plan 296 — Direct-profile footprint and capability split

Bounded to graph pruning + gating experiments. Landed exactly one
production-manifest change (WP-A); file-body and tunnel splits close NO-GO
with measured evidence.

## Kept: tower-layer deactivation (WP-A)

`eggserve-server/tower` no longer activates `tower-layer`: production
adapter code drives Tower services but never implements the `Layer`
contract (the only in-tree uses are tests + a doc mention). The optional
dependency stays declared (topology-gated Plan-276 surface); tests consume
it via dev-dependencies; downstream `Layer` authors depend on it directly
(`docs/http-interop.md`, `docs/dependency-policy.md` updated).

Effect: −1 package node in the resolved direct Tower graph, link-neutral.
No Rust API, config, default, or behavior change.

## NO-GO: file-body / Tokio-`fs` gating (WP-B)

Upper-bound link attribution ≈ 35 KB (~3% of the stripped Tower fixture)
against public-accessor (`file_stream_semaphore()`), `from_parts`, core
projection, config-validation, and adapter-fork churn with zero package
removal. Benefit does not clear the bar; stopped before API polish per the
plan's stop conditions. File serving in static/core/bin compositions is
unchanged and proven by their suites.

## NO-GO: tunnel gating (WP-C)

294 attributed no tunnel cost (≈ 30 KB symbols by the same method), and
gating would fork the additive `Service` tunnel contract plus per-request
classification. Rejected by the plan's own gate.

## Detail

Method, commands, symbol-attribution caveats, package-consumer numbers, and
the semver classification (minor-level at most, undecided — final call in
297) live in `raw/footprint.txt` and `results.json`.

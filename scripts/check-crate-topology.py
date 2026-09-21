#!/usr/bin/env python3
"""Enforce the Plan 211–253 Cargo dependency topology.

This is intentionally a small metadata check rather than a line-count or
source-layout rule. Cargo's resolved direct package graph is the contract:
canonical primitives are a leaf, the generic server does not pull static
serving, static serving consumes the two lower layers, static
path/filesystem confinement lives once in `eggserve-static` with
`eggserve-core` keeping compatibility facades only, the H3/QUIC adapter
lives once in `eggserve-h3` with downward-only primitives/server deps,
no capability-filesystem crate exists (Plan 224 NO-GO), and the
compatibility core is a classified facade/adapter layer with no leftover
duplicate implementations or dependencies (Plan 225 closure). Direct H1 and
static authority convergence, Python wheel typing artifacts, orphan Rust
sources, and inert accepted compatibility features are checked structurally
(Plans 243–247).
"""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys


def check_forbidden_deps(
    owner: str, deps: set[str], forbidden: set[str], why: str
) -> int:
    """Fail when `deps` contains any `forbidden` dependency (pure predicate).

    Extracted so `--self-test` can prove the dependency rules fail closed
    on synthetic graphs without running `cargo metadata`.
    """
    leaked = deps & forbidden
    if leaked:
        print(f"{owner} leaks forbidden dependencies: {sorted(leaked)} ({why})", file=sys.stderr)
        return 1
    return 0


def _inventory_diff(
    actual: set[str], expected: set[str]
) -> tuple[list[str], list[str]]:
    """Return `(new, gone)` sorted inventory differences (pure predicate)."""
    return sorted(actual - expected), sorted(expected - actual)


def main() -> int:
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            text=True,
        )
    )
    packages = {package["name"]: package for package in metadata["packages"]}

    required = {
        "eggnet-tls",
        "eggserve-primitives",
        "eggserve-server",
        "eggserve-static",
        "eggserve-h3",
    }
    missing = required - packages.keys()
    if missing:
        print(f"missing Plan 211–214 packages: {', '.join(sorted(missing))}", file=sys.stderr)
        return 1

    def direct(name: str) -> set[str]:
        return {
            dependency["name"]
            for dependency in packages[name]["dependencies"]
            if dependency["source"] is None
        }

    def production(name: str) -> set[str]:
        return {
            dependency["name"]
            for dependency in packages[name]["dependencies"]
            if dependency["kind"] is None
        }

    primitives = direct("eggserve-primitives")
    forbidden = {"eggserve-core", "eggserve-server", "eggserve-static", "hyper", "hyper-util", "quinn", "h3", "h3-quinn", "rustls", "tokio", "rustix", "windows-sys"}
    if check_forbidden_deps(
        "eggserve-primitives", primitives, forbidden, "transport-neutral leaf"
    ) != 0:
        return 1

    neutral_tls = direct("eggnet-tls")
    declared_tls = {
        dependency["name"]
        for dependency in packages["eggnet-tls"]["dependencies"]
        if dependency["kind"] is None
    }
    forbidden_tls = {
        "eggserve-core",
        "eggserve-server",
        "eggserve-static",
        "eggserve-bin",
        "eggserve-python",
        "eggress-core",
        "eggfetch",
        "tokio",
        "hyper",
        "hyper-util",
        "quinn",
        "h3",
        "h3-quinn",
        "tracing",
    }
    if declared_tls & forbidden_tls:
        print(
            "eggnet-tls leaks application/transport dependencies: "
            f"{sorted(declared_tls & forbidden_tls)}",
            file=sys.stderr,
        )
        return 1
    if neutral_tls:
        print(
            "eggnet-tls must not depend on workspace application crates: "
            f"{sorted(neutral_tls)}",
            file=sys.stderr,
        )
        return 1
    if not {"rustls", "rustls-pki-types"}.issubset(declared_tls):
        print("eggnet-tls must declare rustls and rustls-pki-types", file=sys.stderr)
        return 1

    server = direct("eggserve-server")
    if check_forbidden_deps(
        "eggserve-server",
        server,
        {"eggserve-core", "eggserve-static"},
        "no core/static edges from the generic server",
    ) != 0:
        return 1
    if "eggserve-primitives" not in server:
        print("eggserve-server must depend on eggserve-primitives", file=sys.stderr)
        return 1
    server_h3 = production("eggserve-server") & {"h3", "h3-quinn", "quinn"}
    if check_forbidden_deps(
        "eggserve-server", server_h3, {"h3", "h3-quinn", "quinn"}, "no direct H3/QUIC stack"
    ) != 0:
        return 1

    static = direct("eggserve-static")
    required_static = {"eggserve-primitives", "eggserve-server"}
    allowed_static = required_static | {"httpdate", "phf", "rustix"}
    if not required_static.issubset(static):
        print(
            "eggserve-static must consume the canonical primitives and server layers: "
            f"missing {sorted(required_static - static)}",
            file=sys.stderr,
        )
        return 1
    unexpected_static = static - allowed_static
    if unexpected_static:
        print(f"unexpected eggserve-static direct dependencies: {sorted(unexpected_static)}", file=sys.stderr)
        return 1

    # Platform confinement dependencies are target-specific and do not appear
    # in the no-deps package dependency set above. Keep the source check small:
    # it prevents the old pathname-based fixture from silently returning as a
    # second production implementation.
    static_root = "crates/eggserve-static/src/secure_root.rs"
    static_fs = "crates/eggserve-static/src/fs"
    for path in (static_root, static_fs):
        if not Path(path).exists():
            print(f"eggserve-static is missing mature confinement source: {path}", file=sys.stderr)
            return 1
    scaffold = Path("crates/eggserve-static/src/lib.rs").read_text()
    if "directory listing disabled in topology fixture" in scaffold:
        print("eggserve-static still contains the Plan 211 scaffold", file=sys.stderr)
        return 1

    h3 = production("eggserve-h3")
    expected_h3 = {"h3", "h3-quinn", "quinn"}
    if not expected_h3.issubset(h3):
        print(
            "eggserve-h3 must own the coordinated H3/QUIC direct dependencies: "
            f"missing {sorted(expected_h3 - h3)}",
            file=sys.stderr,
        )
        return 1
    # Plan 220: H3 is the actual transport adapter, allowed downward on
    # primitives/server (canonical types + shared kernel). It must not depend
    # upward on core/static/bin or become a second static implementation.
    allowed_h3_downward = {"eggserve-primitives", "eggserve-server", "eggnet-tls"}
    if not allowed_h3_downward.issubset(h3):
        print(
            "eggserve-h3 must consume the canonical primitives/server layers: "
            f"missing {sorted(allowed_h3_downward - h3)}",
            file=sys.stderr,
        )
        return 1
    if {"eggserve-core", "eggserve-static", "eggserve-bin", "eggserve-python"} & h3:
        upward = {"eggserve-core", "eggserve-static", "eggserve-bin", "eggserve-python"} & h3
        print(
            "eggserve-h3 must not depend upward on core/static/bin/python "
            f"(downward-only): {sorted(upward)}",
            file=sys.stderr,
        )
        return 1

    if "eggnet-tls" not in direct("eggserve-core"):
        print("eggserve-core must consume the neutral eggnet-tls crate", file=sys.stderr)
        return 1
    core = direct("eggserve-core")
    core_h3 = {"h3", "h3-quinn", "quinn"} & core
    if check_forbidden_deps(
        "eggserve-core", core_h3, {"h3", "h3-quinn", "quinn"}, "no direct H3/QUIC stack"
    ) != 0:
        return 1

    if check_plan215_parity() != 0:
        return 1

    if check_plan216_tunnel() != 0:
        return 1

    if check_plan217_convergence() != 0:
        return 1

    if check_plan219_confinement() != 0:
        return 1

    if check_plan220_h3_extraction() != 0:
        return 1

    if check_plan221_frontends() != 0:
        return 1

    if check_plan225_facade() != 0:
        return 1

    if check_plan244_authority_convergence() != 0:
        return 1

    if check_plan249_h1_authority() != 0:
        return 1

    if check_plan253_overlap() != 0:
        return 1

    if check_plan247_leaf_surfaces() != 0:
        return 1

    # Plan 224 NO-GO: no capability-filesystem crate may appear silently.
    # A future split requires an explicit plan and gate update, not a new
    # package in the resolved graph.
    for forbidden_crate in ("eggserve-capfs", "eggcapfs", "capfs"):
        if forbidden_crate in packages:
            print(
                f"unexpected capability-filesystem crate `{forbidden_crate}`: "
                "Plan 224 closed NO-GO with eggserve-static as the single "
                "confinement authority",
                file=sys.stderr,
            )
            return 1

    print(
        "Plan 211–249 topology: primitives leaf; neutral TLS; "
        "server transport-only; static specializes both; H3 adapter owned; "
        "direct H1 runtime owns ops/errors/policy/authority/service/driver; "
        "direct tunnel authority with neutral vocabulary; "
        "direct service/request convergence with single Service contract; "
        "single static/path/filesystem authority with core facades; "
        "single H3/QUIC adapter with core facades; "
        "Plan 221 frontends name leaf crates directly (neutral paths; "
        "extended orchestration blockers documented); "
        "Plan 224 NO-GO: no capability-filesystem crate; "
        "Plan 225: core is a classified compatibility facade "
        "(no second canonical implementation, no leftover MIME dependency); "
        "Plans 243–247: durable shutdown/task drain, direct H1/static "
        "delegation, Python typing artifacts, orphan-source rejection, and "
        "inert accepted feature names; "
        "Plan 249: single H1 authority (no core Hyper H1 execution, Auto "
        "classifies before Hyper) with structured per-connection shutdown; "
        "Plan 253: classified connection overlap (bounded duplication, "
        "crate-private parallels, gated H2 ownership)"
    )
    return 0


def check_plan244_authority_convergence() -> int:
    """Guard the direct H1/static authority projections (Plans 244–245)."""
    repo = Path(__file__).resolve().parent.parent
    core_connection = (repo / "crates/eggserve-core/src/server/connection/mod.rs").read_text()
    if "eggserve_server::connection::serve_http1_connection" not in core_connection:
        print(
            "core H1 compatibility entry points must delegate to eggserve-server "
            "(Plan 244)",
            file=sys.stderr,
        )
        return 1
    core_static = (repo / "crates/eggserve-core/src/server/static_service.rs").read_text()
    if "eggserve_static::StaticService" not in core_static:
        print(
            "core StaticService must project into eggserve-static (Plan 245)",
            file=sys.stderr,
        )
        return 1
    for marker in ("fn plan_static_request", "fn render_directory_listing", "fn canonical_response"):
        if marker in core_static:
            print(
                f"core static service retains duplicate `{marker}` (Plan 245)",
                file=sys.stderr,
            )
            return 1
    return 0


def check_plan249_h1_authority() -> int:
    """Forbid a second executable core H1 path (Plan 249).

    Why H1 markers are forbidden in core: `eggserve-server` is the single H1
    execution authority (Hyper H1 builder/connection/driver). Compatibility
    `Auto` classification must resolve before any Hyper service exists and
    delegate H1 bytes to the direct driver; core keeps only H2-specific
    execution plus protocol-selection/replay composition. The markers below
    are the executable H1 machinery removed by Plan 249 Track B — their
    return would silently resurrect a core Hyper H1 pipeline beside the
    direct authority. H2 Hyper ownership (`hyper2_builder`,
    `http2::Connection`, `serve_h2_with_token`), the bounded H2
    prior-knowledge classifier (`classify_cleartext`), replay composition
    (`PrefixedIo`), and direct calls into `eggserve_server::connection::*`
    remain allowed.
    """
    repo = Path(__file__).resolve().parent.parent

    def code_lines(text: str) -> str:
        return "\n".join(
            line
            for line in text.splitlines()
            if not line.lstrip().startswith(("///", "//!", "//"))
        )

    # Production code only: `#[cfg(test)]` modules legitimately use
    # `tokio::spawn` (test senders) and assert on `WireProtocol::Http1`.
    driver = code_lines(
        (repo / "crates/eggserve-core/src/server/connection/driver.rs")
        .read_text()
        .split("#[cfg(test)]")[0]
    )
    facade = code_lines(
        (repo / "crates/eggserve-core/src/server/connection/mod.rs")
        .read_text()
        .split("#[cfg(test)]")[0]
    )
    accept = code_lines(
        (repo / "crates/eggserve-core/src/server/accept.rs")
        .read_text()
        .split("#[cfg(test)]")[0]
    )

    # 1. No core HTTP/1 Hyper builder or H1 connection execution ownership.
    for marker in (
        "fn hyper_builder",
        "http1::Builder",
        "http1::Connection",
        "UpgradeableConnection",
    ):
        if marker in driver:
            print(
                f"core connection driver keeps executable H1 `{marker}` "
                "(Plan 249: H1 execution lives once in eggserve-server)",
                file=sys.stderr,
            )
            return 1

    # 2. No removed H1-capable driver helpers (Auto→H1 Hyper execution).
    for marker in (
        "fn serve_connection(",
        "fn serve_hyper_with_token(",
        "fn serve_selected_with_token",
        "fn serve_selected_resolved_with_token",
        "fn serve_hyper_with_token_auto",
    ):
        if marker in driver:
            print(
                f"core connection driver keeps `{marker}` "
                "(Plan 249: Auto classifies before Hyper; H1 delegates, H2 only)",
                file=sys.stderr,
            )
            return 1

    # 3. No second `serve_http1_connection` implementation in the driver;
    #    the mod.rs facades must delegate rather than execute.
    if "fn serve_http1_connection" in driver:
        print(
            "core connection driver keeps a second `serve_http1_connection` "
            "(Plan 249: H1 facades live in mod.rs and delegate to eggserve-server)",
            file=sys.stderr,
        )
        return 1
    for facade_fn in (
        "pub async fn serve_http1_connection",
        "pub async fn serve_http1_connection_with_id",
    ):
        if facade_fn not in facade:
            print(
                f"core connection facade lost `{facade_fn}` (Plan 249: public H1 "
                "entry points remain source-compatible)",
                file=sys.stderr,
            )
            return 1

    # 4. No resolved `WireProtocol::Http1` Hyper-driving block in the
    #    driver. (The classifier still *returns* Http1, the H2 path still
    #    *logs* it via `=> "http/1.1"`, and tests still assert it; only a
    #    Hyper-executing `=> {` block is forbidden.)
    if "WireProtocol::Http1 => {" in driver:
        print(
            "core connection driver keeps a resolved `WireProtocol::Http1` "
            "execution branch (Plan 249: H1 delegates to eggserve-server)",
            file=sys.stderr,
        )
        return 1

    # 5. No detached per-connection shutdown forwarder in the accept path.
    #    Connection tasks are JoinSet-owned; shutdown bridges inline via
    #    `run_with_connection_shutdown` so the receiver drops with the task.
    if "tokio::spawn" in accept:
        print(
            "core accept path spawns a detached task (Plan 249 Track C: "
            "per-connection shutdown forwarding must be structured under the "
            "connection task via `run_with_connection_shutdown`)",
            file=sys.stderr,
        )
        return 1
    if "forwarder_shutdown" in accept or "forwarder_rx" in accept:
        print(
            "core accept path keeps detached-forwarder state (Plan 249 Track C: "
            "remove the resubscribed forwarder task; bridge inline)",
            file=sys.stderr,
        )
        return 1
    if "run_with_connection_shutdown" not in accept:
        print(
            "core accept path lost `run_with_connection_shutdown` "
            "(Plan 249 Track C: structured shutdown ownership)",
            file=sys.stderr,
        )
        return 1

    return 0


def check_plan253_overlap() -> int:
    """Classify the core/server connection overlap (Plan 253).

    H1 execution is single-authority in `eggserve-server` (Plan 249), but
    both crates keep similarly named connection helpers: core owns H2
    execution and multiprotocol composition, so its helpers cannot be
    deleted or shared without either exposing new public Hyper/Tokio
    transport types, activating direct H2/TLS capability, or moving H2
    ownership (all mandatory DEFER conditions). The ledger lives in
    `architecture/crate-topology.md` (Plan 253 section); this gate keeps
    that classification mechanical:

    - every parallel pair still exists (no silent deletion);
    - core parallel helpers stay crate-private (no new public transport
      API grown merely to share source);
    - the direct H3-shared re-export set does not grow;
    - H2-only core modules stay feature-gated.
    """
    import re

    repo = Path(__file__).resolve().parent.parent
    direct_conn = repo / "crates/eggserve-server/src/connection"
    core_conn = repo / "crates/eggserve-core/src/server/connection"

    parallel = (
        "activity",
        "deferred_body",
        "driver",
        "lifecycle",
        "pipeline",
        "request",
        "response",
        "transport",
    )
    for name in parallel:
        for side, root in (("direct", direct_conn), ("core", core_conn)):
            if not (root / f"{name}.rs").exists():
                print(
                    f"Plan 253 overlap pair `{name}` lost its {side} copy: "
                    "reclassify the overlap in architecture/crate-topology.md "
                    "before deleting a parallel connection module",
                    file=sys.stderr,
                )
                return 1

    # Core parallel helpers must not grow fully-public items: sharing
    # source across the crate boundary through a new public Hyper/Tokio
    # transport type is a mandatory Plan 253 DEFER condition. Only
    # column-zero declarations count; methods inside `impl` blocks belong
    # to their already-scoped type.
    public_item = re.compile(r"^pub (async fn|fn|struct|enum|use|mod|type|const|static)\b")
    for name in parallel:
        text = (core_conn / f"{name}.rs").read_text().split("#[cfg(test)]")[0]
        for line in text.splitlines():
            if public_item.match(line):
                print(
                    f"core connection/{name}.rs grows public `{line.strip()}` "
                    "(Plan 253: core parallel helpers stay crate-private; "
                    "sharing them needs a new public transport API, which is "
                    "a mandatory DEFER)",
                    file=sys.stderr,
                )
                return 1

    # The direct H3-shared surface is fixed: lifecycle registry, one
    # body-policy selector, and the canonical service/privacy kernel.
    # Growth here solely so core can call the same helper is forbidden.
    allowed_direct_public = {
        "lifecycle.rs": {"pub struct ConnectionRequests {", "pub fn cancel_shared_with_observability("},
        "request.rs": {"pub fn select_body_policy("},
        "response.rs": {
            "pub async fn contain_service_panic<F>(",
            "pub async fn invoke_canonical_service<S>(",
            "pub fn finalize_canonical_response(",
        },
    }
    for name in ("activity", "deferred_body", "driver", "pipeline", "transport"):
        allowed_direct_public[name + ".rs"] = set()
    for name, allowed in allowed_direct_public.items():
        text = (direct_conn / name).read_text().split("#[cfg(test)]")[0]
        for line in text.splitlines():
            if public_item.match(line) and line.strip() not in allowed:
                print(
                    f"direct connection/{name} grows public `{line.strip()}` "
                    "(Plan 253: the H3-shared surface is fixed; do not make "
                    "a private helper public solely for core)",
                    file=sys.stderr,
                )
                return 1

    # H2-only core modules stay feature-gated so default builds compile
    # only the delegating facade plus classifier/replay composition.
    facade = (core_conn / "mod.rs").read_text()
    for name in ("activity", "deferred_body", "lifecycle", "pipeline", "request", "response", "transport"):
        pattern = re.compile(
            r'#\[cfg\(feature = "http2"\)\]\s*\n\s*pub\(crate\) mod ' + name + r";"
        )
        if not pattern.search(facade):
            print(
                f"core connection/{name} lost its http2 feature gate "
                "(Plan 253: H2-only helpers must not compile into default "
                "core builds)",
                file=sys.stderr,
            )
            return 1

    return 0


def _rust_module_declarations(source: str) -> list[tuple[str, str | None]]:
    """Return external module declarations and an optional path override."""
    import re

    clean = re.sub(r"(?s)/\*.*?\*/", "", source)
    clean = re.sub(r"//[^\n]*", "", clean)
    declarations: list[tuple[str, str | None]] = []
    path_override: str | None = None
    for line in clean.splitlines():
        path_match = re.search(r"#\s*\[\s*path\s*=\s*\"([^\"]+)\"\s*\]", line)
        if path_match:
            path_override = path_match.group(1)
            continue
        match = re.search(r"\bmod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;", line)
        if match:
            declarations.append((match.group(1), path_override))
            path_override = None
    return declarations


def check_plan247_leaf_surfaces() -> int:
    """Reject orphan production Rust sources and misleading direct features."""
    import tomllib

    repo = Path(__file__).resolve().parent.parent
    orphaned: list[str] = []
    for src in sorted((repo / "crates").glob("*/src")):
        roots = [path for path in (src / "lib.rs", src / "main.rs") if path.exists()]
        reachable: set[Path] = set()
        pending = list(roots)
        while pending:
            current = pending.pop()
            current = current.resolve()
            if current in reachable or not current.exists():
                continue
            reachable.add(current)
            if current.name in {"lib.rs", "main.rs", "mod.rs"}:
                base = current.parent
            else:
                base = current.parent / current.stem
            for name, override in _rust_module_declarations(current.read_text()):
                candidate = base / override if override else base / f"{name}.rs"
                if not candidate.exists():
                    candidate = base / name / "mod.rs"
                if candidate.exists():
                    pending.append(candidate)
        for source in sorted(src.rglob("*.rs")):
            relative = source.relative_to(src)
            if any(part in {"tests", "examples", "benches"} for part in relative.parts):
                continue
            if source.resolve() not in reachable:
                orphaned.append(str(source.relative_to(repo)))
    if orphaned:
        print(
            "orphan production Rust sources (Plan 247): " + ", ".join(orphaned),
            file=sys.stderr,
        )
        return 1

    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            text=True,
        )
    )
    packages = {package["name"]: package for package in metadata["packages"]}
    expected_inert = {
        "eggserve-server": {"http2": [], "tls": []},
        "eggserve-primitives": {"http-interop": []},
    }
    for package_name, feature_names in expected_inert.items():
        features = packages[package_name]["features"]
        for feature, expected in feature_names.items():
            if features.get(feature) != expected:
                print(
                    f"{package_name}/{feature} no longer has its documented "
                    f"reserved effect: {features.get(feature)!r}",
                    file=sys.stderr,
                )
                return 1
    return 0


def check_plan215_parity() -> int:
    """Enforce Plan 215 direct-embeddable runtime ownership.

    Structural (not line-count) rules: the mature H1 vocabulary lives in
    `eggserve-server`, compatibility facades re-export it, and the direct
    server never imports the compatibility core or static layer.
    """
    repo = Path(__file__).resolve().parent.parent
    server_src = repo / "crates" / "eggserve-server" / "src"
    core_src = repo / "crates" / "eggserve-core" / "src"

    def read(path: Path) -> str:
        return path.read_text()

    # 1. Direct server source must never import the compatibility core or
    #    the static layer (no upward dependency, including tests/examples).
    #    Doc-comment cross-references are ignored; only code counts.
    offenders = []
    for path in list((server_src).rglob("*.rs")) + list(
        (repo / "crates" / "eggserve-server" / "examples").glob("*.rs")
    ):
        code_lines = [
            line
            for line in read(path).splitlines()
            if not line.lstrip().startswith(("///", "//!"))
        ]
        code = "\n".join(code_lines)
        if "eggserve_core" in code or "eggserve-static" in code or "eggserve_static" in code:
            offenders.append(str(path.relative_to(repo)))
    if offenders:
        print(
            "eggserve-server must not reference eggserve-core/static: "
            f"{sorted(offenders)}",
            file=sys.stderr,
        )
        return 1

    # 2. Single-definition authorities owned by the direct server crate.
    owned = {
        "ops/mod.rs": ["pub struct OpsContext", "pub struct Logger"],
        "ops/events.rs": ["pub enum EventKind", "pub struct Event"],
        "ops/counters.rs": ["pub struct OpsCounters"],
        "errors.rs": ["pub enum ServerError", "pub enum ShutdownResult"],
        "response_policy.rs": ["pub struct ResponsePolicy", "pub enum DatePolicy"],
        "runtime_limits.rs": ["pub struct SharedRuntimeValues", "pub struct Violation"],
        "service.rs": ["pub trait Service", "pub struct ServiceError", "pub fn service_fn"],
        "connection/context.rs": [
            "pub struct ConnectionContext",
            "pub struct ConnectionShutdown",
            "pub enum ConnectionOutcome",
        ],
        "config.rs": ["pub struct RuntimeConfig", "pub struct RuntimeConfigBuilder"],
        "runtime.rs": ["pub struct RuntimeState"],
        "connection/mod.rs": [
            "pub async fn serve_http1_connection",
            "pub async fn serve_http1_connection_with_id",
        ],
        "connection/driver.rs": ["async fn drive_connection", "fn hyper_builder"],
        "connection/pipeline.rs": ["fn make_canonical_hyper_service", "async fn invoke_service"],
        "adapters.rs": ["pub fn to_hyper_response"],
    }
    for rel, markers in owned.items():
        text = read(server_src / rel)
        for marker in markers:
            if marker not in text:
                print(
                    f"eggserve-server/{rel} must own `{marker}` (Plan 215)",
                    file=sys.stderr,
                )
                return 1

    # 3. Compatibility facades for moved modules must re-export the direct
    #    implementation rather than define a second one.
    facades = {
        "ops/mod.rs": "pub use eggserve_server::ops::*;",
        "server/errors.rs": "pub use eggserve_server::errors::*;",
        "server/response_policy.rs": "pub use eggserve_server::response_policy::*;",
        "policy.rs": "pub use eggserve_primitives::policy::*;",
        "runtime_limits.rs": "pub use eggserve_server::runtime_limits::*;",
    }
    for rel, marker in facades.items():
        text = read(core_src / rel)
        if marker not in text:
            print(
                f"eggserve-core/{rel} must re-export the direct authority "
                f"(`{marker}`, Plan 215)",
                file=sys.stderr,
            )
            return 1

    # 4. Single service contract (Plan 217 supersedes Plan 215 shape parity).
    #    Before convergence both definitions carried the mature categories;
    #    after convergence the compatibility file is a re-export and the
    #    shape lives once in the direct crate.
    server_service = read(server_src / "service.rs")
    core_service = read(core_src / "server" / "service.rs")
    if "pub use eggserve_server::service::*;" in core_service:
        # Converged: shape owned once by the direct crate.
        shape_markers = [
            "Panic",
            "fn is_panic",
            "fn is_timeout",
            "fn message",
            "service_fn_with_policy",
            "service_fn_head",
            "call_with_tunnel",
            "service_fn_with_tunnel",
        ]
        for marker in shape_markers:
            if marker not in server_service:
                print(
                    "service contract shape diverged: "
                    f"`{marker}` must appear in the direct definition (Plan 217)",
                    file=sys.stderr,
                )
                return 1
    else:
        # Pre-convergence parity (retained for revert safety).
        shape_markers = [
            "Panic",
            "fn is_panic",
            "fn is_timeout",
            "fn message",
            "service_fn_with_policy",
            "service_fn_head",
        ]
        for marker in shape_markers:
            if marker not in server_service or marker not in core_service:
                print(
                    "service contract shape diverged: "
                    f"`{marker}` must appear in both server and core definitions (Plan 215)",
                    file=sys.stderr,
                )
                return 1

    return 0


def check_plan216_tunnel() -> int:
    """Enforce Plan 216 direct generic tunnel/upgrade ownership.

    Structural (not line-count) rules: neutral intent vocabulary lives in
    `eggserve-primitives` (Hyper/Tokio-free), transport execution lives in
    `eggserve-server`, and the compatibility core delegates (facade +
    thin wrapper) instead of keeping a second H1 parser/state
    machine/bridge. The test-only WebSocket codec stays dev-only.
    """
    import re
    import tomllib

    repo = Path(__file__).resolve().parent.parent

    def read(path: Path) -> str:
        return path.read_text()

    def code_lines(text: str) -> str:
        return "\n".join(
            line
            for line in text.splitlines()
            if not line.lstrip().startswith(("///", "//!"))
        )

    # 1. Primitives tunnel vocabulary must stay transport-neutral: no
    #    Hyper/Tokio/H2/H3/QUIC imports in code (docs may name the boundary).
    primitives_tunnel = read(
        repo / "crates" / "eggserve-primitives" / "src" / "primitives" / "tunnel.rs"
    )
    code = code_lines(primitives_tunnel)
    for forbidden in (
        "hyper",
        "hyper_util",
        "tokio",
        "rustls",
        "quinn",
        "windows-sys",
    ):
        if re.search(rf"(^|\W){re.escape(forbidden)}\s*::", code):
            print(
                f"eggserve-primitives tunnel leaks `{forbidden}` (Plan 216)",
                file=sys.stderr,
            )
            return 1
    # `h2`/`h3` are short: match path use only, not prose.
    if re.search(r"use\s+h[23]\s*::", code) or re.search(r"\bh[23]\s*::", code):
        print(
            "eggserve-primitives tunnel leaks h2/h3 (Plan 216)",
            file=sys.stderr,
        )
        return 1

    # 2. Direct server owns the H1 tunnel transport machinery.
    server_tunnel = read(repo / "crates" / "eggserve-server" / "src" / "tunnel.rs")
    for marker in (
        "pub struct TunnelCapability",
        "pub struct TunnelIo",
        "pub fn accept",
        "pub async fn run_tunnel",
        "fn classify_tunnel",
        "fn admit_and_spawn",
    ):
        if marker not in server_tunnel:
            print(
                f"eggserve-server/src/tunnel.rs must own `{marker}` (Plan 216)",
                file=sys.stderr,
            )
            return 1

    # 3. No second H1 tunnel transport in the compatibility core.
    if (repo / "crates" / "eggserve-core" / "src" / "server" / "connection" / "tunnel.rs").exists():
        print(
            "eggserve-core retains server/connection/tunnel.rs: H1 transport "
            "must delegate to eggserve-server (Plan 216)",
            file=sys.stderr,
        )
        return 1
    core_tunnel = read(
        repo / "crates" / "eggserve-core" / "src" / "primitives" / "tunnel.rs"
    )
    core_code = code_lines(core_tunnel)
    for marker in (
        "struct TunnelShared",
        "struct TunnelAcceptance",
        "struct TunnelIo",
        "copy_bidirectional",
        "tokio::io::duplex",
    ):
        if marker in core_code:
            print(
                f"eggserve-core primitives/tunnel.rs keeps a second `{marker}` "
                "(Plan 216: delegate to the direct authority)",
                file=sys.stderr,
            )
            return 1
    for marker in (
        "pub use eggserve_primitives::tunnel::",
        "pub use eggserve_server::tunnel::",
    ):
        if marker not in core_tunnel:
            print(
                f"eggserve-core primitives/tunnel.rs must facade `{marker}` (Plan 216)",
                file=sys.stderr,
            )
            return 1

    # 4. Test-only WebSocket codec stays dev-only in every manifest.
    for rel in (
        "Cargo.toml",
        "crates/eggserve-primitives/Cargo.toml",
        "crates/eggserve-server/Cargo.toml",
        "crates/eggserve-static/Cargo.toml",
        "crates/eggserve-core/Cargo.toml",
        "crates/eggserve-bin/Cargo.toml",
        "crates/eggserve-python/Cargo.toml",
    ):
        data = tomllib.loads((repo / rel).read_text())
        for section in ("dependencies",):
            if "tokio-tungstenite" in data.get(section, {}):
                print(
                    f"{rel} has production tokio-tungstenite (Plan 216: dev-only)",
                    file=sys.stderr,
                )
                return 1

    return 0


def check_plan217_convergence() -> int:
    """Enforce Plan 217 direct service/request type convergence.

    Structural (not line-count) rules: canonical request/service types are
    owned by the direct crates, compatibility files are facades, primitives
    stay Hyper/Tokio-free, and H1/H2 pipelines invoke the single
    `eggserve-server::Service` contract (tunnel via `call_with_tunnel`,
    shared `run_tunnel` future, no second bridge).
    """
    import re

    repo = Path(__file__).resolve().parent.parent

    def read(path: Path) -> str:
        return path.read_text()

    def code_lines(text: str) -> str:
        return "\n".join(
            line
            for line in text.splitlines()
            if not line.lstrip().startswith(("///", "//!"))
        )

    # 1. Primitives must stay transport-neutral: no Hyper/rustls/QUIC
    #    imports in code (docs may name the boundary) across all primitive
    #    modules, not just tunnel. Tokio is allowed only for the
    #    `Semaphore::MAX_PERMITS` constant in shared limit validation
    #    (pre-existing; no runtime use).
    primitives_dir = repo / "crates" / "eggserve-primitives" / "src" / "primitives"
    for path in list(primitives_dir.rglob("*.rs")):
        full = read(path)
        # Tests legitimately use Tokio (dev-dependency); only production code
        # must stay neutral. Strip the `#[cfg(test)]` module before checking.
        code = code_lines(full.split("#[cfg(test)]")[0])
        for forbidden in (
            "hyper",
            "hyper_util",
            "rustls",
            "quinn",
            "windows-sys",
        ):
            if re.search(rf"(^|\W){re.escape(forbidden)}\s*::", code):
                print(
                    f"eggserve-primitives {path.relative_to(repo)} leaks "
                    f"`{forbidden}` (Plan 217: Hyper/TLS/QUIC-free)",
                    file=sys.stderr,
                )
                return 1
        # Tokio: allow only the shared-limit MAX_PERMITS constant.
        if re.search(r"(^|\W)tokio\s*::", code) and (
            "tokio::sync::Semaphore::MAX_PERMITS" not in code
        ):
            print(
                f"eggserve-primitives {path.relative_to(repo)} leaks "
                "`tokio` (Plan 217: Tokio-free except MAX_PERMITS)",
                file=sys.stderr,
            )
            return 1
        if re.search(r"use\s+h[23]\s*::", code) or re.search(r"\bh[23]\s*::", code):
            print(
                f"eggserve-primitives {path.relative_to(repo)} leaks h2/h3 (Plan 217)",
                file=sys.stderr,
            )
            return 1

    # 2. Compatibility primitive files must be facades (re-export the direct
    #    authority) rather than second definitions. Exact ownership checks,
    #    not line counts.
    core_primitives = repo / "crates" / "eggserve-core" / "src" / "primitives"
    facade_expectations = {
        # Trivial value objects (byte-identical → pure re-export).
        "method.rs": ("pub use eggserve_primitives::method::*;", []),
        "request_target.rs": ("pub use eggserve_primitives::request_target::*;", []),
        "trailers.rs": ("pub use eggserve_primitives::trailers::*;", []),
        "proxy.rs": ("pub use eggserve_primitives::proxy::*;", []),
        "connection_info.rs": ("pub use eggserve_primitives::connection_info::*;", []),
        "body.rs": ("pub use eggserve_primitives::body::*;", []),
        "http.rs": ("pub use eggserve_primitives::http::*;", []),
        "incomplete_body_policy.rs": (
            "pub use eggserve_primitives::incomplete_body_policy::*;",
            [],
        ),
        "interim.rs": ("pub use eggserve_primitives::interim::*;", []),
        "request_body_error.rs": (
            "pub use eggserve_primitives::request_body_error::*;",
            [],
        ),
        "request_body_policy.rs": (
            "pub use eggserve_primitives::request_body_policy::*;",
            [],
        ),
        # Nominal duplicates with visibility/doc-only differences.
        "request.rs": ("pub use eggserve_primitives::request::*;", ["pub struct Request"]),
        "request_body.rs": (
            "pub use eggserve_primitives::request_body::*;",
            ["pub struct RequestBody"],
        ),
        "request_lifecycle.rs": (
            "pub use eggserve_primitives::request_lifecycle::*;",
            ["pub struct RequestLifecycle", "pub(crate) struct RequestShared"],
        ),
        "request_context.rs": (
            "pub use eggserve_primitives::request_context::*;",
            ["pub struct RequestContext", "take_tunnel", "with_tunnel("],
        ),
        "request_head.rs": (
            "pub use eggserve_primitives::request_head::*;",
            ["pub struct RequestHead", "try_from_hyper"],
        ),
        "version.rs": (
            "pub use eggserve_primitives::version::*;",
            ["pub enum HttpVersion", "hyper::http::Version"],
        ),
        "response.rs": ("pub use eggserve_primitives::response::*;", ["pub struct FileRange"]),
        "response_stream.rs": (
            "pub use eggserve_primitives::response_stream::*;",
            ["pub struct ResponseStream"],
        ),
        "canonical.rs": (
            "pub use eggserve_primitives::canonical::*;",
            ["pub mod adapters;", "pub mod headers;", "pub mod response;"],
        ),
        "tunnel.rs": (
            "pub use eggserve_primitives::tunnel::",
            ["pub struct TunnelCapability", "copy_bidirectional"],
        ),
    }
    for rel, (marker, forbidden_markers) in facade_expectations.items():
        text = read(core_primitives / rel)
        code = code_lines(text)
        if marker not in text:
            print(
                f"eggserve-core/primitives/{rel} must facade `{marker}` (Plan 217)",
                file=sys.stderr,
            )
            return 1
        for forbidden in forbidden_markers:
            if forbidden in code:
                print(
                    f"eggserve-core/primitives/{rel} keeps a second `{forbidden}` "
                    "(Plan 217: delegate to the direct authority)",
                    file=sys.stderr,
                )
                return 1
    # Tunnel facade must also re-export the server-owned execution types.
    core_tunnel = read(core_primitives / "tunnel.rs")
    if "pub use eggserve_server::tunnel::" not in core_tunnel:
        print(
            "eggserve-core primitives/tunnel.rs must facade "
            "`pub use eggserve_server::tunnel::` (Plan 217)",
            file=sys.stderr,
        )
        return 1
    # Canonical facade must delegate Hyper conversion to the server adapter.
    core_canonical = read(core_primitives / "canonical.rs")
    if "pub use eggserve_server::adapters::" not in core_canonical:
        print(
            "eggserve-core primitives/canonical.rs must delegate Hyper conversion "
            "to `eggserve_server::adapters` (Plan 217: single conversion authority)",
            file=sys.stderr,
        )
        return 1

    # 3. Single service contract: core service must re-export the direct
    #    authority, not define a second trait/error taxonomy.
    core_service = read(
        repo / "crates" / "eggserve-core" / "src" / "server" / "service.rs"
    )
    if "pub use eggserve_server::service::*;" not in core_service:
        print(
            "eggserve-core/server/service.rs must re-export "
            "`eggserve_server::service` (Plan 217: single Service contract)",
            file=sys.stderr,
        )
        return 1
    for second in ("pub trait Service", "pub struct ServiceError", "enum ServiceErrorKind"):
        if second in code_lines(core_service):
            print(
                f"eggserve-core service keeps a second `{second}` "
                "(Plan 217: single contract)",
                file=sys.stderr,
            )
            return 1

    # 4. H1/H2 pipelines must invoke the single contract via
    #    `call_with_tunnel` and share the `run_tunnel` future (no second
    #    bridge). H2 Extended CONNECT stays as explicit transport glue.
    pipeline = read(
        repo / "crates" / "eggserve-core" / "src" / "server" / "connection" / "pipeline.rs"
    )
    for marker in (
        "call_with_tunnel",
        "eggserve_server::tunnel::run_tunnel",
        "struct TunnelInvocation",
        "with_tunnel_request",
    ):
        if marker not in pipeline:
            print(
                f"eggserve-core pipeline must use `{marker}` "
                "(Plan 217: H2 dispatches through the canonical contract)",
                file=sys.stderr,
            )
            return 1
    for second in ("tokio::io::duplex", "copy_bidirectional"):
        if second in code_lines(pipeline):
            print(
                f"eggserve-core pipeline keeps a second tunnel bridge `{second}` "
                "(Plan 217: shared run_tunnel only)",
                file=sys.stderr,
            )
            return 1

    # 5. Downstream fixture proving one `eggserve-server::Service` drives
    #    both direct H1 and compatibility H2 paths.
    fixture = repo / "crates" / "eggserve-core" / "tests" / "direct_service_convergence.rs"
    if not fixture.exists():
        print(
            "missing Plan 217 downstream fixture "
            "crates/eggserve-core/tests/direct_service_convergence.rs",
            file=sys.stderr,
        )
        return 1
    fixture_text = read(fixture)
    for marker in (
        "eggserve_server::Service",
        "service_fn",
        "serve_http1_connection",
        "serve_http_connection",
    ):
        if marker not in fixture_text:
            print(
                f"Plan 217 fixture must exercise `{marker}` "
                "(direct Service through H1 + H2)",
                file=sys.stderr,
            )
            return 1

    return 0


def check_plan219_confinement() -> int:
    """Enforce Plan 219 single static/path/filesystem authority.

    Structural (not line-count) rules: `eggserve-static` owns path parsing,
    secure-root resolution, filesystem confinement, MIME selection, and
    response planning; `eggserve-core` keeps compatibility facades (re-export
    or minimal wrapper) and no second Unix/Windows resolver, parser, or
    planner. Core must not pull static-only platform dependencies for
    confinement.
    """
    import tomllib

    repo = Path(__file__).resolve().parent.parent

    def read(path: Path) -> str:
        return path.read_text()

    def code_lines(text: str) -> str:
        return "\n".join(
            line
            for line in text.splitlines()
            if not line.lstrip().startswith(("///", "//!"))
        )

    core_src = repo / "crates" / "eggserve-core" / "src"
    static_src = repo / "crates" / "eggserve-static" / "src"

    # 1. No second confinement implementation in the compatibility core.
    for rel in ("fs", "path", "mime.rs", "path.rs", "fs.rs"):
        if (core_src / rel).exists():
            print(
                f"eggserve-core/src/{rel} exists: static/path/filesystem "
                "authority must live once in eggserve-static (Plan 219)",
                file=sys.stderr,
            )
            return 1
    lib_rs = read(core_src / "lib.rs")
    for module in ("mod fs", "mod path", "mod mime"):
        if module in code_lines(lib_rs):
            print(
                f"eggserve-core/src/lib.rs declares `{module}`: duplicate "
                "confinement authority (Plan 219)",
                file=sys.stderr,
            )
            return 1

    # 2. Core secure-root/planner modules must be facades over the static
    #    authority, not second definitions.
    secure_root = read(core_src / "primitives" / "secure_root.rs")
    if "pub use eggserve_static::" not in secure_root:
        print(
            "eggserve-core/primitives/secure_root.rs must facade "
            "`pub use eggserve_static::` (Plan 219)",
            file=sys.stderr,
        )
        return 1
    for second in (
        "pub struct SecureRoot",
        "pub struct ResolvedFile",
        "pub struct ResolvedDirectory",
        "pub enum ResolvedResource",
        "pub enum ResourceDeniedReason",
        "pub fn resolve_and_plan",
        "struct PinnedRoot",
        "struct RootGuard",
    ):
        if second in code_lines(secure_root):
            print(
                f"eggserve-core secure_root keeps a second `{second}` "
                "(Plan 219: delegate to the static authority)",
                file=sys.stderr,
            )
            return 1
    planner = read(core_src / "primitives" / "planner.rs")
    if "pub use eggserve_static::" not in planner:
        print(
            "eggserve-core/primitives/planner.rs must facade "
            "`pub use eggserve_static::` (Plan 219)",
            file=sys.stderr,
        )
        return 1
    if "pub fn plan_file_response" in code_lines(planner):
        print(
            "eggserve-core planner keeps a second `pub fn plan_file_response` "
            "(Plan 219: delegate to the static authority)",
            file=sys.stderr,
        )
        return 1

    # 3. The static authority must expose the surface the facades preserve.
    static_lib = read(static_src / "lib.rs")
    for marker in (
        "pub mod path",
        "ConfinedPath",
        "SecureRoot",
        "plan_file_response_with_preconditions_and_metadata",
        "resolve_and_plan",
    ):
        if marker not in static_lib:
            print(
                f"eggserve-static/src/lib.rs must expose `{marker}` (Plan 219)",
                file=sys.stderr,
            )
            return 1
    static_path = read(static_src / "path" / "mod.rs")
    if "pub struct ConfinedPath" not in static_path:
        print(
            "eggserve-static/src/path/mod.rs must own "
            "`pub struct ConfinedPath` (Plan 219)",
            file=sys.stderr,
        )
        return 1

    # 4. Core must not use static-only platform dependencies for confinement:
    #    no `rustix::fs` paths in core source, and the unix target must not
    #    enable the rustix `fs` feature (listener code needs `net` only;
    #    `rustix::io::Errno` is ungated).
    for path in list(core_src.rglob("*.rs")):
        code = code_lines(read(path))
        if "rustix::fs" in code or "rustix/fs" in code:
            print(
                f"eggserve-core {path.relative_to(repo)} uses rustix::fs: "
                "confinement platform code belongs in eggserve-static (Plan 219)",
                file=sys.stderr,
            )
            return 1
    core_manifest = tomllib.loads(
        (repo / "crates" / "eggserve-core" / "Cargo.toml").read_text()
    )
    rustix_features = (
        core_manifest.get("target", {})
        .get("'cfg(unix)'", {})
        .get("dependencies", {})
        .get("rustix", {})
        .get("features", [])
    )
    if "fs" in rustix_features:
        print(
            "eggserve-core must not enable the rustix `fs` feature: "
            "static confinement lives in eggserve-static (Plan 219)",
            file=sys.stderr,
        )
        return 1

    # 5. Authority conformance fixture proving core paths resolve to the
    #    static implementation.
    fixture = (
        repo / "crates" / "eggserve-core" / "tests" / "static_authority_conformance.rs"
    )
    if not fixture.exists():
        print(
            "missing Plan 219 authority fixture "
            "crates/eggserve-core/tests/static_authority_conformance.rs",
            file=sys.stderr,
        )
        return 1
    fixture_text = read(fixture)
    for marker in (
        "eggserve_static::SecureRoot",
        "eggserve_static::ConfinedPath",
        "static_authority",
    ):
        if marker not in fixture_text:
            print(
                f"Plan 219 fixture must exercise `{marker}` "
                "(core facade against the static authority)",
                file=sys.stderr,
            )
            return 1

    return 0


def check_plan220_h3_extraction() -> int:
    """Enforce Plan 220 H3 adapter extraction.

    Structural (not line-count) rules: `eggserve-h3` owns endpoint/request/
    response/tunnel/QUIC/config mechanics over canonical primitives/server
    types; `eggserve-core` keeps a thin facade (no second state machine,
    no direct Quinn/H3 use, config + QUIC assembly delegated).
    """
    repo = Path(__file__).resolve().parent.parent

    def read(path: Path) -> str:
        return path.read_text()

    def code_lines(text: str) -> str:
        return "\n".join(
            line
            for line in text.splitlines()
            if not line.lstrip().startswith(("///", "//!"))
        )

    h3_src = repo / "crates" / "eggserve-h3" / "src"
    core_src = repo / "crates" / "eggserve-core" / "src"

    # 1. No second H3 state machine in the compatibility core.
    if (core_src / "server" / "http3").exists():
        print(
            "eggserve-core retains server/http3/: H3 mechanics must live once "
            "in eggserve-h3 (Plan 220)",
            file=sys.stderr,
        )
        return 1
    for rel in ("server/http3.rs", "server/http3", "server/http3/endpoint.rs"):
        # http3.rs facade must exist; the directory must not.
        pass
    facade = read(core_src / "server" / "http3.rs")
    if "eggserve_h3::accept_loop" not in facade:
        print(
            "eggserve-core/server/http3.rs must delegate to "
            "`eggserve_h3::accept_loop` (Plan 220: thin facade)",
            file=sys.stderr,
        )
        return 1
    for second in (
        "h3::server::builder",
        "RequestStream",
        "copy_bidirectional",
        "quinn::Connection",
        "h3_quinn::Endpoint::server",
        "quinn::Endpoint::new",
    ):
        if second in code_lines(facade):
            print(
                f"eggserve-core H3 facade keeps a second `{second}` "
                "(Plan 220: delegate to the H3 authority)",
                file=sys.stderr,
            )
            return 1

    # 2. H3 crate owns the adapter surface.
    lib_rs = read(h3_src / "lib.rs")
    for marker in (
        "pub mod adapter",
        "pub mod config",
        "pub mod endpoint",
        "pub mod quic",
        "pub mod request",
        "pub mod response",
        "pub mod tunnel",
        "pub use adapter::",
        "pub use config::Http3Config",
    ):
        if marker not in lib_rs:
            print(
                f"eggserve-h3/src/lib.rs must expose `{marker}` (Plan 220)",
                file=sys.stderr,
            )
            return 1
    adapter = read(h3_src / "adapter.rs")
    for marker in (
        "pub async fn accept_loop",
        "Http3Config",
        "apply_alt_svc",
        "serve_connection",
        "handle_request",
        "handle_h3_connect",
    ):
        if marker not in adapter:
            print(
                f"eggserve-h3/src/adapter.rs must own `{marker}` (Plan 220)",
                file=sys.stderr,
            )
            return 1
    for mod_name, markers in {
        "endpoint.rs": ["ActiveConnectionGuard", "h3_connection_close_reason"],
        "request.rs": ["convert_request_head", "declared_content_length", "h3_trailers_to_block"],
        "response.rs": ["send_canonical_response", "send_response_or_cancel", "spawn_body_timeout_watchdog"],
        "tunnel.rs": ["kind_string", "H3ActiveTunnelGuard", "send_h3_tunnel_handshake"],
        "config.rs": ["pub struct Http3Config", "pub fn validate"],
        "quic.rs": ["load_quic_server_config", "server_endpoint", "endpoint_from_socket"],
    }.items():
        text = read(h3_src / mod_name)
        for marker in markers:
            if marker not in text:
                print(
                    f"eggserve-h3/src/{mod_name} must own `{marker}` (Plan 220)",
                    file=sys.stderr,
                )
                return 1

    # 3. Config authority lives once in H3; core is a facade.
    core_h3_config = read(core_src / "server" / "config" / "http3.rs")
    if "pub use eggserve_h3::Http3Config" not in core_h3_config:
        print(
            "eggserve-core/server/config/http3.rs must facade "
            "`pub use eggserve_h3::Http3Config` (Plan 220)",
            file=sys.stderr,
        )
        return 1
    if "pub struct Http3Config" in code_lines(core_h3_config):
        print(
            "eggserve-core keeps a second `pub struct Http3Config` "
            "(Plan 220: delegate to the H3 authority)",
            file=sys.stderr,
        )
        return 1

    # 4. QUIC assembly lives once in H3; core TLS delegates.
    core_tls = read(core_src / "tls.rs")
    if "eggserve_h3::load_quic_server_config" not in core_tls:
        print(
            "eggserve-core/src/tls.rs must delegate QUIC assembly to "
            "`eggserve_h3::load_quic_server_config` (Plan 220)",
            file=sys.stderr,
        )
        return 1
    for second in ("TransportConfig", "with_single_cert", "QuicServerConfig"):
        if second in code_lines(core_tls):
            print(
                f"eggserve-core tls keeps a second QUIC `{second}` "
                "(Plan 220: H3-owned assembly only)",
                file=sys.stderr,
            )
            return 1

    # 5. Server startup uses H3-owned endpoint helpers, not direct Quinn.
    server_mod = read(core_src / "server" / "mod.rs")
    for marker in (
        "eggserve_h3::server_endpoint",
        "eggserve_h3::endpoint_from_socket",
        "eggserve_h3::validate_same_port_udp",
    ):
        if marker not in server_mod:
            print(
                f"eggserve-core/server/mod.rs must use `{marker}` "
                "(Plan 220: H3-owned endpoint assembly)",
                file=sys.stderr,
            )
            return 1
    for second in ("quinn::Endpoint::new", "h3_quinn::Endpoint::server"):
        if second in code_lines(server_mod):
            print(
                f"eggserve-core server keeps direct QUIC `{second}` "
                "(Plan 220: H3-owned assembly only)",
                file=sys.stderr,
            )
            return 1

    # 6. Shared kernel stays single: core must not keep H3-only canonical
    #    helpers that now live in server/H3.
    core_resp = read(core_src / "server" / "connection" / "response.rs")
    for second in ("pub(crate) async fn invoke_canonical_service", "pub(crate) fn finalize_canonical_response"):
        if second in code_lines(core_resp):
            print(
                f"eggserve-core connection/response keeps `{second}` "
                "(Plan 220: shared kernel in server, Alt-Svc in H3)",
                file=sys.stderr,
            )
            return 1

    return 0


def check_plan221_frontends() -> int:
    """Enforce Plan 221 first-party frontend leaf-crate migration (progress gate).

    Structural (not line-count) rules: `eggserve-bin` and `eggserve-python`
    name the canonical leaf crates directly for neutral policy/primitives,
    runtime, static authority, and TLS substrate paths. The extended server
    orchestration (serve_config/try_from_serve_config, full Server with
    TLS/H2/H3, full StaticService with extra headers/error policy,
    ServeConfig/Limits static budgets, ServerHandle lifecycle) remains
    compatibility-owned until Plan 225, so a narrow documented blocker set
    is allowed. The Python -> bin extension CLI (`eggserve_bin::run_cli`)
    is confirmed used and remains.
    """
    repo = Path(__file__).resolve().parent.parent

    def read(path: Path) -> str:
        return path.read_text()

    def code_lines(text: str) -> str:
        # Plan 221 comments legitimately name the compatibility facade when
        # documenting the blocker set; only code imports count. Strip all
        # `//` comment lines here (the Plan 215–220 gates above keep their
        # doc-only exclusion).
        return "\n".join(
            line
            for line in text.splitlines()
            if not line.lstrip().startswith("//")
        )

    # 1. Frontend manifests must name the leaf crates directly (not only
    #    transitively through the compatibility core).
    import tomllib

    for rel, required in (
        ("crates/eggserve-bin/Cargo.toml", {"eggserve-primitives", "eggserve-server", "eggserve-static", "eggnet-tls"}),
        ("crates/eggserve-python/Cargo.toml", {"eggserve-primitives", "eggserve-server", "eggserve-static", "eggnet-tls", "eggserve-bin"}),
    ):
        data = tomllib.loads((repo / rel).read_text())
        deps = set(data.get("dependencies", {}))
        missing = required - deps
        if missing:
            print(f"{rel} must name leaf crates directly: missing {sorted(missing)} (Plan 221)", file=sys.stderr)
            return 1

    # 2. Binary neutral paths must use the leaf authorities.
    bin_lib = read(repo / "crates" / "eggserve-bin" / "src" / "lib.rs")
    bin_code = code_lines(bin_lib)
    for marker in (
        "use eggserve_server::ops::",
        "eggserve_primitives::policy::ErrorRepresentationPolicy",
    ):
        if marker not in bin_lib:
            print(f"eggserve-bin/src/lib.rs must use leaf `{marker}` (Plan 221)", file=sys.stderr)
            return 1
    bin_tls = read(repo / "crates" / "eggserve-bin" / "src" / "tls.rs")
    if "pub use eggnet_tls::" not in bin_tls:
        print("eggserve-bin/src/tls.rs must re-export the neutral `eggnet_tls` substrate directly (Plan 221)", file=sys.stderr)
        return 1
    if "use eggserve_core::ops::" in bin_code or "use eggserve_core::tls::" in bin_code:
        print("eggserve-bin/src/lib.rs must not import ops/TLS through the compatibility core (Plan 221)", file=sys.stderr)
        return 1
    bin_args = read(repo / "crates" / "eggserve-bin" / "src" / "args.rs")
    if "eggserve_primitives::policy::" not in bin_args:
        print("eggserve-bin/src/args.rs must name `eggserve_primitives::policy` directly (Plan 221)", file=sys.stderr)
        return 1
    # Binary unit tests prove the direct H1 static path (leaf server + leaf static).
    if "use eggserve_server::" not in bin_lib or "use eggserve_static::StaticService" not in bin_lib:
        print("eggserve-bin/src/lib.rs tests must exercise the direct leaf H1 static path (Plan 221)", file=sys.stderr)
        return 1

    # 3. Python bridge modules must be core-free except the documented
    #    extended-orchestration blockers (runtime lifecycle/serve_config,
    #    static config/listing budgets). Neutral policy/primitives/ops/
    #    service/response-policy/TLS paths must use the leaf.
    bridge_dir = repo / "crates" / "eggserve-python" / "src" / "server"
    blocker_files = {"runtime.rs", "static_responder.rs", "lifecycle.rs"}
    for path in sorted(bridge_dir.glob("*.rs")):
        code = code_lines(read(path))
        # Neutral paths must not route through the core facade.
        for second in (
            "eggserve_core::policy::",
            "eggserve_core::primitives::body::",
            "eggserve_core::primitives::canonical::",
            "eggserve_core::primitives::header_block::",
            "eggserve_core::primitives::http::",
            "eggserve_core::primitives::request",
            "eggserve_core::ops::",
            "eggserve_core::server::service::",
            "eggserve_core::server::response_policy::",
            "eggserve_core::server::errors::",
            "eggserve_core::tls::",
        ):
            if second in code:
                print(f"eggserve-python server/{path.name} routes neutral paths through `{second}` (Plan 221: use the leaf)", file=sys.stderr)
                return 1
        # Outside the documented blocker files, no core code import remains.
        if path.name not in blocker_files and "eggserve_core::" in code:
            print(f"eggserve-python server/{path.name} keeps a compatibility-core import outside the documented blocker set (Plan 221)", file=sys.stderr)
            return 1
    # Top-level Python lib keeps one static-budget use (listing entries);
    # everything else static/neutral must be leaf.
    py_lib = code_lines(read(repo / "crates" / "eggserve-python" / "src" / "lib.rs"))
    for second in ("eggserve_core::primitives::", "eggserve_core::policy::"):
        if second in py_lib:
            print(f"eggserve-python/src/lib.rs routes neutral paths through `{second}` (Plan 221: use the leaf)", file=sys.stderr)
            return 1

    # 4. Python -> bin extension CLI remains (confirmed used; Plan 221 §3 keeps it).
    if "eggserve_bin::run_cli" not in read(repo / "crates" / "eggserve-python" / "src" / "lib.rs"):
        print("eggserve-python must retain the extension-backed CLI via `eggserve_bin::run_cli` (Plan 221 §3: confirmed used)", file=sys.stderr)
        return 1

    # 5. Shared runtime validation has one Rust authority: the Python bridge
    #    must project through `SharedRuntimeValues` instead of an independent table.
    runtime_rs = read(repo / "crates" / "eggserve-python" / "src" / "server" / "runtime.rs")
    if "SharedRuntimeValues" not in runtime_rs or "shared.validate()" not in runtime_rs:
        print("eggserve-python runtime must validate through canonical `SharedRuntimeValues` (Plan 221 §4)", file=sys.stderr)
        return 1

    return 0


# Plan 225 §1 classified production-module inventory for `eggserve-core`.
# Module-level so `--self-test` can build fixture trees from the same
# source of truth the gate enforces.
PLAN225_CORE_MODULE_INVENTORY = frozenset(
    {
        "config.rs",
        "lib.rs",
        "limits.rs",
        "ops/mod.rs",
        "policy.rs",
        "primitives/authority.rs",
        "primitives/body.rs",
        "primitives/canonical.rs",
        "primitives/connection_info.rs",
        "primitives/header_block.rs",
        "primitives/http.rs",
        "primitives/incomplete_body_policy.rs",
        "primitives/interim.rs",
        "primitives/interop.rs",
        "primitives/method.rs",
        "primitives/mod.rs",
        "primitives/planner.rs",
        "primitives/proxy.rs",
        "primitives/request.rs",
        "primitives/request_body.rs",
        "primitives/request_body_error.rs",
        "primitives/request_body_policy.rs",
        "primitives/request_context.rs",
        "primitives/request_head.rs",
        "primitives/request_lifecycle.rs",
        "primitives/request_target.rs",
        "primitives/response.rs",
        "primitives/response_stream.rs",
        "primitives/secure_root.rs",
        "primitives/trailers.rs",
        "primitives/tunnel.rs",
        "primitives/version.rs",
        "response.rs",
        "runtime_limits.rs",
        "server/accept.rs",
        "server/config.rs",
        "server/config/http1.rs",
        "server/config/http2.rs",
        "server/config/http3.rs",
        "server/config/runtime.rs",
        "server/config/tls.rs",
        "server/connection/activity.rs",
        "server/connection/context.rs",
        "server/connection/deferred_body.rs",
        "server/connection/driver.rs",
        "server/connection/lifecycle.rs",
        "server/connection/mod.rs",
        "server/connection/pipeline.rs",
        "server/connection/request.rs",
        "server/connection/response.rs",
        "server/connection/transport.rs",
        "server/errors.rs",
        "server/handle.rs",
        "server/http3.rs",
        "server/lifecycle.rs",
        "server/listener.rs",
        "server/mod.rs",
        "server/proxy.rs",
        "server/response_policy.rs",
        "server/runtime.rs",
        "server/service.rs",
        "server/static_service.rs",
        "server/tower.rs",
        "tls.rs",
    }
)


def check_plan225_facade() -> int:
    """Enforce Plan 225 compatibility-facade closure.

    Structural (not line-count) rules: `eggserve-core` is a classified
    facade/adapter layer. No second canonical/parser/resolver/state-machine
    implementation may return, no leftover implementation dependencies may
    remain, every production module must be in the classified inventory
    (new files fail until explicitly classified per Plan 225 §1), and
    every `primitives/*.rs` compatibility file must facade the direct
    authority except the documented adapters.
    """
    import tomllib

    repo = Path(__file__).resolve().parent.parent
    core_src = repo / "crates" / "eggserve-core" / "src"

    # 1. No second canonical implementation: the orphaned
    #    `primitives/canonical/` duplicate (deleted by this plan) must not
    #    return as a directory next to the `canonical.rs` facade.
    if (core_src / "primitives" / "canonical").exists():
        print(
            "eggserve-core retains primitives/canonical/: the canonical "
            "response vocabulary lives once in eggserve-primitives with "
            "Hyper conversion once in eggserve-server (Plan 225: facade only)",
            file=sys.stderr,
        )
        return 1

    # 2. No leftover implementation dependencies: the MIME perfect-hash map
    #    lives once in `eggserve-static`; core must not keep `phf`.
    core_manifest = tomllib.loads(
        (repo / "crates" / "eggserve-core" / "Cargo.toml").read_text()
    )
    for section in ("dependencies", "dev-dependencies"):
        if "phf" in core_manifest.get(section, {}):
            print(
                "eggserve-core keeps a `phf` dependency: MIME selection lives "
                "once in eggserve-static (Plan 225: remove the leftover)",
                file=sys.stderr,
            )
            return 1
    for target in core_manifest.get("target", {}).values():
        if "phf" in target.get("dependencies", {}):
            print(
                "eggserve-core keeps a target-gated `phf` dependency: MIME "
                "selection lives once in eggserve-static (Plan 225)",
                file=sys.stderr,
            )
            return 1

    # 3. Classified inventory (Plan 225 §1): facades, adapters, documented
    #    orchestration, and the H2/listener/proxy/TLS transport glue. A new
    #    production module fails here until it is classified and added with
    #    an owner (rollback rule: do not silently re-expand core).
    expected = PLAN225_CORE_MODULE_INVENTORY
    actual = {
        str(path.relative_to(core_src)) for path in core_src.rglob("*.rs")
    }
    new, gone = _inventory_diff(actual, expected)
    if new or gone:
        detail = []
        if new:
            detail.append(f"unclassified new modules: {new}")
        if gone:
            detail.append(f"inventory modules missing: {gone}")
        print(
            "eggserve-core module inventory changed "
            f"({'; '.join(detail)}). "
            "Classify every production module per Plan 225 §1 "
            "(facade / adapter / orchestration / blocker) before extending "
            "the compatibility core.",
            file=sys.stderr,
        )
        return 1

    # 4. Facade discipline: every `primitives/*.rs` compatibility file must
    #    re-export the direct authority (`pub use eggserve_...`). The only
    #    documented exceptions are the Plan 200 `http-interop` adapters
    #    (`interop.rs`, loss-aware conversions over canonical types, never
    #    in default builds) — `mod.rs` only declares modules and re-exports.
    for path in sorted((core_src / "primitives").glob("*.rs")):
        if path.name in {"interop.rs", "mod.rs"}:
            continue
        if "pub use eggserve_" not in path.read_text():
            print(
                f"eggserve-core/primitives/{path.name} is not a facade: "
                "compatibility files must re-export the direct authority "
                "(`pub use eggserve_...`, Plan 225)",
                file=sys.stderr,
            )
            return 1

    return 0


def run_self_tests() -> int:
    """Fixture-driven mutation tests for the brittle rules (Plan 255 Track I).

    Operates on synthetic temp trees (plus pure-predicate unit checks) so no
    intentionally broken repository source is ever committed. The stable
    entrypoint is unchanged: `python3 scripts/check-crate-topology.py`.
    Run these with `python3 scripts/check-crate-topology.py --self-test`.
    """
    import ast
    import contextlib
    import tempfile

    failures: list[str] = []
    passed = 0

    def check(name: str, cond: bool) -> None:
        nonlocal passed
        print(f"  {'ok' if cond else 'FAIL'} {name}")
        if cond:
            passed += 1
        else:
            failures.append(name)

    # 1. Rule inventory: every check_plan* rule stays wired into main(), so
    #    a refactor cannot silently drop a rejection family.
    src = Path(__file__).read_text()
    tree = ast.parse(src)
    defined = {
        node.name
        for node in ast.walk(tree)
        if isinstance(node, ast.FunctionDef) and node.name.startswith("check_plan")
    }
    main_fn = next(
        node
        for node in ast.walk(tree)
        if isinstance(node, ast.FunctionDef) and node.name == "main"
    )
    called = {
        node.func.id
        for node in ast.walk(main_fn)
        if isinstance(node, ast.Call) and isinstance(node.func, ast.Name)
    }
    check("rule-inventory-wired", bool(defined) and defined <= called)

    # 2. Pure predicates fail closed on synthetic inputs.
    check(
        "forbidden-deps-pass",
        check_forbidden_deps("t", {"a"}, {"b"}, "why") == 0,
    )
    check(
        "forbidden-deps-fail",
        check_forbidden_deps("t", {"a", "hyper"}, {"hyper"}, "why") == 1,
    )
    check(
        "inventory-diff",
        _inventory_diff({"a", "b"}, {"b", "c"}) == (["a"], ["c"]),
    )

    saved_file = globals().get("__file__", __file__)

    @contextlib.contextmanager
    def fake_repo():
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "scripts").mkdir()
            globals()["__file__"] = str(root / "scripts" / "check-crate-topology.py")
            try:
                yield root
            finally:
                globals()["__file__"] = saved_file

    def write(root: Path, rel: str, content: str = "") -> None:
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)

    # 3. Plan 249: forbidden core H1 machinery and detached forwarders.
    good_driver = (
        "pub(crate) enum WireProtocol { Http1, Http2 }\n"
        "pub(crate) fn classify_cleartext() {}\n"
        "pub(crate) async fn serve_h2_with_token() {}\n"
    )
    good_facade = (
        "pub async fn serve_http1_connection() {}\n"
        "pub async fn serve_http1_connection_with_id() {}\n"
    )
    good_accept = "fn accept_loop() {\n    run_with_connection_shutdown();\n}\n"

    def make_249(root: Path) -> None:
        base = "crates/eggserve-core/src/server/connection"
        write(root, f"{base}/driver.rs", good_driver)
        write(root, f"{base}/mod.rs", good_facade)
        write(root, "crates/eggserve-core/src/server/accept.rs", good_accept)

    with fake_repo() as root:
        make_249(root)
        check("plan249-good", check_plan249_h1_authority() == 0)
        base = "crates/eggserve-core/src/server/connection"
        write(root, f"{base}/driver.rs", good_driver + "fn hyper_builder() {}\n")
        check("plan249-h1-builder", check_plan249_h1_authority() == 1)
        write(root, f"{base}/driver.rs", good_driver + "async fn serve_hyper_with_token() {}\n")
        check("plan249-h1-driver-fn", check_plan249_h1_authority() == 1)
        write(root, f"{base}/driver.rs", good_driver)
        write(
            root,
            "crates/eggserve-core/src/server/accept.rs",
            good_accept + "    tokio::spawn(async {});\n",
        )
        check("plan249-detached-forwarder", check_plan249_h1_authority() == 1)
        write(root, "crates/eggserve-core/src/server/accept.rs", good_accept)
        write(root, f"{base}/mod.rs", "pub async fn serve_http1_connection() {}\n")
        check("plan249-facade-loss", check_plan249_h1_authority() == 1)

    # 4. Plan 253: parallel helpers stay crate-private, pairs exist, H2 gated.
    pairs = (
        "activity",
        "deferred_body",
        "driver",
        "lifecycle",
        "pipeline",
        "request",
        "response",
        "transport",
    )
    gated = (
        "activity",
        "deferred_body",
        "lifecycle",
        "pipeline",
        "request",
        "response",
        "transport",
    )

    def make_253(root: Path) -> None:
        for name in pairs:
            write(root, f"crates/eggserve-server/src/connection/{name}.rs", f"// direct {name}\n")
            write(root, f"crates/eggserve-core/src/server/connection/{name}.rs", f"// core {name}\n")
        gates = "".join(
            f'#[cfg(feature = "http2")]\npub(crate) mod {name};\n' for name in gated
        )
        write(root, "crates/eggserve-core/src/server/connection/mod.rs", gates)

    with fake_repo() as root:
        make_253(root)
        check("plan253-good", check_plan253_overlap() == 0)
        write(
            root,
            "crates/eggserve-core/src/server/connection/transport.rs",
            "pub fn probe() {}\n",
        )
        check("plan253-core-public", check_plan253_overlap() == 1)
        make_253(root)
        write(
            root,
            "crates/eggserve-server/src/connection/request.rs",
            "pub fn extra() {}\n",
        )
        check("plan253-direct-public", check_plan253_overlap() == 1)
        make_253(root)
        (root / "crates/eggserve-core/src/server/connection/pipeline.rs").unlink()
        check("plan253-pair-loss", check_plan253_overlap() == 1)
        make_253(root)
        mod_rs = root / "crates/eggserve-core/src/server/connection/mod.rs"
        mod_rs.write_text(mod_rs.read_text().replace('pub(crate) mod transport;\n', 'pub(crate) mod extra;\n'))
        check("plan253-gate-loss", check_plan253_overlap() == 1)

    # 5. Plan 219: second confinement authority fails closed.
    with fake_repo() as root:
        write(root, "crates/eggserve-core/src/lib.rs", "pub mod primitives;\n")
        write(root, "crates/eggserve-core/src/fs/mod.rs", "pub struct Second;\n")
        check("plan219-second-fs", check_plan219_confinement() == 1)
        (root / "crates/eggserve-core/src/fs/mod.rs").unlink()
        (root / "crates/eggserve-core/src/fs").rmdir()
        write(
            root,
            "crates/eggserve-core/src/primitives/secure_root.rs",
            "pub struct SecureRoot;\n",
        )
        check("plan219-second-struct", check_plan219_confinement() == 1)

    # 6. Plan 225: inventory built from the same constant the gate enforces.
    with fake_repo() as root:
        core_src = root / "crates/eggserve-core/src"
        for rel in PLAN225_CORE_MODULE_INVENTORY:
            content = ""
            if rel.startswith("primitives/") and rel not in {"primitives/interop.rs", "primitives/mod.rs"}:
                content = "pub use eggserve_x::Y;\n"
            if rel == "primitives/mod.rs":
                content = "pub mod authority;\n"
            write(root, f"crates/eggserve-core/src/{rel}", content)
        (core_src / "primitives" / "canonical").mkdir(exist_ok=True)
        write(root, "crates/eggserve-core/Cargo.toml", '[package]\nname = "x"\n[dependencies]\n')
        # The canonical/ directory resurrection must fail even when the
        # inventory is otherwise exact.
        check("plan225-canonical-dir", check_plan225_facade() == 1)
        (core_src / "primitives" / "canonical").rmdir()
        check("plan225-good", check_plan225_facade() == 0)
        write(root, "crates/eggserve-core/src/server/sneaky.rs", "pub fn x() {}\n")
        check("plan225-new-module", check_plan225_facade() == 1)

    # 7. Live-tree positives: the real repository still passes.
    globals()["__file__"] = saved_file
    check("live-plan249", check_plan249_h1_authority() == 0)
    check("live-plan253", check_plan253_overlap() == 0)
    check("live-plan219", check_plan219_confinement() == 0)
    check("live-plan225", check_plan225_facade() == 0)
    globals()["__file__"] = saved_file

    if failures:
        print(f"self-test failures: {failures}", file=sys.stderr)
        return 1
    print(f"self-test: {passed} checks passed")
    return 0


if __name__ == "__main__":
    if "--self-test" in sys.argv:
        raise SystemExit(run_self_tests())
    raise SystemExit(main())

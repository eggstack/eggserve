#!/usr/bin/env python3
"""Enforce the Plan 211–214 Cargo dependency topology.

This is intentionally a small metadata check rather than a line-count or
source-layout rule. Cargo's resolved direct package graph is the contract:
canonical primitives are a leaf, the generic server does not pull static
serving, and static serving consumes the two lower layers.
"""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys


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
    if primitives & forbidden:
        print(f"eggserve-primitives leaks forbidden dependencies: {sorted(primitives & forbidden)}", file=sys.stderr)
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
    if "eggserve-core" in server or "eggserve-static" in server:
        print("eggserve-server must not depend on the compatibility core or static layer", file=sys.stderr)
        return 1
    if "eggserve-primitives" not in server:
        print("eggserve-server must depend on eggserve-primitives", file=sys.stderr)
        return 1
    server_h3 = production("eggserve-server") & {"h3", "h3-quinn", "quinn"}
    if server_h3:
        print(
            "eggserve-server must not directly depend on the H3/QUIC stack: "
            f"{sorted(server_h3)}",
            file=sys.stderr,
        )
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
    if {"eggserve-server", "eggserve-static", "eggserve-primitives"} & h3:
        print(
            "eggserve-h3 must remain a transport dependency boundary, not a "
            "server/static/primitives implementation dependency",
            file=sys.stderr,
        )
        return 1

    if "eggnet-tls" not in direct("eggserve-core"):
        print("eggserve-core must consume the neutral eggnet-tls crate", file=sys.stderr)
        return 1
    core = direct("eggserve-core")
    core_h3 = {"h3", "h3-quinn", "quinn"} & core
    if core_h3:
        print(
            "eggserve-core must not directly depend on the H3/QUIC stack: "
            f"{sorted(core_h3)}",
            file=sys.stderr,
        )
        return 1

    if check_plan215_parity() != 0:
        return 1

    if check_plan216_tunnel() != 0:
        return 1

    print(
        "Plan 211–216 topology: primitives leaf; neutral TLS; "
        "server transport-only; static specializes both; H3/QUIC isolated; "
        "direct H1 runtime owns ops/errors/policy/authority/service/driver; "
        "direct tunnel authority with neutral vocabulary"
    )
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

    # 4. Behavioral shape parity between the direct service contract and the
    #    compatibility one. Full trait identity waits on Request-type
    #    unification (tunnel slot, Plan 216); until then both definitions
    #    must carry the mature categories so neither silently diverges.
    shape_markers = [
        "Panic",
        "fn is_panic",
        "fn is_timeout",
        "fn message",
        "service_fn_with_policy",
        "service_fn_head",
    ]
    server_service = read(server_src / "service.rs")
    core_service = read(core_src / "server" / "service.rs")
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


if __name__ == "__main__":
    raise SystemExit(main())

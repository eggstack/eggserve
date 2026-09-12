#!/usr/bin/env python3
"""Validate the conformance matrix's schema and value domains."""

from pathlib import Path
import tomllib


REQUIRED = {
    "resource",
    "method",
    "conditional",
    "range",
    "file_state",
    "http_version",
    "connection",
    "expected_status",
    "body_forbidden",
    "connection_reuse",
}
METHODS = {"GET", "HEAD", "POST"}
CONNECTIONS = {"close", "keep_alive"}

# Plan 207 cross-protocol inventory vocabulary.
APP_REQUIRED = {"id", "category", "description", "transports", "consumers", "routine", "evidence"}
APP_TRANSPORTS = {
    "h1_tcp",
    "h1_tls",
    "h1_prebound",
    "h1_unix",
    "h2_prior",
    "h2_tls",
    "h2_prebound",
    "h3_quic",
    "caller_owned",
}
APP_CONSUMERS = {"native", "http_interop", "tower", "async_python", "asgi_fixture"}
APP_CATEGORIES = {
    "request_metadata",
    "request_body",
    "response",
    "lifecycle",
    "multiplexing",
    "tunnel",
    "security",
    "resources",
    "consumer",
    "interop",
    "python_loop",
    "performance",
    "ci_release",
}

H3_REQUIRED = {
    "protocol",
    "adversarial",
    "lifecycle",
    "interoperability",
    "configuration",
    "dependencies",
    "promotion",
}
H3_CLASSIFICATIONS = {"deterministic", "manual", "blocked"}


def validate_app_server_conformance(root: Path) -> None:
    inventory_path = root / "conformance" / "app_server_conformance.toml"
    with inventory_path.open("rb") as inventory_file:
        document = tomllib.load(inventory_file)
    scenarios = document.get("scenario")
    if not isinstance(scenarios, list) or not scenarios:
        raise SystemExit("app-server conformance inventory has no [[scenario]] entries")
    seen_ids: set[str] = set()
    for index, entry in enumerate(scenarios, start=1):
        missing = APP_REQUIRED - entry.keys()
        if missing:
            raise SystemExit(f"app-server scenario {index} is missing: {sorted(missing)}")
        scenario_id = entry["id"]
        if scenario_id in seen_ids:
            raise SystemExit(f"duplicate app-server scenario id: {scenario_id}")
        seen_ids.add(scenario_id)
        if entry["category"] not in APP_CATEGORIES:
            raise SystemExit(f"app-server scenario {scenario_id} has invalid category")
        if not isinstance(entry["description"], str) or not entry["description"].strip():
            raise SystemExit(f"app-server scenario {scenario_id} needs a description")
        for transport in entry["transports"]:
            if transport not in APP_TRANSPORTS:
                raise SystemExit(
                    f"app-server scenario {scenario_id} has invalid transport {transport}"
                )
        for consumer in entry["consumers"]:
            if consumer not in APP_CONSUMERS:
                raise SystemExit(
                    f"app-server scenario {scenario_id} has invalid consumer {consumer}"
                )
        if not isinstance(entry["routine"], bool):
            raise SystemExit(f"app-server scenario {scenario_id} needs a boolean routine flag")
        if not isinstance(entry["evidence"], str) or not entry["evidence"].strip():
            raise SystemExit(f"app-server scenario {scenario_id} needs evidence")
    exercised = {e["category"] for e in scenarios}
    missing_categories = APP_CATEGORIES - exercised
    if missing_categories:
        raise SystemExit(
            f"app-server categories not exercised: {sorted(missing_categories)}"
        )
    routine = [e for e in scenarios if e["routine"]]
    if not routine:
        raise SystemExit("app-server inventory has no routine CI scenarios")
    print(f"validated {len(scenarios)} app-server scenarios ({len(routine)} routine)")


def validate_h3_qualification(root: Path) -> None:
    matrix_path = root / "conformance" / "http3_qualification.toml"
    with matrix_path.open("rb") as matrix_file:
        document = tomllib.load(matrix_file)
    metadata = document.get("metadata", {})
    if metadata.get("plan") != 213 or metadata.get("status") != "experimental":
        raise SystemExit("HTTP/3 qualification metadata must identify Plan 213 as experimental")
    scenarios = document.get("scenario")
    if not isinstance(scenarios, list) or not scenarios:
        raise SystemExit("HTTP/3 qualification inventory has no [[scenario]] entries")
    seen_ids: set[str] = set()
    for index, entry in enumerate(scenarios, start=1):
        required = {"id", "category", "classification", "routine", "evidence"}
        missing = required - entry.keys()
        if missing:
            raise SystemExit(f"HTTP/3 scenario {index} is missing: {sorted(missing)}")
        if entry["id"] in seen_ids:
            raise SystemExit(f"duplicate HTTP/3 scenario id: {entry['id']}")
        seen_ids.add(entry["id"])
        if entry["classification"] not in H3_CLASSIFICATIONS:
            raise SystemExit(f"HTTP/3 scenario {entry['id']} has invalid classification")
        if not isinstance(entry["routine"], bool):
            raise SystemExit(f"HTTP/3 scenario {entry['id']} needs a boolean routine flag")
        if not isinstance(entry["evidence"], str) or not entry["evidence"].strip():
            raise SystemExit(f"HTTP/3 scenario {entry['id']} needs evidence")
    missing_categories = H3_REQUIRED - {entry["category"] for entry in scenarios}
    if missing_categories:
        raise SystemExit(f"HTTP/3 categories not exercised: {sorted(missing_categories)}")
    if not any(entry["routine"] for entry in scenarios):
        raise SystemExit("HTTP/3 qualification inventory has no routine scenarios")
    print(f"validated {len(scenarios)} Plan 213 HTTP/3 scenarios")


def main() -> None:
    root = Path(__file__).resolve().parents[1]
    matrix_path = root / "conformance" / "conformance_matrix.toml"
    with matrix_path.open("rb") as matrix_file:
        document = tomllib.load(matrix_file)

    entries = document.get("matrix")
    if not isinstance(entries, list) or not entries:
        raise SystemExit("conformance matrix has no [[matrix]] entries")

    for index, entry in enumerate(entries, start=1):
        missing = REQUIRED - entry.keys()
        if missing:
            raise SystemExit(f"matrix entry {index} is missing: {sorted(missing)}")
        if entry["method"] not in METHODS:
            raise SystemExit(f"matrix entry {index} has invalid method")
        if entry["connection"] not in CONNECTIONS:
            raise SystemExit(f"matrix entry {index} has invalid connection policy")
        if not isinstance(entry["expected_status"], int) or not 100 <= entry["expected_status"] <= 599:
            raise SystemExit(f"matrix entry {index} has invalid expected status")
        if not isinstance(entry["body_forbidden"], bool):
            raise SystemExit(f"matrix entry {index} has invalid body_forbidden value")
        if not isinstance(entry["connection_reuse"], bool):
            raise SystemExit(f"matrix entry {index} has invalid connection_reuse value")

    # Coverage check: every declared resource must appear at least once.
    declared_resources = {
        "direct_file",
        "directory_index",
        "root_index",
        "directory_listing",
        "missing",
        "denied",
    }
    exercised = {e["resource"] for e in entries}
    missing = declared_resources - exercised
    if missing:
        raise SystemExit(
            f"declared resources not exercised in any matrix entry: {sorted(missing)}"
        )

    print(f"validated {len(entries)} conformance matrix entries")
    validate_app_server_conformance(root)
    validate_h3_qualification(root)


if __name__ == "__main__":
    main()

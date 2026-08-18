#!/usr/bin/env python3
"""Release preflight: verify version sync and wheel contract before release builds."""

from __future__ import annotations

import configparser
import re
import sys
import tomllib
import zipfile
from pathlib import Path


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    errors: list[str] = []

    # --- B1: Package identity ---
    pyproject_path = repo_root / "crates" / "eggserve-python" / "pyproject.toml"
    with open(pyproject_path, "rb") as f:
        pyproject = tomllib.load(f)

    project_name = pyproject.get("project", {}).get("name")
    if project_name != "eggserve":
        errors.append(f"pyproject project name != eggserve: {project_name!r}")

    maturin_module = pyproject.get("tool", {}).get("maturin", {}).get("module-name")
    if maturin_module != "eggserve._native":
        errors.append(f"maturin module-name != eggserve._native: {maturin_module!r}")

    scripts = pyproject.get("project", {}).get("scripts", {})
    if scripts.get("eggserve") != "eggserve._bin:main":
        errors.append(f"console script entry point incorrect: {scripts!r}")

    # --- B2: Version sync ---
    # Workspace version from root Cargo.toml
    root_cargo = repo_root / "Cargo.toml"
    with open(root_cargo, "rb") as f:
        workspace = tomllib.load(f)
    workspace_version = workspace.get("workspace", {}).get("package", {}).get("version")

    # Python crate Cargo.toml version
    py_cargo = repo_root / "crates" / "eggserve-python" / "Cargo.toml"
    with open(py_cargo, "rb") as f:
        py_cargo_data = tomllib.load(f)
    py_cargo_version = py_cargo_data.get("package", {}).get("version")

    # pyproject.toml version
    pyproject_version = pyproject.get("project", {}).get("version")

    # __init__.py version (read as fallback, should derive from metadata now)
    init_path = repo_root / "crates" / "eggserve-python" / "python" / "eggserve" / "__init__.py"
    init_text = init_path.read_text()
    init_version_match = re.search(r'__version__\s*=\s*["\']([^"\']+)["\']', init_text)

    versions = {
        "workspace Cargo.toml": workspace_version,
        "python crate Cargo.toml": py_cargo_version,
        "pyproject.toml": pyproject_version,
    }

    # All three must agree
    unique_versions = set(versions.values())
    if len(unique_versions) > 1:
        errors.append(
            f"version mismatch across packaging surfaces: {versions}"
        )

    expected_version = workspace_version
    if not expected_version:
        errors.append("workspace version is missing")
    else:
        for label, v in versions.items():
            if v != expected_version:
                errors.append(f"{label} version {v!r} != expected {expected_version!r}")

    # --- B3: Python compatibility contract ---
    requires_python = pyproject.get("project", {}).get("requires-python", "")
    if "3.11" not in requires_python:
        errors.append(f"requires-python does not include 3.11: {requires_python!r}")

    py_cargo_features = []
    for dep in py_cargo_data.get("dependencies", {}).values():
        if isinstance(dep, dict) and dep.get("package") == "pyo3":
            py_cargo_features = dep.get("features", [])
            break
    # Also check the top-level [dependencies] table format
    if not py_cargo_features:
        deps = py_cargo_data.get("dependencies", {})
        if "pyo3" in deps and isinstance(deps["pyo3"], dict):
            py_cargo_features = deps["pyo3"].get("features", [])

    if "abi3-py311" not in py_cargo_features:
        errors.append(f"pyo3 features missing abi3-py311: {py_cargo_features}")

    maturin_bindings = pyproject.get("tool", {}).get("maturin", {}).get("bindings")
    if maturin_bindings != "pyo3":
        errors.append(f"maturin bindings != pyo3: {maturin_bindings!r}")

    # --- B4: Wheel architecture contract (if wheel exists in dist/) ---
    dist_dirs = [
        repo_root / "dist",
        repo_root / "target" / "wheels",
    ]
    for dist_dir in dist_dirs:
        if not dist_dir.is_dir():
            continue
        wheels = list(dist_dir.glob("*.whl"))
        for whl in wheels:
            with zipfile.ZipFile(whl) as zf:
                # Check no bundled executable
                forbidden = [
                    n for n in zf.namelist()
                    if n.startswith("eggserve/bin/eggserve")
                ]
                if forbidden:
                    errors.append(f"{whl.name} contains bundled executable: {forbidden}")

                # Check metadata version
                for name in zf.namelist():
                    if name.endswith(".dist-info/METADATA"):
                        metadata_text = zf.read(name).decode()
                        for line in metadata_text.splitlines():
                            if line.startswith("Version:"):
                                wheel_version = line.split(":", 1)[1].strip()
                                if wheel_version != expected_version:
                                    errors.append(
                                        f"{whl.name} metadata version {wheel_version!r} != {expected_version!r}"
                                    )
                                break
                    if name.endswith(".dist-info/WHEEL"):
                        wheel_text = zf.read(name).decode()
                        if "abi3" not in wheel_text and "cp311" not in wheel_text:
                            errors.append(f"{whl.name} WHEEL metadata missing abi3 tag")

    if errors:
        print("Release preflight FAILED:", file=sys.stderr)
        for e in errors:
            print(f"  - {e}", file=sys.stderr)
        return 1

    print(f"Release preflight passed (version: {expected_version})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

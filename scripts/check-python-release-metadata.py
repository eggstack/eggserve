#!/usr/bin/env python3
"""Release preflight: verify version sync and wheel contract before release builds."""

from __future__ import annotations

import configparser
import re
import sys
import tomllib
import zipfile
from pathlib import Path

# Plan 327: the free-threading classifier is the advertised support tier. The
# gate accepts exactly one classifier, at the tier this constant declares, and
# requires it exactly when the cp314t family is a required matrix family. A
# tier change flips pyproject, the docs, and this constant in one change.
FREE_THREADING_CLASSIFIER_PREFIX = (
    "Programming Language :: Python :: Free Threading :: "
)
FREE_THREADING_TIERS = {
    1: "Unstable",
    2: "Beta",
    3: "Stable",
    4: "Resilient",
}
EXPECTED_FREE_THREADING_TIER = 3


def free_threading_classifier_errors(
    classifiers: list[str],
    ft_declared: bool,
    expected_tier: int = EXPECTED_FREE_THREADING_TIER,
) -> list[str]:
    """Return classifier-contract violations for the declared ABI families."""
    errors: list[str] = []
    found = [
        classifier
        for classifier in classifiers
        if classifier.startswith(FREE_THREADING_CLASSIFIER_PREFIX)
    ]
    expected = (
        f"{FREE_THREADING_CLASSIFIER_PREFIX}{expected_tier} - "
        f"{FREE_THREADING_TIERS[expected_tier]}"
    )
    if len(found) > 1:
        errors.append(
            "pyproject declares more than one free-threading classifier: "
            f"{sorted(found)}; declare exactly one"
        )
    if ft_declared and expected not in found:
        errors.append(
            "the free-threading classifier must be exactly "
            f"{expected!r} when the cp314t family is required "
            f"(got {sorted(found) if found else 'none'})"
        )
    if not ft_declared and found:
        errors.append(
            "the free-threading classifier must not be declared when the "
            f"cp314t family is absent (got {sorted(found)})"
        )
    return errors


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
        # Plan 226: this main line carries intentional pre-1.0 breaking
        # changes and must never ship as another 0.1.x patch.
        if expected_version.startswith("0.1."):
            errors.append(
                f"workspace version {expected_version!r} is a 0.1.x patch line; "
                "the current main line must release as 0.2.0 or later"
            )
        for label, v in versions.items():
            if v != expected_version:
                errors.append(f"{label} version {v!r} != expected {expected_version!r}")

    # --- B2b: Distribution profile sync (Plan 182 Track C) ---
    # The Maturin crate is intentionally excluded from the workspace so it
    # can build independently; its [profile.dist] must remain equivalent to
    # the workspace profile. Compare the required release-critical keys.
    for label, cargo_data in (
        ("workspace Cargo.toml", workspace),
        ("python crate Cargo.toml", py_cargo_data),
    ):
        profile = cargo_data.get("profile", {}).get("dist")
        if not isinstance(profile, dict):
            errors.append(f"{label} is missing [profile.dist]")
            continue
        for key, expected in (
            ("inherits", "release"),
            ("opt-level", "z"),
            ("lto", "fat"),
            ("codegen-units", 1),
            ("strip", "symbols"),
        ):
            if profile.get(key) != expected:
                errors.append(
                    f"{label} [profile.dist].{key} == {profile.get(key)!r}, "
                    f"expected {expected!r}"
                )

    workspace_dist = workspace.get("profile", {}).get("dist", {})
    py_dist = py_cargo_data.get("profile", {}).get("dist", {})
    if isinstance(workspace_dist, dict) and isinstance(py_dist, dict):
        if workspace_dist != py_dist:
            errors.append(
                f"[profile.dist] drift: workspace {workspace_dist!r} != "
                f"python crate {py_dist!r}"
            )

    # --- B3: Python compatibility contract ---
    # Plan 264 + Plan 326: GIL-enabled CPython support is 3.11-3.15 through
    # one cp311-abi3 artifact per platform. The free-threaded family is
    # cp314-cp314t on three targets only (Beta); no other free-threaded
    # implementation or platform may be advertised here.
    requires_python = pyproject.get("project", {}).get("requires-python", "")
    if "3.11" not in requires_python:
        errors.append(f"requires-python does not include 3.11: {requires_python!r}")
    # An artificial upper bound below 3.15 would contradict the 3.11-3.15
    # contract (e.g. "<3.15", "<=3.14", "==3.14.*").
    upper_bound = re.search(r"<\s*=?\s*3\.(\d+)", requires_python)
    if upper_bound and int(upper_bound.group(1)) <= 15 and "<" in requires_python:
        # Allow ">=3.11" (no upper bound). Reject explicit ceilings at/below 3.15.
        if re.search(r"<\s*=?\s*3\.1[0-5]", requires_python):
            errors.append(
                f"requires-python sets an upper bound excluding 3.15: {requires_python!r}"
            )

    classifiers = pyproject.get("project", {}).get("classifiers", [])
    for minor in ("3.11", "3.12", "3.13", "3.14", "3.15"):
        if f"Programming Language :: Python :: {minor}" not in classifiers:
            errors.append(
                f"pyproject classifiers omit Python {minor} "
                "(Plan 264: advertise 3.11-3.15)"
            )

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

    # --- B3b: Declared ABI families (Plan 264 Track D; Plan 326) ---
    # The packaging configuration, the release matrix, and the release
    # workflow must agree on exactly two families: the GIL cp311-abi3 family
    # on every platform and the Beta cp314-cp314t family on three targets.
    # The workflow takes both its build interpreter and its setup-python
    # version from the matrix; a hard-coded interpreter is a defect.
    build_requires = pyproject.get("build-system", {}).get("requires", [])
    if not any(
        isinstance(req, str) and req.strip().startswith("maturin>=1.14.1")
        for req in build_requires
    ):
        errors.append(
            "build-system maturin floor must be >=1.14.1 "
            f"(got {build_requires!r})"
        )

    matrix_path = repo_root / "release" / "wheel-matrix.toml"
    families: dict[str, set[str]] = {}
    ft_platforms: set[str] = set()
    matrix: dict = {}
    try:
        with open(matrix_path, "rb") as f:
            matrix = tomllib.load(f)
    except (FileNotFoundError, tomllib.TOMLDecodeError) as exc:
        errors.append(f"wheel matrix unreadable for ABI-family checks: {exc}")
    expected_tags = {
        "cp311": ("cp311", "abi3"),
        "cp314t": ("cp314", "cp314t"),
    }
    for target in matrix.get("target", []):
        if target.get("tier") != "required":
            continue
        family = target.get("abi_family")
        if not isinstance(family, str):
            errors.append(
                f"required target {target.get('id')!r} has no abi_family"
            )
            continue
        families.setdefault(family, set()).add(
            target.get("platform_tag", "")
        )
        if family == "cp314t":
            ft_platforms.add(target.get("platform_tag", ""))
        expected = expected_tags.get(family)
        if expected is None:
            errors.append(f"required target declares unknown family {family!r}")
            continue
        if (target.get("python_tag"), target.get("abi_tag")) != expected:
            errors.append(
                f"{target.get('id')}: tag triple "
                f"{target.get('python_tag')}-{target.get('abi_tag')} "
                f"!= {expected[0]}-{expected[1]}"
            )
        if not target.get("interpreter") or not target.get("setup_python"):
            errors.append(
                f"{target.get('id')}: missing interpreter/setup_python "
                "matrix metadata"
            )
    if set(families) != set(expected_tags):
        errors.append(
            "required ABI families must be exactly cp311 + cp314t "
            f"(got {sorted(families)})"
        )
    declared_ft_platforms = {
        "manylinux_2_17_x86_64", "macosx_11_0_arm64", "win_amd64",
    }
    if ft_platforms != declared_ft_platforms:
        errors.append(
            "free-threaded family platforms must be exactly the three "
            f"declared targets (got {sorted(ft_platforms)})"
        )
    ft_declared = "cp314t" in families
    errors.extend(
        free_threading_classifier_errors(classifiers, ft_declared)
    )

    release_yml = repo_root / ".github" / "workflows" / "release.yml"
    if release_yml.is_file():
        workflow_text = release_yml.read_text()
        if "--interpreter ${{ matrix.interpreter }}" not in workflow_text:
            errors.append(
                "release workflow must take --interpreter from "
                "matrix.interpreter"
            )
        if re.search(r"--interpreter\s+python3\.\d+", workflow_text):
            errors.append(
                "release workflow hard-codes a python3.x interpreter; the "
                "wheel matrix is its only authority"
            )
        if "python-version: ${{ matrix.setup_python }}" not in workflow_text:
            errors.append(
                "release workflow must take actions/setup-python "
                "python-version from matrix.setup_python"
            )
        if "cp312-cp312" in workflow_text or "cp315-cp315" in workflow_text:
            errors.append(
                "release workflow appears to build per-minor cpNN wheels; "
                "the GIL family requires one cp311-abi3 artifact per platform"
            )

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
                        if not any(
                            token in wheel_text
                            for token in ("abi3", "cp311", "cp314t", "cp314")
                        ):
                            errors.append(
                                f"{whl.name} WHEEL metadata missing a declared "
                                "ABI tag"
                            )

    if errors:
        print("Release preflight FAILED:", file=sys.stderr)
        for e in errors:
            print(f"  - {e}", file=sys.stderr)
        return 1

    print(f"Release preflight passed (version: {expected_version})")
    return 0


def self_test() -> int:
    """Exercise the classifier contract in both tier configurations."""
    beta = "Programming Language :: Python :: Free Threading :: 2 - Beta"
    stable = "Programming Language :: Python :: Free Threading :: 3 - Stable"
    unstable = (
        "Programming Language :: Python :: Free Threading :: 1 - Unstable"
    )
    results = []

    def check(name: str, condition: bool, detail: str = "") -> None:
        results.append((name, bool(condition), detail))

    check(
        "beta-expected-beta-present",
        free_threading_classifier_errors([beta], True, 2) == [],
    )
    check(
        "stable-expected-stable-present",
        free_threading_classifier_errors([stable], True, 3) == [],
    )
    check(
        "beta-expected-stable-present",
        len(free_threading_classifier_errors([stable], True, 2)) == 1,
    )
    check(
        "stable-expected-beta-present",
        len(free_threading_classifier_errors([beta], True, 3)) == 1,
    )
    check(
        "declared-family-missing-classifier",
        len(free_threading_classifier_errors([], True, 2)) == 1,
    )
    check(
        "wrong-tier-unstable",
        len(free_threading_classifier_errors([unstable], True, 2)) == 1,
    )
    check(
        "two-tiers-rejected",
        len(free_threading_classifier_errors([beta, stable], True, 2)) == 1,
    )
    check(
        "no-family-no-classifier",
        free_threading_classifier_errors([], False, 2) == [],
    )
    check(
        "no-family-with-classifier",
        len(free_threading_classifier_errors([beta], False, 2)) == 1,
    )
    unrelated = free_threading_classifier_errors(
        ["Programming Language :: Python :: 3.14"], True, 2
    )
    check(
        "unrelated-classifiers-ignored",
        len(unrelated) == 1 and beta in unrelated[0],
        repr(unrelated),
    )

    failures = [name for name, ok, _ in results if not ok]
    for name, ok, detail in results:
        suffix = "" if ok else ": " + detail
        print(f"{'ok  ' if ok else 'FAIL'}{name}{suffix}")
    print(
        f"self-test: {len(results)} checks, {len(failures)} failure(s)"
    )
    if failures:
        print("failed: " + ", ".join(failures), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    if "--self-test" in sys.argv[1:]:
        raise SystemExit(self_test())
    raise SystemExit(main())

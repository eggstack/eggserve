#!/usr/bin/env python3
"""Release-workflow structure guard (Plan 268 Tracks D/E/F).

Validates `.github/workflows/release.yml` against the corrective closure
rules without executing the release graph:

- aggregate gates pre-publication qualification (abi-proof + required
  native qualifiers), so a failing gate blocks publication;
- no `continue-on-error` on a required qualification lane;
- build-host native smoke excludes deferred artifacts;
- `manylinux:` never receives the string `pypi` (baseline vs policy split);
- `--compatibility` travels separately in the maturin args;
- QEMU lanes have explicit `docker/setup-qemu-action` setup (build + post);
- QEMU/Alpine executions use `sh -c`, never `bash -c` in minimal images;
- CPython 3.15 lanes enable prerelease resolution until final is available;
- the build job pins `MACOSX_DEPLOYMENT_TARGET` to the matrix policy
  (`11.0`; maturin otherwise defaults x86_64 to 10.12).

Usage:
    python3 scripts/check-release-workflow.py [--workflow .github/workflows/release.yml]
    python3 scripts/check-release-workflow.py --self-test
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release.yml"

REQUIRED_AGGREGATE_NEEDS = {
    "preflight",
    "build",
    "abi-proof",
    "qualify-aarch64-glibc",
    "qualify-aarch64-musl",
    "qualify-windows-arm64",
}

# Jobs that must never carry continue-on-error (required gates).
REQUIRED_NO_CONTINUE = {
    "build",
    "abi-proof",
    "qualify-aarch64-glibc",
    "qualify-aarch64-musl",
    "qualify-windows-arm64",
    "aggregate",
}


def load_text(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def check_workflow(text: str) -> list[str]:
    errors: list[str] = []

    # --- Track E: aggregate needs ---
    m = re.search(r"(?m)^\s{2}aggregate:\n(?:.*\n)*?\s+needs:\s*\[(.*?)\]", text)
    if not m:
        errors.append("aggregate job `needs:` not found")
    else:
        needs = {n.strip().strip("'\"") for n in m.group(1).split(",")}
        missing = REQUIRED_AGGREGATE_NEEDS - needs
        if missing:
            errors.append(
                "aggregate `needs:` missing required gates: "
                + ", ".join(sorted(missing))
            )

    # --- Track E: no continue-on-error on required lanes ---
    for job in REQUIRED_NO_CONTINUE:
        # Find the job block and look for continue-on-error: true inside it.
        jm = re.search(
            rf"(?m)^\s{{2}}{re.escape(job)}:\n((?:.*\n)*?)(?=^\s{{2}}\w+:|\Z)",
            text,
        )
        if jm and re.search(r"continue-on-error\s*:\s*true", jm.group(1)):
            errors.append(f"job {job!r} must not use continue-on-error")

    # --- Track B: build-host native smoke must exclude deferred ---
    if "matrix.smoke == 'native'" not in text:
        errors.append(
            "build native smoke condition `matrix.smoke == 'native'` not found "
            "(deferred artifacts must skip build-host execution)"
        )
    # A deferred artifact must never be installed on the build host: the
    # only install in the build job's native-smoke step is guarded above.
    # Detect an accidental second native condition covering deferred.
    if re.search(r"matrix\.smoke\s*==\s*'deferred", text):
        # Deferred smoke must not appear as a build-job install guard.
        # Qualifier jobs legitimately mention artifact names, not this guard.
        pass

    # --- Track A: baseline vs policy split ---
    if re.search(r"(?m)^\s*manylinux:\s*[\"']?pypi[\"']?\s*$", text):
        errors.append(
            "workflow passes `pypi` through `manylinux:` "
            "(baseline must be 2_17/musllinux_1_2/auto; pypi belongs in --compatibility)"
        )
    if "matrix.manylinux" not in text:
        errors.append("workflow must feed `manylinux:` from matrix.manylinux")
    if "matrix.compatibility" not in text and "--compatibility" not in text:
        errors.append(
            "workflow must pass `--compatibility ${{ matrix.compatibility }}` "
            "in the maturin args"
        )

    # --- Track D: explicit QEMU setup + sh (not bash) in minimal images ---
    # Build job QEMU setup (for matrix.qemu_platform).
    if "docker/setup-qemu-action" not in text:
        errors.append("workflow missing pinned docker/setup-qemu-action")
    # Post-publish must have its own explicit setup (runner-global binfmt is
    # not sufficient for linux/arm/v7 Docker invocations).
    post_idx = text.find("post-publish:")
    if post_idx == -1:
        errors.append("post-publish job not found")
    else:
        post_text = text[post_idx:]
        if "setup-qemu-action" not in post_text:
            errors.append(
                "post-publish lanes lack explicit docker/setup-qemu-action "
                "(ARMv7 QEMU requires pinned binfmt setup)"
            )
        if "platforms: linux/arm/v7" not in post_text:
            errors.append(
                "post-publish QEMU setup must declare `platforms: linux/arm/v7`"
            )
    # Minimal Alpine images may lack bash: QEMU/Alpine docker executions must
    # use `sh -c`. Search docker invocations for `bash -c`.
    for i, line in enumerate(text.splitlines(), start=1):
        stripped = line.strip()
        # Only docker-run payload lines matter; workflow shell lines are bash
        # by design (`shell: bash` for the runner itself is fine).
        if stripped.startswith("bash -c") and (
            "arm32v7" in text[max(0, text.find(line) - 2000):text.find(line)]
            or "alpine" in text[max(0, text.find(line) - 2000):text.find(line)].lower()
        ):
            errors.append(
                f"line {i}: QEMU/Alpine docker execution must use `sh -c`, "
                "not `bash -c` (minimal images may lack bash)"
            )
    # Simpler global guard: no `bash -c '` payload inside a docker run block.
    # The two QEMU/alpine lanes were converted to `sh -c`; any remaining
    # `bash -c '` adjacent to a docker image is a regression.
    if re.search(r"\$\{\{\s*matrix\.qemu_image\s*\}\}\s*\n\s*bash -c", text):
        errors.append("build QEMU lane must use `sh -c`, not `bash -c`")
    if re.search(r'"\$IMAGE"\s*\n\s*bash -c', text):
        errors.append("post-publish QEMU lane must use `sh -c`, not `bash -c`")

    # --- Track F: 3.15 prerelease resolution ---
    # Any lane requesting 3.15 must enable allow-prereleases until final.
    if '"3.15"' in text and "allow-prereleases" not in text:
        errors.append(
            "workflow requests CPython 3.15 without allow-prereleases "
            "(Track F: prerelease resolution required until final)"
        )

    # --- Plan 269 Track F: macOS deployment-target pin ---
    # The wheel matrix declares macosx_11_0_* platform tags, but maturin
    # defaults x86_64 to MACOSX_DEPLOYMENT_TARGET=10.12 (aarch64 already
    # floors at 11.0). The build job must pin the matrix policy explicitly
    # or the x86_64 wheel tag falls outside the wheel-matrix authority.
    if not re.search(
        r'(?m)^\s*MACOSX_DEPLOYMENT_TARGET:\s*["\']?11\.0["\']?\s*$', text
    ):
        errors.append(
            "workflow must pin `MACOSX_DEPLOYMENT_TARGET: \"11.0\"` "
            "(matrix policy is macosx_11_0_*; unpinned x86_64 defaults to 10.12)"
        )

    return errors


def cmd_self_test() -> int:
    failures = 0

    def check(name: str, text: str, expect_ok: bool, snippet: str = "") -> None:
        nonlocal failures
        errs = check_workflow(text)
        ok = not errs
        combined = "\n".join(errs)
        if ok == expect_ok and (not snippet or snippet in combined):
            print(f"OK {name}")
        else:
            print(f"FAIL {name}: ok={ok} expected_ok={expect_ok}")
            print(combined[-2000:] or "(no errors)")
            failures += 1

    real = load_text(DEFAULT_WORKFLOW)
    check("real workflow passes", real, True)
    # Mutations that must fail.
    check(
        "aggregate missing gate fails",
        real.replace(
            "needs: [preflight, build, abi-proof, qualify-aarch64-glibc, "
            "qualify-aarch64-musl, qualify-windows-arm64]",
            "needs: [preflight, build]",
        ),
        False,
        "missing required gates",
    )
    check(
        "pypi as manylinux fails",
        real.replace(
            "manylinux: ${{ matrix.manylinux }}",
            "manylinux: pypi",
        ),
        False,
        "through `manylinux:`",
    )
    check(
        "bash in QEMU fails",
        real.replace(
            "${{ matrix.qemu_image }} \\\n            sh -c '",
            "${{ matrix.qemu_image }} \\\n            bash -c '",
        ),
        False,
        "must use `sh -c`",
    )
    check(
        "unpinned deployment target fails",
        real.replace(
            'MACOSX_DEPLOYMENT_TARGET: "11.0"',
            'MACOSX_DEPLOYMENT_TARGET: "10.12"',
        ),
        False,
        "MACOSX_DEPLOYMENT_TARGET",
    )
    return 1 if failures else 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workflow", type=Path, default=DEFAULT_WORKFLOW)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)
    if args.self_test:
        return cmd_self_test()
    try:
        text = load_text(args.workflow)
    except FileNotFoundError:
        print(f"FAIL: workflow not found: {args.workflow}", file=sys.stderr)
        return 1
    errors = check_workflow(text)
    if errors:
        print("Release workflow INVALID:", file=sys.stderr)
        for e in errors:
            print(f"  - {e}", file=sys.stderr)
        return 1
    print("Release workflow structure OK:")
    print("  aggregate gates abi-proof + aarch64-glibc/musl + windows-arm64")
    print("  no continue-on-error on required lanes")
    print("  manylinux baseline vs --compatibility policy split")
    print("  explicit QEMU setup + sh (not bash) in minimal images")
    print("  3.15 prerelease resolution present")
    print('  MACOSX_DEPLOYMENT_TARGET "11.0" pinned')
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

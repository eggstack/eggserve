# Phase 38 — Packaging and Installation Closure

## Goal

Prove that EggServe’s published artifacts work independently of the source checkout. Packaging tests must validate the actual wheel, binary, and Rust crate surfaces users will install.

## Track A — Clean wheel verification

Build wheels for every supported Python/platform target through the intended release workflow.

For each wheel:
- create a fresh virtual environment;
- install only the wheel and declared runtime dependencies;
- run from a temporary directory outside the repository;
- unset `PYTHONPATH` and repository-specific environment variables;
- verify `import eggserve` and every public `__all__` name;
- verify native extension loading;
- verify version metadata;
- verify package data, licenses, and notices;
- verify no source-tree module shadows the installed package.

Do not count tests that set `PYTHONPATH` to the checkout as installed-wheel validation.

## Track B — Installed-wheel functional smoke tests

From the clean environment, exercise:
- static server startup on an ephemeral loopback port;
- context-manager lifecycle;
- Python callback handler;
- static fallback;
- HEAD and range responses;
- public-bind acknowledgement behavior;
- local HTTP client request;
- local HTTPS client request when TLS is included;
- documented exception classes;
- `python -m eggserve --help`;
- CLI/binary discovery.

Use standalone smoke scripts copied to a temporary directory, not package test modules imported from the source tree.

## Track C — Binary packaging and discovery

Define how the Python package locates the `eggserve` executable:
- bundled binary, companion package, or PATH discovery;
- deterministic search order;
- actionable missing-binary errors;
- architecture/platform match;
- version compatibility between Python package and binary.

Test:
- clean PATH with installed binary;
- missing binary;
- wrong-version binary if version coupling exists;
- paths containing spaces;
- executable permission failures;
- Windows executable naming.

## Track D — Rust crate consumption

Create clean sample projects that consume published-intent crate surfaces:
- default features;
- server TLS feature;
- `client`;
- `client-tls`;
- Python internal feature excluded from normal use.

Run `cargo package` and inspect the package contents. Verify examples, README links, license files, and source inclusion/exclusion.

If an MSRV is declared, test it. Otherwise document that stable Rust is required without promising a fixed MSRV.

## Track E — Python version and ABI matrix

Define supported Python versions and wheel ABI strategy.

Validate:
- minimum supported Python;
- current stable Python;
- newest supported Python;
- macOS/Linux/Windows wheel tags;
- x86_64/arm64 architecture correctness;
- PyO3 forward-compatibility workarounds only where explicitly supported.

Reject mislabeled wheel tags and avoid committing wheel artifacts to the repository.

## Track F — Metadata and release contents

Verify:
- package name/version consistency across Cargo, Python metadata, and CLI;
- project URLs;
- description and classifiers;
- license expression and included files;
- README rendering;
- security/contact links;
- supported platform claims;
- dependency declarations;
- source distribution policy if an sdist is published.

## Track G — Installation paths

Document and test:
- `pip install` from wheel;
- `cargo install` for the binary if supported;
- crate dependency usage;
- local development build separately from release installation;
- uninstall/reinstall behavior;
- upgrade from the immediately preceding pre-release where practical.

## CI requirements

Add matrix jobs that:
- build artifacts once per target;
- install in clean environments;
- run smoke scripts outside the checkout;
- upload artifacts only through Actions;
- never commit `dist/` outputs;
- fail on package-content or import drift.

## Acceptance criteria

- Installed-wheel tests use no source-tree imports.
- All public imports work from the wheel.
- Server, callback, client, and CLI smoke tests pass from clean installs.
- Binary discovery is deterministic and documented.
- Wheel tags match actual architecture and Python ABI.
- Rust package contents are audited.
- Metadata, licenses, and versions are consistent.
- No build artifact is tracked in git.

## Non-goals

- No new package manager integration beyond declared release targets.
- No automatic system service installation.
- No container image requirement unless separately scoped.

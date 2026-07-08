# Contributing to eggserve

## Filing issues

Open issues on the GitHub repository. Use the appropriate template:

- **Bug reports:** Include steps to reproduce, expected behavior, and actual behavior
- **Feature requests:** Must include a rationale and reference to how it fits the project scope
- **Security issues:** See [SECURITY.md](SECURITY.md)

## Pull requests

- Keep PRs small and focused on a single change
- All changes must be backed by a plan in `plans/`
- Do not make broad changes without updating the relevant plan first
- Ensure `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` pass

## Non-goal changes

If your change crosses a current non-goal (listed in [docs/non-goals.md](docs/non-goals.md)), you must update `docs/non-goals.md` and [docs/threat-model.md](docs/threat-model.md) before or in the same PR. Do not expand scope implicitly.

## Dependency additions

See [docs/dependency-policy.md](docs/dependency-policy.md) before adding any dependency. All dependencies require explicit justification.

## Roadmap

The project milestone sequence is documented in [plans/ROADMAP.md](plans/ROADMAP.md). Changes should align with the current milestone.

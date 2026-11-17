# Action Pinning Policy

## Policy

All third-party GitHub Actions used in CI/CD workflows are pinned to immutable commit SHA digests. This prevents supply-chain attacks where a compromised action could be silently updated at a major-version tag.

## Current pinned actions

| Action | SHA | Version |
|--------|-----|---------|
| `actions/checkout` | `34e114876b0b11c390a56381ad16ebd13914f8d5` | v4.3.1 |
| `dtolnay/rust-toolchain` | `fa04a1451ff1842e2626ccb99004d0195b455a88` | master |
| `Swatinem/rust-cache` | `23869a5bd66c73db3c0ac40331f3206eb23791dc` | v2.9.1 |
| `actions/setup-python` | `a26af69be951a213d495a4c3e4e4022e16d87065` | v5.6.0 |
| `actions/upload-artifact` | `ea165f8d65b6e75b540449e92b4886f43607fa02` | v4.6.2 |
| `actions/download-artifact` | `d3f86a106a0bac45b974a628896c90dbdf5c8093` | v4.3.0 |
| `docker/setup-qemu-action` | `c7c53464625b32c7a7e944ae62b3e17d2b600130` | v3 |
| `PyO3/maturin-action` | `e83996d129638aa358a18fbd1dfb82f0b0fb5d3b` | v1 |
| `pypa/gh-action-pypi-publish` | `dc37677b2e1c63e2034f94d8a5b11f265b73ba33` | v1.14.2 |

## Update procedure

1. Check for new releases on the action's GitHub repository.
2. Verify the release is from a trusted source (official GitHub org, well-known maintainer).
3. Look up the full commit SHA for the new release tag:
   ```sh
   git ls-remote https://github.com/<owner>/<repo>.git refs/tags/<tag>
   ```
4. Update the SHA in all workflow files under `.github/workflows/`.
5. Add a comment with the version tag for human readability (e.g., `# v4.3.1`).
6. Test the workflow locally or via a PR before merging.

## Container image pinning

Release qualification also runs in container images. Mutable tags
(`python:3.11-alpine`) are pinned to manifest-list digests so the qualification
input cannot change silently:

| Image | Digest |
|-------|--------|
| `python:3.11-alpine` | `sha256:7fbad8e4652eae0186429a6f26cf5b23e27e8c6fd7ac4d0cfd7a6988cdeea1e1` |
| `arm32v7/python:3.11-bookworm` | `sha256:f5ac5bd76de95eeadfd7fc35d5385e6fa14fa3ed6ce070ecacbfb935eec47c66` |
| `arm32v7/python:3.11-alpine` | `sha256:c7b37e6338824a66959195b7657a7bd0c6e5020d51b49f295b27fd9af95dd227` |

### Image digest update procedure

1. Resolve the manifest-list digest for the tag from Docker Hub (requires
   network access; the token is anonymous registry auth):
   ```sh
   repo=library/python; tag=3.11-alpine
   token=$(curl -fsSL "https://auth.docker.io/token?service=registry.docker.io&scope=repository:${repo}:pull" \
     | python3 -c 'import json,sys;print(json.load(sys.stdin)["token"])')
   curl -fsSI -H "Authorization: Bearer ${token}" \
     -H 'Accept: application/vnd.oci.image.index.v1+json' \
     -H 'Accept: application/vnd.docker.distribution.manifest.list.v2+json' \
     -H 'Accept: application/vnd.docker.distribution.manifest.v2+json' \
     "https://registry-1.docker.io/v2/${repo}/manifests/${tag}" \
     | tr -d '\r' | grep -i '^docker-content-digest'
   ```
2. Replace the `image:tag@sha256:...` reference in
   `.github/workflows/release.yml`.
3. Record the new digest in the table above in the same change.
4. Do not guess a digest: if the registry is unreachable, leave the tag and
   record the blocked lookup rather than pinning an unverified value.

## Verification

To verify all actions are pinned to SHAs:
```sh
grep -rn 'uses:' .github/workflows/ | grep -v '@[a-f0-9]\{40\}'
```

Any output indicates an action not pinned to a SHA digest.

Release tooling is kept deterministic separately from action pinning:
`scripts/install-cargo-tools.sh` installs and verifies the pinned
`cargo-audit 0.23.0` and `cargo-deny 0.21.0` versions. The manually dispatched release workflow builds, qualifies, and optionally
publishes cross-platform wheel artifacts. PyPI publication uses OIDC Trusted
Publishing via the `pypa/gh-action-pypi-publish` action (pinned to commit SHA)
through protected GitHub Environments. crates.io publication remains a local
maintainer action.

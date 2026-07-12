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
| `actions/cache` | `0057852bfaa89a56745cba8c7296529d2fc39830` | v4.3.0 |
| `softprops/action-gh-release` | `c95fe1489396fe8a9eb87c0abf8aa5b2ef267fda` | v2.2.1 |

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

## Verification

To verify all actions are pinned to SHAs:
```sh
grep -rn 'uses:' .github/workflows/ | grep -v '@[a-f0-9]\{40\}'
```

Any output indicates an action not pinned to a SHA digest.

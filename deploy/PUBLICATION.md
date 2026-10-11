# Publishing a verified release

Build every platform from one reviewed main-branch commit using `release.yml`. Retain the successful release run ID and verify the dry-run publisher plan before publication. The game source comes from that build run, even when the publication tooling is newer.

## Exact tag preflight

A maintainer must create the lightweight `v<BUILD_VERSION>` tag at the **release build's exact `head_sha`** before dispatching publication. Use the existing authenticated GitHub CLI account; do not add a broad token to Actions. Do not change an existing tag.

1. Inspect the successful `release.yml` run, its main branch, source commit and the `BUILD_VERSION` file at that commit.
2. Read `repos/PIndustries/arkline-engine/git/ref/tags/v<VERSION>` with `gh api`. If it exists, accept it only when `object.type` is `commit` and `object.sha` exactly matches the verified source. Stop on any mismatch. A permission/network failure is not proof of absence.
3. Only after an explicit HTTP 404, create the missing ref with `gh api --method POST repos/PIndustries/arkline-engine/git/refs -f ref=refs/tags/v<VERSION> -f sha=<VERIFIED_FULL_SOURCE_SHA>`.
4. Read back the ref and verify the exact commit. Never use a force update or delete/recreate a tag.

The publisher checks this tag before image builds or deployment and uses `gh release create --verify-tag`. This separates maintainer tag creation from the scoped Actions token's release upload, avoiding an attempted implicit tag creation at a historical workflow-bearing commit. The token retains only the existing contents/package write and Actions read permissions. An existing release is still rejected; failed partial deployments must be reconciled rather than blindly rerun.

## Coordinated server upgrades

When a separate reviewed operation owns the TLS server rollout, dispatch the publisher with `upgrade_servers=false`. Publication then updates browser/download hosting and release links without replacing server images or changing their TLS configuration. Verify world continuity and public secure connections after the separately coordinated server rollout.

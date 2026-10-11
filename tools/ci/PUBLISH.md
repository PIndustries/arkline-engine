# Reviewed release publication

`publish.yml` is a main-only manual workflow on Fleet. It accepts one successful main-branch `release.yml` dispatch containing all three platform download artifacts plus the browser artifact. A partial platform run is insufficient. Build and publish are deliberately separate; publication never compiles from a dirty local checkout.

Before dispatch, review the complete release-build run, native/browser acceptance and version. For an optional world-server upgrade, also create and verify operator backups for both worlds and provide their reference. The reference is an operator attestation, not automated backup verification. Server upgrades default off; web/download releases can advance independently.

The helper validates the run's repository/workflow/branch/conclusion, each artifact's full checksum inventory and receipt, inner client/server manifests, the browser's recursive license notices, and every tracked source file in each vendored source archive against the exact release commit. Artifact ZIP extraction rejects traversal, symlinks, duplicates and oversized payloads. Executable archives are inspected, never run by the publisher.

For a review-only plan, use:

```
python3 tools/ci/publish-release.py --run-id RUN_ID --version YY.M.D.BUILD --output /outside/repository/new-directory
```

This downloads and verifies artifacts and writes `publication-plan.json`; it does not build/push images, create releases or alter deployments. `--apply` is the publication boundary. A version already present in GitHub releases or either current static-image version directory is rejected, including after partial failures. Never delete that guard to retry blindly.

Publication copies `/srv/web` and `/srv/downloads` from the current immutable trusted web image, preserving previous version paths. It adds the complete new platform/source archives and a versioned browser tree, matching source offer, notices, checksums and manifest; builds server/web images from the exact game commit; pushes only to the existing private packages; and verifies package privacy and immutable digests. The new web image rolls out first, followed by public full-file hash checks of every download and critical browser assets. Optional server upgrades require the backup reference and fresh world-health checks. GitHub release assets and Bastion's versioned download URLs are published only after those checks. GitHub release visibility follows repository visibility; the download endpoints are public.

Bastion updates use resourceVersion-checked JSON patches for only Arkline publication flags/URLs. Unrelated environment entries and secret references are never rewritten. Scoped Kubernetes credentials and the registry token live only in job-specific temporary files/configuration, removed on exit. Existing world data is never copied, reinitialized or included in artifacts.

Public web rollout/hash/redirect failure attempts restoration of the previous immutable web digest; successful restoration is explicitly reported, and failure requires operator recovery. Other failed workflow stages are not evidence of rollback or completion. Inspect its uploaded plan and actual image/release state before continuing. A later failure after successful web verification may leave the new static version served while Bastion still points to the previous release; a failure after GitHub release creation requires operator reconciliation. No automatic server rollback, schema rollback, backup service, native Windows acceptance or unattended browser acceptance is claimed. The first complete registry/deployment execution of this helper remains to be verified independently; unit tests cover validation boundaries, not external services.


## Partial-publication recovery

Each workflow attempt has its own staging directory, so a validation/build failure before changing the web image can be retried normally. After successful web verification, immutable version paths intentionally prevent blind rerun. Download the failed attempt's plan, compare the recorded web/server digests and public hashes, and inspect the GitHub release and Bastion URLs. If bytes are already published, either finish only the remaining publication metadata manually after verifying those exact hashes, or issue a new version through a new successful build run. Do not replace existing version bytes or delete a GitHub release to bypass the guard. Any server-upgrade failure requires independent world/backup assessment; there is no automated database rollback.

The publisher does not manage Cloudflare ingress. Dedicated apex/www routing to Bastion therefore remains intact across web-image releases, even when an older source image contains an unused apex redirect server block.

## Isolated image builder

Publication uses a job-scoped `docker-container` Buildx builder rather than the
Docker daemon's embedded BuildKit. Its Linux AMD64 BuildKit 0.34.0 image is pinned
by manifest digest; the worker permits two parallel build steps and is limited
to four CPU cores and 8 GiB memory. Builds explicitly select that builder and
load their output for the existing private push and digest checks. Missing or
non-container builder configuration fails closed. The exit trap removes only
the job's builder and temporary configuration, including on publication failure.
No shared Docker daemon restart, history query, or cache pruning is performed.

A publisher-only workflow update can publish an earlier successful release run:
source identity is taken from that run's commit, not the publisher checkout's
HEAD. All receipts, tracked source bytes and BUILD_VERSION are still checked
against the selected release commit before any publication.

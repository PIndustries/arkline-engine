# Self-hosted build and deployment operators

Fleet's existing organization runners use `self-hosted`, `Linux`, `X64`, `phx`, `fleet`, and `org` labels. The private Default group includes phx through phx-4 and permits private organization repositories. phx-5 belongs to public-release. Repository runner listings omit these organization-owned runners.

These existing runners share a privileged Docker host and passwordless administrative access. Run only trusted main-branch push/manual jobs; do not execute untrusted fork code. Namespace-scoped deploy credentials constrain workflow intent, but do not isolate hostile code from the shared host. Runner registrations and global permissions were not changed by this setup.

## Fleet toolchain

Verified host: Ubuntu26.04 LTS, x86_64, glibc2.43. Installed for the existing `gha-runner` user:

- Rust/Cargo1.89.0 with rustfmt, Clippy and wasm32-unknown-unknown target.
- wasm-bindgen-cli0.2.129.
- Native ALSA, udev, X11, cursor/randr/input, xkbcommon, Wayland, OpenGL/EGL development packages; clang/lld/LLVM, CMake and Ninja.
- Existing Python3.12.13 and Docker29.1.3/Buildx0.37.0 verified.

Add `/opt/gha-org-runner/.cargo/bin` to `GITHUB_PATH` in jobs. Python3.12 is available at `/opt/py312/bin/python3.12`. No global runner PATH was changed. Future host package maintenance should set `NEEDRESTART_MODE=l` to report service restart needs without restarting unrelated services. Building native Linux artifacts directly on this host can impose a recent glibc floor; choose a pinned older Linux build container if supporting older distributions. Container, Windows cross-build and macOS runtime validation remain separate acceptance boundaries.

## Deployment credentials

GitHub repository secrets contain dedicated service-account kubeconfigs with the current official Talos API endpoint and certificate authority. TLS verification stays enabled. They contain no cluster-admin identity.

- Arkline: `ARKLINE_DEPLOY_KUBECONFIG`, service account `arkline/arkline-deploy`.
- Bastion: `BASTION_DEPLOY_KUBECONFIG`, service account `bastion/bastion-deploy`.

Arkline can get/list/watch/patch/update only `arkline-web` Deployment and `arkline`/`arkline-test` StatefulSets, read named services, and read pod health/logs in its namespace. A separate Bastion RoleBinding allows that Arkline identity to get/list/watch/patch/update only Deployment `bastion/bastion`, for publishing download URLs. Bastion's own identity can update only its named Deployment and read its namespace's pods/logs.

Neither identity can read Secrets/PVCs, exec into pods, create/delete workloads, or access cluster nodes. Actual-token authorization checks verified both allowed and denied operations. Workload list/watch is restricted to those same resource names for rollout status. Unfiltered workload listing and unrelated workload access remain denied.

Use a temporary0600 kubeconfig file inside the job and remove it on exit. Never print or attach it. Guard deployment with trusted main-only dispatch and the required operator backup reference. Backup creation/exec is intentionally not delegated; operator backup verification remains a server-upgrade prerequisite.

## Manual rotation and revocation

The approved service-account tokens are persistent and require operator rotation. Create a replacement annotated service-account-token Secret for the same scoped identity, wait for controller population, and construct a kubeconfig using only that token plus the verified API endpoint/CA. Replace the corresponding GitHub repository secret through encrypted stdin, run positive/negative authorization checks and a read-only pod-health check, then delete the old token Secret. Do not print token data during these steps.

For emergency revocation, remove the relevant RoleBindings and delete the token Secret, then remove the repository secret. Removing the Arkline identity also requires removing its cross-namespace Bastion RoleBinding. Recreate/rotate credentials only after identifying why revocation was required. No automatic token rotation or automatic backup service is claimed.

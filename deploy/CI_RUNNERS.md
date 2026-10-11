# Self-hosted build and deployment operators

Fleet's existing organization runners use `self-hosted`, `Linux`, `X64`, `phx`, `fleet`, and `org` labels. The private Default group includes phx through phx-4 and permits private organization repositories. phx-5 belongs to public-release. Repository runner listings omit these organization-owned runners.

These existing runners share a privileged Docker host and passwordless administrative access. Build CI runs only for pushes to trusted repository branches or manual dispatch; there is no pull_request trigger. Deployment/publication dispatch is main-only. Do not execute untrusted fork code. Namespace-scoped deploy credentials constrain workflow intent, but do not isolate hostile code from the shared host. Runner registrations and global permissions were not changed by this setup.

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

## Dedicated macOS runner

The repository-scoped `arkline-macos-arm64` runner is installed at
`/Users/Shared/arkline-macos-runner` with labels `self-hosted`, `macOS`,
`ARM64`, and `arkline`. Its single listener executes one job at a time.
The runner directory is mode 0700 and registration credentials are mode 0600.
Do not copy those files into source, release archives, or diagnostics.

The user LaunchAgent is
`~/Library/LaunchAgents/actions.runner.PIndustries-arkline-engine.arkline-macos-arm64.plist`.
It runs in the `thepherm` login session: the Mac must be awake, connected, and
that user logged in for availability. From the runner directory, use
`./svc.sh status`, `./svc.sh stop`, and `./svc.sh start` for lifecycle management.
The directory is outside Documents because macOS privacy controls prevented
LaunchAgent access to the initial workspace location.

The runner PATH explicitly selects installed Python 3.12, Rust, Homebrew tools,
and system tools. Rust 1.89.0, Python 3.12.13 and Swift 6.2.3 were verified.
The runner includes its own Node runtime for Actions. Do not infer an installed
Node version from Homebrew's directory name; inspect the executable version.
Registration used a short-lived repository token supplied through the runner's
supported environment input, without printing it or placing it in command-line
arguments. This is a persistent runner: repository scope is not a sandbox.
Workflow trusted-source gates are essential, and unrelated repositories must
not be granted access to this runner.

Actual self-hosted macOS CI completed successfully in run `38121079749`.
CLI/tests/build success does not establish interactive GPU/audio acceptance or
Apple notarization; those remain separate release checks.

## Windows cross-build on Fleet

`tools/ci/windows-cross.sh build-image` prepares the private Docker toolchain;
`test` cross-compiles the MSVC workspace and runs tests through Wine;
`package /absolute/output/path` creates Windows ZIP downloads and matching vendored-source
archives. Use an empty output directory outside the checkout. The wrapper
restores generated output ownership to the calling runner. `ARKLINE_WINDOWS_IMAGE`
can select an explicitly prepared toolchain image.

This path targets `x86_64-pc-windows-msvc` from Fleet's x86_64 Linux host. It does
not switch Windows releases to GNU/MinGW. The build uses Rust 1.89.0,
cargo-xwin 0.20.2, LLVM, Microsoft's SDK/CRT and Wine in an isolated build image.
The Dockerfile records the base-image digest and SDK/CRT versions. SDK files
remain private build dependencies; they are not added to source or binary
release archives. Provisioning requires compliance with the Microsoft SDK/CRT
terms referenced by cargo-xwin. Toolchain versions and the resolved Docker image
ID should be retained with the workflow evidence.

`--windows-cross` explicitly separates target identity from host identity,
selects `target/x86_64-pc-windows-msvc/release`, produces `.exe`/ZIP output, and
marks the release manifest's validation runtime as Wine compatibility only.
Passing these checks is not native Windows acceptance: real Windows graphics,
audio, device handling, installer/runtime availability, and signing still need
separate validation. Windows binaries retain their Microsoft Visual C++ runtime
requirement. No SDK shim or replacement Windows DLL is shipped with Arkline.

The initial Fleet proof used Wine 10.0 (`10.0~repack-6`), LLVM 19,
SDK `10.0.26100`, and CRT package selector `14.44.17.14` (whose headers identify
CRT 14.44.35220). Wine 8 failed to load Rust binaries because it lacks
`bcryptprimitives.dll`; do not downgrade the compatibility runtime or ship a
replacement DLL to work around that. All four cross-built debug executables
(client, server, CLI, verifier) completed `--help` under Wine 10. Final release
workflows still rebuild and test their selected committed source.

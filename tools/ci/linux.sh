#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
IMAGE="${ARKLINE_LINUX_IMAGE:-arkline-linux:rust1.89-ubuntu22.04}"
MODE="${1:-test}"
if [[ "$MODE" == build-image ]]; then
  docker build -f "$ROOT/tools/ci/linux.Dockerfile" -t "$IMAGE" "$ROOT/tools/ci"
  exit
fi
CACHE="${RUNNER_TEMP:-/tmp}/arkline-linux-${GITHUB_RUN_ID:-local}-${GITHUB_RUN_ATTEMPT:-1}"
mkdir -p "$CACHE"
# Test worlds are disposable. Keep their many SQLite commits off Docker's shared
# overlay disk; files still survive connection close/reopen within each test.
ARGS=(--rm --tmpfs /tmp:rw,exec,nosuid,size=2g --user "$(id -u):$(id -g)" -e HOME=/cache -e CARGO_HOME=/cache/cargo -e RUSTUP_HOME=/usr/local/rustup -e CARGO_BUILD_JOBS=4 -v "$ROOT:/src" -v "$CACHE:/cache" -w /src)
case "$MODE" in
 test)
  # Limit overlapping test fixtures and subprocesses independently of compilation.
  # Current geometry is covered by workspace tests. The never-adopted v4
  # enclosure executable is retired; its immutable export still has Python checks.
  docker run "${ARGS[@]}" -e RUST_TEST_THREADS=2 "$IMAGE" bash -lc 'export PATH=/usr/local/cargo/bin:$PATH; /bin/kill --version && git --version && git check-attr --source=HEAD filter -- Cargo.toml && python3 tools/check_repository_organization.py && python3 tools/test_repository_organization.py && cargo fmt --all -- --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --locked -p arkline-editor git_workflow::tests::clone_and_contribution_branch_use_the_requested_checkout -- --exact --nocapture && cargo test --locked -p arkline-editor git_workflow::tests::in_flight_cancel_terminates_owned_transport_without_connecting -- --exact --nocapture && cargo test --workspace --locked && python3 tools/test_package_release.py && python3 tools/art/test_pioneer_export_contract.py && python3 tools/art/test_forward_bay_proof.py && python3 tools/art/test_canonical_enclosure_proof.py'
  ;;
 package|source)
  DEST="$(python3 -c 'import pathlib,sys;print(pathlib.Path(sys.argv[1]).resolve())' "${2:?output directory required}")"
  mkdir -p "$DEST"
  FLAG=--binaries-only
  [[ "$MODE" != source ]] || FLAG=--source-only
  docker run "${ARGS[@]}" -v "$DEST:/out" "$IMAGE" bash -lc 'set -e; export PATH=/usr/local/cargo/bin:$PATH; python3 tools/package-release.py /out "$1"; if [[ "$1" == --source-only ]]; then bash tools/ci/check-source.sh /out; fi' _ "$FLAG" ;;
 browser)
  DEST="$(python3 -c 'import pathlib,sys;print(pathlib.Path(sys.argv[1]).resolve())' "${2:?output directory required}")"
  mkdir -p "$DEST"
  docker run "${ARGS[@]}" -v "$DEST:/out" "$IMAGE" bash -lc 'set -e; export PATH=/usr/local/cargo/bin:$PATH; cargo clippy --locked -p arkline-client --target wasm32-unknown-unknown -- -D warnings && bash apps/client/web/build.sh /out' ;;
 *) echo 'Usage: linux.sh build-image|test|package OUTPUT|source OUTPUT|browser OUTPUT' >&2; exit 2 ;;
esac

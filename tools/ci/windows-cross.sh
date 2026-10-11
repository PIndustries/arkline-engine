#!/usr/bin/env bash
set -euo pipefail
# Private SDK build image; outputs are explicitly Wine-tested, not native Windows QA.
repo=$(cd "$(dirname "$0")/../.." && pwd)
mode=${1:-test}
image=${ARKLINE_WINDOWS_IMAGE:-arkline-windows-cross:rust1.89-xwin0.20.2}
case "$mode" in
  build-image) exec docker build -t "$image" -f "$repo/tools/ci/windows-cross.Dockerfile" "$repo/tools/ci" ;;
  test|package) ;;
  *) echo "Usage: $0 build-image|test|package [output-directory]" >&2; exit 2 ;;
esac
args=(--rm --init --tmpfs /tmp:rw,exec,nosuid,size=3g -v "$repo:/workspace" -w /workspace -e WINEDEBUG=-all -e WINEPREFIX=/tmp/arkline-wine -e GIT_CONFIG_COUNT=1 -e GIT_CONFIG_KEY_0=safe.directory -e GIT_CONFIG_VALUE_0=/workspace)
if [[ "$mode" == package ]]; then
  [[ $# == 2 ]] || { echo 'package requires an empty output directory outside the repository' >&2; exit 2; }
  mkdir -p "$2"
  output=$(cd "$2" && pwd)
  case "$output/" in "$repo/"*) echo 'Output must be outside repository' >&2; exit 2 ;; esac
  args+=(-v "$output:/out")
  command=(python3 tools/package-release.py /out --windows-cross)
else
  command=(cargo xwin test --workspace --locked --target x86_64-pc-windows-msvc)
fi
# Docker runs trusted release source. Restore ownership of only generated output/cache.
cleanup() {
  docker run --rm -v "$repo/target:/generated" "$image" chown -R "$(id -u):$(id -g)" /generated
  if [[ "$mode" == package ]]; then
    docker run --rm -v "$output:/generated" "$image" chown -R "$(id -u):$(id -g)" /generated
  fi
}
trap cleanup EXIT
mkdir -p "$repo/target"
docker run "${args[@]}" "$image" "${command[@]}"

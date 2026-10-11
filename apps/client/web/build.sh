#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
DEST="${1:-$ROOT/target/web}"
cd "$ROOT"
command -v wasm-bindgen >/dev/null || { echo 'Install wasm-bindgen-cli matching Cargo.lock before building.' >&2; exit 1; }
cargo fetch --locked
cargo build --locked --release -p arkline-client --target wasm32-unknown-unknown
mkdir -p "$DEST"
wasm-bindgen --target web --no-typescript --out-dir "$DEST" "$ROOT/target/wasm32-unknown-unknown/release/arkline-client.wasm"
cp apps/client/web/index.html apps/client/web/bootstrap.js "$DEST/"
cp LICENSE TRADEMARKS.md BUILD_VERSION "$DEST/"
python3 - "$ROOT" "$DEST" <<'PYNOTICES'
import importlib.util, pathlib, sys
root, dest = map(pathlib.Path, sys.argv[1:])
spec = importlib.util.spec_from_file_location("arkline_release", root / "tools/package-release.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module.notices(root, dest / "ThirdParty")
PYNOTICES
printf 'Browser build: %s\n' "$DEST"

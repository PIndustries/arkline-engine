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
python3 - "$ROOT" "$DEST" <<'PYRECEIPT'
import hashlib, json, pathlib, subprocess, sys
root, dest = map(pathlib.Path, sys.argv[1:])
version = (root / "BUILD_VERSION").read_text().strip()
receipt = {"build_version": version,
           "source_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
           "uncommitted_source": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=root, text=True).strip()),
           "target": "wasm32-unknown-unknown",
           "source_archive": f"Arkline-{version}-aarch64-apple-darwin-source.tar.gz"}
(dest / "RELEASE.json").write_text(json.dumps(receipt, sort_keys=True, indent=2) + "\n")
lines = []
for path in sorted(dest.rglob("*")):
    if path.is_file() and path != dest / "SHA256SUMS":
        digest = hashlib.sha256()
        with path.open("rb") as handle:
            for block in iter(lambda: handle.read(1024 * 1024), b""):
                digest.update(block)
        lines.append(f"{digest.hexdigest()}  {path.relative_to(dest).as_posix()}")
(dest / "SHA256SUMS").write_text("\n".join(lines) + "\n")
PYRECEIPT
printf 'Browser build: %s\n' "$DEST"

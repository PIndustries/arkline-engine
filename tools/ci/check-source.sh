#!/usr/bin/env bash
set -euo pipefail
OUTPUT="$(realpath "${1:?source output directory required}")"
COMMIT="$(git rev-parse HEAD)"
VERSION="$(cat BUILD_VERSION)"
shopt -s nullglob
ARCHIVES=("$OUTPUT"/Arkline-*-source.tar.gz)
[[ ${#ARCHIVES[@]} == 1 ]] || { echo 'Exactly one source archive required' >&2; exit 1; }
(cd "$OUTPUT" && sha256sum -c SHA256SUMS)
VERIFY="$(mktemp -d)"
trap 'rm -rf "$VERIFY"' EXIT
tar -xzf "${ARCHIVES[0]}" -C "$VERIFY"
cd "$VERIFY/$(basename "${ARCHIVES[0]}" .tar.gz)"
test -d vendor
test -s .cargo/config.toml
test -s ThirdParty/THIRD-PARTY-NOTICES.txt
test -s ThirdParty/dependencies.json
CARGO_HOME="$VERIFY/empty-cargo-home" cargo metadata --locked --offline --format-version 1 > "$VERIFY/metadata.json"
python3 - "$VERIFY/metadata.json" "$COMMIT" "$VERSION" "$OUTPUT/RELEASE.json" <<'PY'
import json, sys
record = json.load(open('RELEASE.json'))
assert record == json.load(open(sys.argv[4])), 'Source receipts differ'
assert record['source_commit'] == sys.argv[2] and record['build_version'] == sys.argv[3]
assert record['uncommitted_source'] is False and 'target' not in record
metadata = json.load(open(sys.argv[1]))
notices = json.load(open('ThirdParty/dependencies.json'))
expected = {(p['name'], p['version']) for p in metadata['packages'] if p['source']}
actual = {(p['name'], p['version']) for p in notices}
assert expected and actual == expected, 'Dependency notice inventory is incomplete'
print(f'Validated offline vendored source and {len(expected)} dependency notices')
PY

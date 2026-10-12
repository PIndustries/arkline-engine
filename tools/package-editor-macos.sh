#!/bin/bash
# Local editor bundle; ad-hoc signing is not Developer ID signing/notarization.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "Usage: tools/package-editor-macos.sh OUTPUT_DIRECTORY [debug|release]" >&2
  exit 2
fi
PROFILE="${2:-release}"
case "$PROFILE" in debug|release) ;; *) echo "Profile must be debug or release" >&2; exit 2 ;; esac
if [ "$(uname -s)" != Darwin ]; then
  echo "This package requires macOS and codesign" >&2
  exit 2
fi
for TOOL in cargo python3 codesign; do
  command -v "$TOOL" >/dev/null || { echo "Missing required tool: $TOOL" >&2; exit 2; }
done
mkdir -p "$1"
OUTPUT="$(cd "$1" && pwd)"
APP="$OUTPUT/Arkline Editor.app"
if [ -e "$APP" ] || [ -L "$APP" ]; then
  echo "Refusing to overwrite existing app: $APP. Use a new output directory." >&2
  exit 2
fi
for FILE in BUILD_VERSION LICENSE assets/Arkline.icns; do
  [ -f "$ROOT/$FILE" ] || { echo "Missing package input: $FILE" >&2; exit 2; }
done
BUILD_VERSION="$(cat "$ROOT/BUILD_VERSION")"
BUILD_NUMBER="$(python3 - "$BUILD_VERSION" <<'PY'
import datetime, re, sys
value = sys.argv[1]
if not re.fullmatch(r'\d{2}\.\d{1,2}\.\d{1,2}\.\d{1,9}', value):
    sys.exit('BUILD_VERSION must be yy.month.day.positive-build')
year, month, day, sequence = map(int, value.split('.'))
if sequence < 1:
    sys.exit('BUILD_VERSION build must be positive')
try:
    date = datetime.date(2000 + year, month, day)
except ValueError as error:
    sys.exit(str(error))
print(f'{date:%Y%m%d}.{sequence}')
PY
)"
STAGING="$(mktemp -d "$OUTPUT/.arkline-editor-package.XXXXXX")"
trap 'rm -rf "$STAGING"' EXIT
cd "$ROOT"
# Metadata resolves absolute, relative and configured target directories.
TARGET_DIRECTORY="$(cargo metadata --format-version 1 --no-deps --locked | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
BUILD_ARGS=(build -p arkline-editor --locked --message-format=json-render-diagnostics)
if [ "$PROFILE" = release ]; then BUILD_ARGS+=(--release); fi
CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}" cargo "${BUILD_ARGS[@]}" > "$STAGING/cargo-artifacts.jsonl"
# Use Cargo's exact executable path, including any configured target triple.
BINARY="$(python3 - "$STAGING/cargo-artifacts.jsonl" "$TARGET_DIRECTORY" <<'PY'
import json, pathlib, sys
artifacts = []
for line in pathlib.Path(sys.argv[1]).read_text().splitlines():
    event = json.loads(line)
    if event.get('reason') == 'compiler-artifact' and event.get('target', {}).get('name') == 'arkline-editor' and 'bin' in event.get('target', {}).get('kind', []) and event.get('executable'):
        artifacts.append(pathlib.Path(event['executable']).resolve())
if len(artifacts) != 1:
    sys.exit('Expected exactly one editor executable from Cargo')
try:
    artifacts[0].relative_to(pathlib.Path(sys.argv[2]).resolve())
except ValueError:
    sys.exit('Editor executable is outside Cargo target_directory')
if not artifacts[0].is_file():
    sys.exit('Cargo editor executable is missing')
print(artifacts[0])
PY
)"
BUNDLE="$STAGING/Arkline Editor.app"
mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Resources"
cp "$BINARY" "$BUNDLE/Contents/MacOS/arkline-editor"
chmod 755 "$BUNDLE/Contents/MacOS/arkline-editor"
cp "$ROOT/assets/Arkline.icns" "$ROOT/LICENSE" "$ROOT/BUILD_VERSION" "$BUNDLE/Contents/Resources/"
if [ -f "$ROOT/TRADEMARKS.md" ]; then cp "$ROOT/TRADEMARKS.md" "$BUNDLE/Contents/Resources/"; fi
if [ -d "$ROOT/ThirdParty" ]; then cp -R "$ROOT/ThirdParty" "$BUNDLE/Contents/Resources/ThirdParty"; fi
python3 - "$BUNDLE/Contents/Info.plist" "$BUILD_VERSION" "$BUILD_NUMBER" <<'PY'
import plistlib, sys
with open(sys.argv[1], 'wb') as file:
    plistlib.dump({
        'CFBundleName': 'Arkline Editor',
        'CFBundleDisplayName': 'Arkline Editor',
        'CFBundleIdentifier': 'industries.p.arkline.editor.embedded',
        'CFBundleExecutable': 'arkline-editor',
        'CFBundlePackageType': 'APPL',
        'CFBundleShortVersionString': '0.1.0',
        'CFBundleVersion': sys.argv[3],
        'ArklineBuildVersion': sys.argv[2],
        'CFBundleIconFile': 'Arkline.icns',
        'NSHighResolutionCapable': True,
    }, file)
PY
codesign --force --deep --sign - "$BUNDLE"
codesign --verify --deep --strict "$BUNDLE"
# Recheck after building: never replace an app created while compilation ran.
if [ -e "$APP" ] || [ -L "$APP" ]; then
  echo "Output app appeared during packaging; refusing to overwrite it" >&2
  exit 2
fi
# mkdir is exclusive even if another process races the existence check.
mkdir "$APP" || { echo "Cannot reserve new output app" >&2; exit 2; }
if ! mv "$BUNDLE/Contents" "$APP/Contents"; then
  rmdir "$APP" 2>/dev/null || true
  echo "Cannot install the verified bundle" >&2
  exit 1
fi
codesign --verify --deep --strict "$APP"
printf 'Built %s (build %s; local ad-hoc signature)\n' "$APP" "$BUILD_VERSION"

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
STAGING="$(mktemp -d "$OUTPUT/.arkline-editor-package.XXXXXX")"
trap 'rm -rf "$STAGING"' EXIT
cd "$ROOT"
BUILD_ARGS=(build -p arkline-editor --locked --message-format=json-render-diagnostics)
if [ "$PROFILE" = release ]; then BUILD_ARGS+=(--release); fi
CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}" python3 "$ROOT/tools/build.py" \
  --artifact-copy "$STAGING/arkline-editor" \
  --version-output "$STAGING/BUILD_VERSION" \
  -- "${BUILD_ARGS[@]}" > "$STAGING/cargo-artifacts.jsonl"
BUILD_VERSION="$(cat "$STAGING/BUILD_VERSION")"
# CFBundleVersion has numeric component limits; retain the human build stamp
# separately in ArklineBuildVersion and the bundled BUILD_VERSION file.
BUILD_NUMBER="$(python3 -B - "$BUILD_VERSION" "$ROOT/tools" <<'PY'
import sys
sys.path.insert(0, sys.argv[2])
from build import macos_bundle_version

try:
    print(macos_bundle_version(sys.argv[1]))
except ValueError as error:
    sys.exit(str(error))
PY
)"
BINARY="$STAGING/arkline-editor"
BUNDLE="$STAGING/Arkline Editor.app"
mkdir -p "$BUNDLE/Contents/MacOS" "$BUNDLE/Contents/Resources"
cp "$BINARY" "$BUNDLE/Contents/MacOS/arkline-editor"
chmod 755 "$BUNDLE/Contents/MacOS/arkline-editor"
cp "$ROOT/assets/Arkline.icns" "$ROOT/LICENSE" "$STAGING/BUILD_VERSION" "$BUNDLE/Contents/Resources/"
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

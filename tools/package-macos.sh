#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUTPUT="${1:?Usage: tools/package-macos.sh OUTPUT_DIRECTORY [debug|release]}"
PROFILE="${2:-release}"
case "$PROFILE" in
  release|debug) ;;
  *) echo "Profile must be debug or release" >&2; exit 2 ;;
esac
cd "$ROOT"
if [ "$PROFILE" = release ]; then
  cargo build --workspace --locked --release
else
  cargo build --workspace --locked
fi
APP="$OUTPUT/Arkline.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$OUTPUT/command-tools"
cp -f "target/$PROFILE/arkline-client" "$APP/Contents/MacOS/Arkline"
cp -f "target/$PROFILE/arkline-server" "$APP/Contents/MacOS/arkline-server"
cp -f "target/$PROFILE/arkline" "$OUTPUT/command-tools/arkline"
cp -f "target/$PROFILE/arkline-verifier" "$OUTPUT/command-tools/arkline-verifier"
cp -f README.md LICENSE "$OUTPUT/"
cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Arkline</string>
<key>CFBundleDisplayName</key><string>Arkline</string>
<key>CFBundleIdentifier</key><string>industries.p.arkline.preview</string>
<key>CFBundleExecutable</key><string>Arkline</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
# Local ad-hoc signing verifies bundle integrity; this is not Developer ID
# signing or Apple notarization, and is not a public distribution package.
codesign --force --deep --sign - "$APP"
codesign --verify --deep --strict "$APP"
printf 'Built %s\n' "$APP"

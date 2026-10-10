#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "Usage: tools/package-macos.sh OUTPUT_DIRECTORY [debug|release]" >&2
  echo "Build identity comes from the repository BUILD_VERSION file; overrides are not accepted." >&2
  exit 2
fi
OUTPUT="$1"
PROFILE="${2:-release}"
BUILD_VERSION="$(cat "$ROOT/BUILD_VERSION")"
if [[ ! "$BUILD_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "BUILD_VERSION must contain four numeric components: year.month.day.build" >&2
  exit 2
fi
IFS='.' read -r BUILD_YEAR BUILD_MONTH BUILD_DAY BUILD_SEQUENCE <<< "$BUILD_VERSION"
if [ "${#BUILD_YEAR}" -ne 2 ] || [ "${#BUILD_MONTH}" -gt 2 ] || [ "${#BUILD_DAY}" -gt 2 ] || [ "${#BUILD_SEQUENCE}" -gt 9 ]; then
  echo "BUILD_VERSION requires a two-digit year, month/day of at most two digits, and build of at most nine digits" >&2
  exit 2
fi
if (( 10#$BUILD_MONTH < 1 || 10#$BUILD_MONTH > 12 || 10#$BUILD_DAY < 1 || 10#$BUILD_DAY > 31 || 10#$BUILD_SEQUENCE < 1 )); then
  echo "BUILD_VERSION month/day/build components are out of range" >&2
  exit 2
fi
# Calendar identity stays visible in ArklineBuildVersion; the bundle build
# uses numeric date.sequence spelling (26.10.9.1 becomes 20261009.1).
printf -v BUILD_NUMBER '20%02d%02d%02d.%d' "$((10#$BUILD_YEAR))" "$((10#$BUILD_MONTH))" "$((10#$BUILD_DAY))" "$((10#$BUILD_SEQUENCE))"
if [ ! -f "$ROOT/assets/Arkline.icns" ]; then
  echo "Missing bundle icon: $ROOT/assets/Arkline.icns" >&2
  exit 2
fi
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
cp -f README.md LICENSE BUILD_VERSION "$OUTPUT/"
cp -f assets/Arkline.icns "$APP/Contents/Resources/Arkline.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Arkline</string>
<key>CFBundleDisplayName</key><string>Arkline</string>
<key>CFBundleIdentifier</key><string>industries.p.arkline.preview</string>
<key>CFBundleExecutable</key><string>Arkline</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>$BUILD_NUMBER</string>
<key>ArklineBuildVersion</key><string>$BUILD_VERSION</string>
<key>CFBundleIconFile</key><string>Arkline.icns</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSMicrophoneUsageDescription</key><string>Record your spoken ship order when you press Talk to ship.</string>
<key>NSSpeechRecognitionUsageDescription</key><string>Transcribe your spoken ship orders on this Mac.</string>
</dict></plist>
PLIST
swiftc -target "$(uname -m)-apple-macos14.0" -swift-version 5 -O "$ROOT/tools/arkline-speech.swift" -o "$APP/Contents/MacOS/arkline-speech" \
  -Xlinker -sectcreate -Xlinker __TEXT -Xlinker __info_plist -Xlinker "$APP/Contents/Info.plist"
# Local ad-hoc signing verifies bundle integrity; this is not Developer ID
# signing or Apple notarization, and is not a public distribution package.
codesign --force --deep --sign - "$APP"
codesign --verify --deep --strict "$APP"
printf 'Built %s\n' "$APP"

#!/usr/bin/env bash
# Build OpenTouchDesigner.app and a .dmg from it.
#
#   scripts/package-macos.sh [version]
#
# The app is *unsigned*. Signing needs a paid Apple Developer account, and
# pretending otherwise would be worse than saying so: Gatekeeper will refuse
# the first launch, and the README tells people how to get past it. Everything
# here is reproducible from a clean checkout — no assets are committed that
# this script cannot regenerate.
set -euo pipefail

VERSION="${1:-0.1.0}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST="$ROOT/dist"
APP="$DIST/OpenTouchDesigner.app"
DMG="$DIST/OpenTouchDesigner-$VERSION-macos.dmg"

cd "$ROOT"
rm -rf "$DIST"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

echo "==> building"
# Native by default. A universal binary needs a universal *Python* to link
# PyO3 against, and a stock Homebrew Python is one architecture — so
# cross-linking fails on most machines with a linker error that has nothing
# to do with this code. Opt in with OTD_UNIVERSAL=1 when you have both, and
# get a clear message rather than a mystery when you do not.
HOST="$(rustc -vV | awk '/^host:/ {print $2}')"
BIN_APP=()
BIN_CLI=()
build() {
    local t="$1"
    cargo build --release --target "$t" -p otd-app -p otd-cli
    BIN_APP+=("target/$t/release/otd-app")
    BIN_CLI+=("target/$t/release/otd")
}
build "$HOST"
if [ "${OTD_UNIVERSAL:-0}" = "1" ]; then
    OTHER=x86_64-apple-darwin
    [ "$HOST" = "$OTHER" ] && OTHER=aarch64-apple-darwin
    if build "$OTHER"; then
        echo "==> universal ($HOST + $OTHER)"
    else
        echo "!! $OTHER failed to build — shipping $HOST only."
        echo "!! Usually PyO3: linking it for another architecture needs a"
        echo "!! Python for that architecture as well."
    fi
fi

echo "==> bundling"
# The headless runtime rides along: a show machine that has the editor should
# have the thing that runs a patch without one.
lipo -create "${BIN_APP[@]}" -output "$APP/Contents/MacOS/OpenTouchDesigner"
lipo -create "${BIN_CLI[@]}" -output "$APP/Contents/MacOS/otd"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>OpenTouchDesigner</string>
  <key>CFBundleDisplayName</key><string>OpenTouchDesigner</string>
  <key>CFBundleIdentifier</key><string>dev.opentouchdesigner.editor</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleExecutable</key><string>OpenTouchDesigner</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <!-- The Audio Device In CHOP opens the microphone, and Video Device In
       runs ffmpeg against the camera. Without these the system kills the
       request instead of asking, and the node reports a missing device. -->
  <key>NSMicrophoneUsageDescription</key>
  <string>The Audio Device In CHOP listens to an input so patches can react to sound.</string>
  <key>NSCameraUsageDescription</key>
  <string>The Video Device In TOP reads a camera as a texture.</string>
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key><string>OpenTouchDesigner project</string>
      <key>CFBundleTypeRole</key><string>Editor</string>
      <key>LSItemContentTypes</key><array><string>public.data</string></array>
      <key>CFBundleTypeExtensions</key><array><string>otd</string><string>otdc</string></array>
    </dict>
  </array>
</dict>
</plist>
PLIST

echo "==> icon"
"$ROOT/scripts/make-icon.sh" "$APP/Contents/Resources/AppIcon.icns"

echo "==> dmg"
STAGE="$DIST/stage"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
cp "$ROOT/docs/INSTALL-macOS.txt" "$STAGE/READ ME FIRST.txt"
hdiutil create -volname "OpenTouchDesigner" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
rm -rf "$STAGE"

echo "==> $DMG"
du -h "$DMG"

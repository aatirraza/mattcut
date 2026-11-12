#!/usr/bin/env bash
# Build MattCut for macOS as one universal app (Apple Silicon + Intel) on a drag-to-Applications DMG:
#
#   dist/MattCut-<version>-mac-universal.dmg
#   dist/MattCut-mac-universal.dmg              same file under a fixed name (the download link)
#
# Usage: packaging/macos/package.sh [--skip-build]
#
# Signing: ad-hoc by default (unsigned for distribution: on other Macs, Gatekeeper asks the user to
# allow it once in System Settings > Privacy & Security). Set MACOS_SIGN_IDENTITY to a
# "Developer ID Application" identity to sign properly.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
TARGET_DIR="$(cd "$ROOT/../mattcraft" && pwd)/target"
DIST="$ROOT/dist"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
IDENTITY="${MACOS_SIGN_IDENTITY:--}"
TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
export MACOSX_DEPLOYMENT_TARGET=11.0

SKIP_BUILD=0
[[ "${1:-}" == "--skip-build" ]] && SKIP_BUILD=1

echo "==> MattCut $VERSION for macOS (universal), signing identity: $IDENTITY"

if [[ "$SKIP_BUILD" == 0 ]]; then
  args=()
  for t in "${TARGETS[@]}"; do args+=(--target "$t"); done
  (cd "$ROOT" && cargo build --release -p mattcut "${args[@]}")
fi

WORK="$TARGET_DIR/mattcut-macos-package"
APP="$WORK/MattCut.app"
rm -rf "$WORK"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$DIST"

# ---- universal binary --------------------------------------------------------------------------
inputs=()
for t in "${TARGETS[@]}"; do inputs+=("$TARGET_DIR/$t/release/mattcut"); done
lipo -create -output "$APP/Contents/MacOS/MattCut" "${inputs[@]}"
lipo -info "$APP/Contents/MacOS/MattCut"

# ---- icon (from the 200 px logo) ---------------------------------------------------------------
ICONSET="$WORK/MattCut.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s "$ROOT/apps/mattcut/assets/logo.png" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  sips -z $d $d "$ROOT/apps/mattcut/assets/logo.png" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/MattCut.icns"

# ---- Info.plist --------------------------------------------------------------------------------
sed -e "s/@VERSION@/$VERSION/g" "$HERE/Info.plist.in" >"$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist" >/dev/null

# ---- sign --------------------------------------------------------------------------------------
# Hardened runtime (needed for notarization) only with a real identity; it would also need the
# audio-input entitlement for voice-over recording.
opts=(--timestamp --options runtime)
[[ "$IDENTITY" == "-" ]] && opts=(--timestamp=none)
codesign --force --deep --sign "$IDENTITY" "${opts[@]}" "$APP"
codesign --verify --strict "$APP"

# ---- DMG ---------------------------------------------------------------------------------------
STAGE="$WORK/dmg"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
DMG="$DIST/MattCut-$VERSION-mac-universal.dmg"
rm -f "$DMG"
hdiutil create -volname "MattCut $VERSION" -srcfolder "$STAGE" -fs HFS+ -format UDZO -imagekey zlib-level=9 -ov "$DMG" >/dev/null
[[ "$IDENTITY" != "-" ]] && codesign --force --sign "$IDENTITY" --timestamp "$DMG"
cp "$DMG" "$DIST/MattCut-mac-universal.dmg"

echo "==> $DMG ($(du -h "$DMG" | cut -f1))"
shasum -a 256 "$DMG"

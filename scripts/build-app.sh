#!/bin/bash
# Builds build/Folio.app (Apple Silicon, ad-hoc signed unless a "Developer ID Application" identity exists).
# Usage: scripts/build-app.sh [--debug]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
PROFILE=release
[[ "${1:-}" == "--debug" ]] && PROFILE=debug
VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/')r"
BUILD="$(git rev-list --count HEAD 2>/dev/null || echo 1)"
if [[ $PROFILE == release ]]; then cargo build --release -p folio-shell >&2; else cargo build -p folio-shell >&2; fi
APP="$ROOT/build/Folio.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "target/$PROFILE/Folio" "$APP/Contents/MacOS/Folio"
cp Resources/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
sed -e "s/__VERSION__/$VERSION/" -e "s/__BUILD__/$BUILD/" Resources/Info.plist > "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"
IDENTITY="$(security find-identity -v -p codesigning 2>/dev/null | grep -m1 'Developer ID Application' | sed -E 's/.*"(.*)"/\1/' || true)"
if [[ -n "$IDENTITY" ]]; then
  codesign --force --options runtime --timestamp --entitlements Resources/Folio.entitlements --sign "$IDENTITY" "$APP" >&2
else
  codesign --force --entitlements Resources/Folio.entitlements --sign - "$APP" >&2
fi
echo "$APP"

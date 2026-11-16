#!/bin/bash
# Builds dist/Folio-<version>.dmg from build/Folio.app (macOS, Apple Silicon). Notarizes when the
# keychain profile "folio-notary" and a Developer ID exist (see docs/SIGNING.md).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
[[ -d build/Folio.app ]] || ./scripts/build-app.sh >/dev/null
VERSION="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' build/Folio.app/Contents/Info.plist)"
mkdir -p dist
STAGE="$(mktemp -d)"
cp -R build/Folio.app "$STAGE/"
ln -s /Applications "$STAGE/Programme"
DMG="dist/Folio-$VERSION.dmg"
[[ -f "$DMG" ]] && mv "$DMG" "$(mktemp -d)/"
hdiutil create -volname "Folio $VERSION" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
if security find-identity -v -p codesigning 2>/dev/null | grep -q "Developer ID Application" && xcrun notarytool history --keychain-profile folio-notary >/dev/null 2>&1; then
  xcrun notarytool submit "$DMG" --keychain-profile folio-notary --wait >&2
  xcrun stapler staple "$DMG" >&2
  echo "notarisiert" >&2
else
  echo "ad hoc signiert (keine Developer ID / kein Notar-Profil)" >&2
fi
echo "$DMG"

#!/bin/bash
# Copies Folio.app with its own bundle ID (de.daniel.folio.rust.selftest), so tests never touch the real
# settings, and prints the path of its binary.
# Usage: BIN="$(scripts/make-test-app.sh build/Folio.app <new folder>)"; FOLIO_LIBRARY=<copy> FOLIO_BACKGROUND=1 "$BIN"
set -euo pipefail
SRC="${1:?Folio.app}"; DEST="${2:?Zielordner}"
mkdir -p "$DEST"
APP="$DEST/Folio Selftest.app"
rm -rf "$APP"
cp -R "$SRC" "$APP"
PL="$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier de.daniel.folio.rust.selftest" "$PL"
/usr/libexec/PlistBuddy -c "Set :CFBundleName Folio Selftest" "$PL"
/usr/libexec/PlistBuddy -c "Delete :CFBundleURLTypes" "$PL" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Delete :CFBundleDocumentTypes" "$PL" 2>/dev/null || true
codesign --force --sign - "$APP" >/dev/null 2>&1
echo "$APP/Contents/MacOS/Folio"

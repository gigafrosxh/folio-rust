#!/bin/bash
# make release V=X.Y.Z: sets the version, moves [Unreleased] in CHANGELOG.md to [X.Y.Zr], runs the tests and the
# self test, commits, tags vX.Y.Zr, builds the DMG and publishes a GitHub release. macOS only.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
V="${1:?Version, z. B. 1.6.2}"
V="${V%r}"
[[ "$V" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Version wie 1.6.2 angeben (das r wird angehängt)"; exit 1; }
TAG="v${V}r"
[[ -z "$(git status --porcelain)" ]] || { echo "Arbeitskopie nicht sauber"; exit 1; }
BODY="$(awk '/^## \[Unreleased\]/{f=1;next} /^## \[/{f=0} f' CHANGELOG.md)"
[[ -n "$(echo "$BODY" | tr -d '[:space:]')" ]] || { echo "Unter [Unreleased] steht nichts"; exit 1; }
sed -i '' -E "s/^version = \"[0-9.]+\"/version = \"$V\"/" Cargo.toml
sed -i '' -E "s/\"version\": \"[0-9.]+\"/\"version\": \"$V\"/" crates/folio-shell/tauri.conf.json
DATE="$(date +%Y-%m-%d)"
awk -v v="${V}r" -v d="$DATE" '{print} /^## \[Unreleased\]/{print ""; print "## [" v "] – " d}' CHANGELOG.md > CHANGELOG.tmp && mv CHANGELOG.tmp CHANGELOG.md
cargo test --workspace
./scripts/build-app.sh >/dev/null
make selftest
git add -A
git commit -m "Version ${V}r"
git tag "$TAG"
DMG="$(./scripts/make-installer.sh)"
git push origin HEAD --tags
gh release create "$TAG" "$DMG" --title "Folio ${V}r" --notes "$BODY"

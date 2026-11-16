# Mitwirken an Folio (Rust)

Danke fürs Mitmachen! Folio (Rust) ist die Rust-Neuschreibung der macOS-Notizen-App Folio, nur für macOS auf Apple Silicon.
Die Oberfläche und die Dokumentation sind deutsch; Code und Kommentare sind englisch.

## Einrichten

1. Rust über `rustup` installieren (stable, 1.85 oder neuer). Node/npm sind nicht nötig.
2. Bauen und testen:

   ```sh
   make build      # cargo build
   make test       # cargo test
   make app        # build/Folio.app
   ```

3. Die App mit einer **Testbibliothek** starten, nie mit deinen echten Notizen:

   ```sh
   BIN="$(scripts/make-test-app.sh build/Folio.app /tmp/folio-test-app)"
   mkdir -p /tmp/folio-test && FOLIO_LIBRARY=/tmp/folio-test "$BIN"
   ```

## Architektur

Jede Notiz ist eine eigene komprimierte `.folio`-Datei in einem normalen Ordnerbaum; es gibt keine Datenbank. Das Format
ist dasselbe wie in der Swift-App.

| Bereich | Wo | Was |
| --- | --- | --- |
| Kern | `crates/folio-core` | Ohne UI und getestet: Dateiformat, RTF, Suche, Bibliothek, Versionen, Papierkorb, Sicherungen, Links, Markdown, Codeausführung, KI |
| App | `crates/folio-app` | Zustand und Befehle (`App::call`), Datenbanken, Import, Testserver |
| Fenster | `crates/folio-shell` | Tauri 2: Fenster, Menüs, Dialoge, URL-Schemata |
| Oberfläche | `ui/` | HTML/CSS/JS-Module ohne Framework |

Faustregeln:

- Logik gehört in `folio-core`, mit Unit-Tests. Die Oberfläche ruft nur Befehle auf.
- Neue Felder im Dateiformat immer optional; unbekannte Felder bleiben erhalten (`extra`).
- Beschädigte Notizen werden nie gelöscht, nur nach `.folio/damaged` verschoben.

## Tests

- `make test` muss grün sein, inklusive `crates/folio-core/tests/compat.rs` (Notizen der Swift-App).
- Selbsttest im echten Fenster, unsichtbar: `make selftest`.
- Oberflächentests im Headless-Browser gegen `folio-dev` (siehe AGENTS.md).

## Issues und Pull Requests

- Fehler und Wünsche als Issue mit den Vorlagen melden; jedes Issue bekommt eine Priorität `P0`–`P3`.
- Ein Pull Request löst ein Issue (`Closes #N`), trägt einen Eintrag unter `## [Unreleased]` in `CHANGELOG.md` ein und
  beschreibt, wie er getestet wurde.
- Datenschutz: Neue Netzwerkzugriffe oder Speicherorte in `docs/PRIVACY.md` nachtragen.

Releases macht der Maintainer mit `make release V=X.Y.Z` (Tag `vX.Y.Zr`, siehe README).

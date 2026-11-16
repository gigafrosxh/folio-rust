# AGENTS.md – Arbeitsanleitung für KI-Agenten (Claude, Codex …)

Folio (Rust) ist die Rust-Neuschreibung der macOS-Notizen-App Folio. Jede Notiz ist eine eigene komprimierte `.folio`-Datei
in einem normalen Ordnerbaum, im selben Format wie die Swift-App. Repository: <https://github.com/gigafrosxh/folio-rust>.
Die Swift-App liegt unter <https://github.com/gigafrosxh/folio>. Kommuniziert wird mit dem Nutzer auf **Deutsch**; Code und
Kommentare sind englisch, Commit-Titel deutsch im Stil des Repos.

## Feste Regeln

1. **Nur macOS.** Folio (Rust) wird ausschließlich für macOS gebaut und veröffentlicht (Apple Silicon). Keine Builds,
   Releases oder Dokumentation für andere Plattformen. Der Kern nutzt bewusst macOS-Bibliotheken (`libcompression`).
2. **Versionsschema `x.y.zr`**: Die Rust-App trägt die Versionsnummer der Swift-App mit angehängtem `r` (z. B. `1.6.1r`).
   In `Cargo.toml` steht die Zahl ohne `r`; Tags heißen `vX.Y.Zr`, Releases „Folio X.Y.Zr“.
3. **Vor jeder Aufgabe die GitHub-Issues prüfen** (`gh issue list --state open`). Gibt es die Anfrage schon als Issue,
   wird genau dieses Issue abgearbeitet und im Commit mit `Closes #N` geschlossen.
4. **Jedes Update wird ein Release**: Eintrag unter `## [Unreleased]` in `CHANGELOG.md`, dann `make release V=X.Y.Z`.
   Jeder gemeldete Bug („[Bug] …“-Issue) bekommt ein eigenes Patch-Release.
5. **Folio nie selbst auf dem Mac des Nutzers installieren** – kein Ersetzen von `/Applications/Folio.app`.
6. **Nie mit der echten Bibliothek testen** (`~/Documents/Folio`). Immer eine Kopie oder eine leere Testbibliothek über
   `FOLIO_LIBRARY=<pfad>`; Einstellungen landen dann in `<pfad>/.folio/rust-settings.json`.
7. **Nur im Hintergrund testen** (`FOLIO_BACKGROUND=1`): keine sichtbaren Fenster, kein Fokus, keine Dialoge. Der Nutzer
   arbeitet währenddessen am selben Mac.
8. Systemänderungen (z. B. `brew install …`, `cargo install …`) nur nach ausdrücklicher Zustimmung des Nutzers.
9. Nichts löschen, was der Nutzer nicht ausdrücklich freigibt; beschädigte Notizen werden nie gelöscht, nur nach
   `.folio/damaged` verschoben.
10. **Dateiformat kompatibel halten**: Neue Felder immer optional (`#[serde(default)]`), unbekannte Felder bleiben über
    `extra` erhalten. Eine von Rust geschriebene Notiz muss die Swift-App öffnen können und umgekehrt
    (`crates/folio-core/tests/compat.rs`, Fixtures aus der Swift-App).
11. **Jedes Issue hat eine Priorität** `P0`–`P3` (P0: Datenverlust, Absturz, Eingabe am falschen Ort).

## Aufbau

| Pfad | Inhalt |
| --- | --- |
| `crates/folio-core` | Kern ohne UI, voll getestet: `archive` (Container, LZFSE, CRC), `model` (Metadaten, Blöcke, Swift-kompatibles JSON), `rtf` (Run-Modell ⇄ Cocoa-RTF), `text` (Tags, Wiki-Links, Titel, Statistik, Vorlagen), `search` (Filter, Sortierung, manuelle Reihenfolge, Schnell öffnen), `library` (Scanner, Organisation, Versionen, Papierkorb, Sicherungen), `links` (Link-Index, Graph-Layout), `markdown`, `html` (Import), `runner` (Codeausführung), `languages`, `ai` (Ollama, Aktionen, BM25), `capture` (Deep Links, Schnellerfassung), `note` (abgeleitete Metadaten) |
| `crates/folio-app` | `App` mit Zustand und `call(cmd, args)` (`commands.rs`), Einstellungen, Datenbanken, Apple-Notizen-Import, Testserver `folio-dev` |
| `crates/folio-shell` | Tauri-Programm `Folio`: Fenster, Menüleiste, Dialoge, `folioatt://`-Anhänge, `folio://`-Links, Menüleistensymbol |
| `ui/` | Oberfläche ohne Framework: `js/main.js` (Fenster, Liste, Befehle), `editor.js` (Notiz, Speichern, Undo, Suchen), `blocks.js` (alle Blockarten), `richtext.js` (DOM ⇄ Runs), `highlight.js`, `panels.js` (Dialoge), `selftest.js` |
| `scripts/` | `build-app.sh`, `make-test-app.sh`, `make-installer.sh`, `release.sh` |

## Bauen, testen, veröffentlichen

```sh
make build            # cargo build
make test             # cargo test (Kern + App), muss grün sein
make app              # build/Folio.app
make selftest         # App-Selbsttest im echten Fenster, unsichtbar, mit leerer Testbibliothek
make installer        # DMG in dist/
make release V=X.Y.Z  # Version setzen, Tests, Tag vX.Y.Zr, DMG, GitHub-Release
```

### Ohne Bildschirmzugriff testen

- `FOLIO_LIBRARY=<kopie>` – Pflicht bei jedem Test.
- `FOLIO_BACKGROUND=1` – kein sichtbares Fenster, kein Dock-Symbol, kein Fokus.
- `FOLIO_SELFTEST=1` – Selbsttest in der echten WebKit-Ansicht: Notiz anlegen, Listen, Code ausführen, Formel, Suche,
  Wiki-Link, Papierkorb, Versionen, Graph, Export, Anhänge. Gibt `ok`/`FAIL` je Schritt aus und beendet sich (Exit 0/1).
- Release-Builds starten mit `FOLIO_LIBRARY` nur als Testkopie (`scripts/make-test-app.sh`, Bundle-ID
  `de.daniel.folio.rust.selftest`); die echte Bundle-ID `de.daniel.folio.rust` verweigert den Testmodus (Exit 78).
- `folio-dev [port]` (Binary aus `folio-app`) liefert Oberfläche und Befehle über `http://127.0.0.1:<port>` für Tests
  im Headless-Browser.
- Kompatibilität mit der Swift-App: deren `folio-dump --check <datei>` muss Notizen der Rust-App als intakt melden;
  `textutil -convert txt` muss das RTF der Textblöcke lesen.

## Arbeitsweise, die sich bewährt hat

- Logik zuerst in `folio-core` mit Unit-Tests, dann ein Befehl in `folio-app`, dann die Oberfläche.
- Die Oberfläche spricht nur über `call(cmd, args)` mit Rust; neue Befehle in `commands.rs::dispatch` eintragen.
- Text wird im Editor als Run-Modell gehalten (`{paras: [{runs: [{text, b, i, u, s, size, font, color, link}]}]}`);
  Listen sind wie in der Swift-App Textpräfixe (`• `, `– `, `1. `, `☐ `, `☑ `).
- Lange Arbeiten (Code, KI) laufen als Auftrag (`job_poll`, `job_cancel`, `job_send`).

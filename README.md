# Folio (Rust)

Folio ist eine Notizen-App für macOS. Diese Version ist eine Neuschreibung der [Swift-App](https://github.com/gigafrosxh/folio) in **Rust**: Der gesamte Kern (Dateiformat, Bibliothek, Suche, Links, Versionen, Code-Ausführung, KI-Anbindung) ist Rust, die Oberfläche läuft in einem schlanken WebKit-Fenster (Tauri 2) ohne Framework. Text, Code und eingebettete Inhalte bleiben gemeinsam in einer lokalen Notizdatei. Die Oberfläche ist deutschsprachig.

Folio gibt es **nur für macOS** (Apple Silicon, ab macOS 14).

Die Rust-Version liest und schreibt **dieselben `.folio`-Dateien** wie die Swift-App, bytegenau im selben Format (LZFSE über Apples `libcompression`, RTF wie `NSAttributedString` es schreibt). Beide Apps können dieselbe Bibliothek benutzen.

Versionen der Rust-App tragen ein `r`: `1.6.1r` entspricht dem Funktionsstand der Swift-App 1.6.1.

## Warum Rust

Die Swift-App wurde bei großen Bibliotheken und langen Notizen langsam (synchrone Scans auf dem Hauptthread, teure Höhenberechnungen im Editor, siehe Issues #219–#222 im Swift-Repo). In der Rust-Version

- liest der Bibliotheksscan die Metadaten **parallel** (rayon) per Memory-Mapping und merkt sie sich nach Pfad, Größe und Änderungsdatum; ein erneuter Scan liest nur geänderte Dateien,
- ist die Suche über vorbereitete, gefaltete Suchtexte eine reine Speicheroperation,
- läuft jede Dateioperation außerhalb des Oberflächenthreads,
- rendert der Editor Text direkt im Browser-Layout statt Höhen selbst zu berechnen.

## Funktionen

- Echte Ordner, Suche mit Filtern (`tag:`, `#tag`, `ordner:`, `typ:`, `seit:`, `status:offen`), angeheftete Notizen, Favoriten und `#tags` (auch verschachtelt, `#projekt/folio`).
- Sortieren nach Bearbeitung, Erstellung, Titel oder eigener Reihenfolge (Notizen und Ordner per Ziehen), gesicherte Suchen als intelligente Ansichten.
- Rich Text mit Titel, Überschrift, Unterüberschrift, Monospace, Zitat, Fett, Kursiv, Unterstrichen, Durchgestrichen, Links, Aufzählungen, nummerierten Listen und Checklisten (gespeichert als RTF).
- `[[Wiki-Links]]` mit Vorschlägen, Rückverweise, nicht verlinkte Erwähnungen, eingebettete Notizen (`![[Titel]]`) und ein Graph aller Notizen.
- Codeblöcke mit Syntaxhervorhebung für 25 Sprachen und Ausführung über lokal installierte Interpreter oder Compiler, mit stdin, interaktiver Eingabe, Tests der Ausgabe, Zeitlimit und eigener Ausführungsumgebung pro Notiz.
- Formeln (KaTeX) und Diagramme (Mermaid), offline gerendert.
- Eingebettete PDFs, Bilder (drehen, Alternativtext, Breite), Dateien, Webseiten (live oder als Link), Audioaufnahmen und Zeichnungen.
- Datenbank-Abfragen gegen SQLite, PostgreSQL und MySQL; Passwörter im Schlüsselbund (dieselben Einträge wie die Swift-App).
- Eigenschaften pro Notiz (Text, Zahl, Datum, Auswahl, Ja/Nein), Vorlagen mit Platzhaltern (`{{datum}}`, `{{titel}}`, `{{cursor}}` …) und Tagesnotizen.
- Tabs, geteilte Ansicht, Notiz im eigenen Fenster, Fokusmodus, Suchen und Ersetzen.
- Versionsverlauf, eigener Papierkorb, automatische Sicherungen als ZIP, Erkennung von Änderungen durch andere Apps (Konflikte ohne Datenverlust), beschädigte Notizen werden nie gelöscht.
- Import aus Markdown und Apple Notizen, Export als Markdown und PDF (Drucken).
- Lokale KI über Ollama: Chat über die Notiz oder die ganze Bibliothek (mit Quellen), Aktionen für markierten Text (erklären, vereinfachen, umschreiben, Lernfragen), Tag-Vorschläge, Antworten als Blöcke.
- Schnellerfassung (`folio://capture`), `folio://`-Links und optional ein Menüleistensymbol.
- Heller und dunkler Modus, Akzentfarbe, Schriften und Abstände, schwebende Bedienelemente als Glas, Milchglas oder deckend.

### Noch nicht in der Rust-Version

Diese Funktionen der Swift-App hängen an Apple-Frameworks, die es für Rust nicht gibt, oder kommen in einer späteren Version:

- Apple Intelligence (Chat, Writing Tools als eigenes Menü, Textvorhersage) und Image Playground. Die Writing Tools von macOS stehen im Kontextmenü der Textfelder trotzdem zur Verfügung; als KI dient ein lokales Ollama.
- Desktop-Widgets (WidgetKit) und Transkription von Audio.
- Numbers-Tabellen erstellen; Pakete aus der Swift-Version (z. B. eingebettete `.numbers`) bleiben in der Notiz erhalten, lassen sich aber noch nicht öffnen.
- Zeichnungen der Swift-Version (PencilKit) werden nicht angezeigt, bleiben aber unverändert erhalten. Neue Zeichnungen sind Bilder.
- Terminal-Panel, Kernels (geteilte Variablen zwischen Code-Blöcken), Autovervollständigung über Sprachserver, Microsoft SQL Server.
- PDF-Volltext für die Suche wird übernommen, wenn die Swift-App ihn schon angelegt hat, aber noch nicht selbst erzeugt.
- Update-Knopf in der App (Updates gibt es über die GitHub-Releases).

## Dateien statt Datenbank

Standardmäßig liegt die Bibliothek in `~/Documents/Folio`. Jede Notiz ist eine einzelne `.folio`-Datei; Ordner entsprechen Verzeichnissen. Folios eigene Daten liegen versteckt in `<Bibliothek>/.folio/` (`history`, `trash`, `templates`, `damaged`, `organization.json`).

### Das `.folio`-Format

Die Implementierung steht in `crates/folio-core/src/archive.rs` und `model.rs`. Ganzzahlen sind Little Endian.

| Abschnitt | Inhalt |
| --- | --- |
| Header | 4 Byte Kennung `FOLI`, `u16` Version (aktuell 1), `u16` Flags (Bit 0: LZFSE) |
| Metadaten | LZFSE-komprimiertes JSON: UUID, Titel, Vorschau, Tags, Erstellungs- und Änderungsdatum, Pin-Status und Suchtext |
| Inhalt | LZFSE-komprimiertes JSON mit den Blöcken; unter anderem RTF-Daten, Quellcode, Sprache, Ausgabe, URLs und Anhangsreferenzen |
| Anhänge | `u32` Anzahl; je Anhang `u32` JSON-Headerlänge, JSON-Header und komprimierte Datensektion |
| Prüfsumme | `u32` CRC-32 über sämtliche vorangehenden Bytes |

Jede Datensektion enthält zuerst die unkomprimierte Länge (`u64`), dann die gespeicherte Länge (`u64`) und anschließend die gespeicherten Bytes. Datumswerte sind wie bei Swifts `JSONEncoder` Sekunden seit dem 1.1.2001, JSON-Schlüssel sind sortiert. Felder, die diese Version nicht kennt, werden beim Speichern unverändert übernommen. Komprimierung ist keine Verschlüsselung.

## Bauen und starten

Voraussetzungen: macOS 14 oder neuer auf Apple Silicon, Rust 1.85 oder neuer (`rustup`). Node oder npm braucht es nicht: Die Oberfläche besteht aus statischen Dateien in `ui/`.

```sh
make build        # cargo build (alle Crates)
make test         # Unit-Tests von Kern und App
make app          # Release-App: build/Folio.app (ad hoc signiert)
make run          # App bauen und öffnen
make installer    # DMG in dist/
```

Mit einer Testbibliothek starten, nie mit den echten Notizen:

```sh
BIN="$(scripts/make-test-app.sh build/Folio.app /tmp/folio-test-app)"
mkdir -p /tmp/folio-test && FOLIO_LIBRARY=/tmp/folio-test "$BIN"
```

Ohne Fenster testen (läuft im Hintergrund, stiehlt keinen Fokus):

```sh
FOLIO_LIBRARY=/tmp/folio-test FOLIO_BACKGROUND=1 FOLIO_SELFTEST=1 "$BIN"   # Selbsttest im echten Fenster
FOLIO_LIBRARY=/tmp/folio-test ./target/debug/folio-dev 47811               # Oberfläche + Befehle für Browser-Tests
```

## Berechtigungen

Beim Import aus Apple Notizen fragt macOS nach der Erlaubnis, Notizen zu steuern (**Systemeinstellungen → Datenschutz & Sicherheit → Automation → Folio**). Für Audioaufnahmen fragt macOS nach dem Mikrofon.

## Projektstruktur

| Pfad | Aufgabe |
| --- | --- |
| `crates/folio-core` | Kern ohne Oberfläche, mit Tests: Dateiformat, RTF, Suche, Bibliothek, Versionen, Papierkorb, Sicherungen, Links und Graph, Markdown, Codeausführung, Ollama, Schnellerfassung |
| `crates/folio-app` | Zustand und Befehle der App (`App::call`), Datenbanken, Apple-Notizen-Import, Testserver `folio-dev` |
| `crates/folio-shell` | Das macOS-Programm (Tauri 2): Fenster, Menüleiste, `folio://`-Links, Anhang-URLs |
| `ui/` | Oberfläche: HTML, CSS und JavaScript-Module ohne Framework, KaTeX und Mermaid offline |
| `Resources/` | Info.plist, Entitlements, Icon |
| `scripts/` | App bauen, Testkopie, DMG, Release |

## Versionen und Releases

Die Rust-App folgt den Versionen der Swift-App mit angehängtem `r` (z. B. `1.6.1r`). In `Cargo.toml` steht die Zahl ohne `r`, Tags heißen `v1.6.1r`. Jede Version hat einen Abschnitt in [CHANGELOG.md](CHANGELOG.md).

```sh
make release V=1.6.2   # Version setzen, testen, taggen, DMG bauen, GitHub-Release v1.6.2r anlegen
```

## Lizenz

[PolyForm Noncommercial 1.0.0](LICENSE.md), wie die Swift-App.

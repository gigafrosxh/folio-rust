# Changelog

Alle nennenswerten Änderungen an Folio (Rust). Format nach [Keep a Changelog](https://keepachangelog.com/de/1.1.0/),
Versionen nach dem Schema der Swift-App mit angehängtem `r` (siehe README). Die Geschichte der Swift-App steht in
[gigafrosxh/folio](https://github.com/gigafrosxh/folio/blob/main/CHANGELOG.md).

## [Unreleased]

## [1.6.1r] – 2026-11-16

Erste Version der Rust-Neuschreibung, auf dem Funktionsstand der Swift-App 1.6.1.

### Neu

- Kern komplett in Rust: `.folio`-Format bytegleich mit der Swift-App (LZFSE über `libcompression`, CRC-32, sortiertes JSON,
  Datumswerte seit 2001), unbekannte Felder bleiben beim Speichern erhalten; RTF wird wie von `NSAttributedString` gelesen und geschrieben.
- Bibliotheksscan parallel und mit Cache nach Pfad, Größe und Änderungsdatum; Suche über vorbereitete Suchtexte; alle
  Dateioperationen außerhalb des Oberflächenthreads.
- Oberfläche als schlanke Web-Ansicht (Tauri 2) ohne Framework: Seitenleiste mit Ordnern, Tags, intelligenten Ansichten,
  Vorlagen, Papierkorb und Graph; Notizliste mit Datumsgruppen und eigener Reihenfolge; Tabs, geteilte Ansicht, Notiz im
  eigenen Fenster, Fokusmodus.
- Editor mit Absatzstilen, Zeichenformaten, Listen und Checklisten (Textpräfixe wie in der Swift-App), Wiki-Links mit
  Vorschlägen, eingebetteten Notizen, Eigenschaften, Suchen und Ersetzen, Widerrufen auch für Blockoperationen.
- Blöcke: Code (25 Sprachen, Ausführung, stdin, interaktiv, Tests, Ausführungsumgebung), Formel, Diagramm, Bild, PDF,
  Datei, Webseite, Audioaufnahme, Zeichnung, Datenbank-Abfrage (SQLite, PostgreSQL, MySQL), eingebettete Notiz.
- Versionsverlauf, Papierkorb, automatische Sicherungen, Konflikterkennung bei Änderungen durch andere Apps,
  doppelte Notiz-IDs werden aufgelöst, beschädigte Notizen werden beiseitegelegt statt gelöscht.
- Import aus Markdown und Apple Notizen, Export als Markdown und PDF/Drucken.
- Lokale KI über Ollama: Chat über Notiz oder Bibliothek mit Quellen, Textaktionen, Tag-Vorschläge, Antworten als Blöcke.
- Schnellerfassung, `folio://`-Links, Menüleistensymbol, Tagesnotizen, Vorlagen mit Platzhaltern.
- Selbsttest im echten Fenster ohne Sichtbarkeit (`FOLIO_SELFTEST=1`) und Testserver `folio-dev` für Browser-Tests.

### Noch nicht enthalten

Apple Intelligence, Image Playground, Widgets, Audio-Transkription, Numbers-Tabellen, PencilKit-Zeichnungen der
Swift-Version (bleiben erhalten), Terminal, Kernels, Sprachserver-Vervollständigung, Microsoft SQL Server, eigene
PDF-Textextraktion und der Update-Knopf. Siehe README.

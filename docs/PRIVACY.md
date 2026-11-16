# Datenschutz und Sicherheit

Stand: Folio 1.6.1r. Kurzfassung: **Folio hat kein Konto, keinen Server und keine Telemetrie.** Deine Notizen bleiben als
Dateien auf deinem Mac. Ins Netz geht Folio nur für Inhalte, die du selbst in eine Notiz einbettest.

## Wo Folio Daten speichert

| Was | Ort | Hinweis |
| --- | --- | --- |
| Notizen | Bibliotheksordner, Standard `~/Documents/Folio` | Eine `.folio`-Datei pro Notiz (LZFSE-komprimiert, **nicht verschlüsselt**), dasselbe Format wie die Swift-App. |
| Versionen, Papierkorb, Vorlagen, beschädigte Notizen, Ordnung | `<Bibliothek>/.folio/` (`history`, `trash`, `templates`, `damaged`, `organization.json`) | Versteckter Ordner, wandert mit der Bibliothek. Versionen und Papierkorb enthalten auch Inhalte, die du in der Notiz schon gelöscht hast. |
| Backups | `~/Library/Application Support/Folio/Backups` oder der gewählte Ordner | ZIP-Archive der Bibliothek, unverschlüsselt. |
| Wartende Schnellerfassungen | `<Bibliothek>/.folio/capture-queue` | Nur solange die Zielnotiz nicht beschreibbar ist. |
| Einstellungen und zuletzt geöffnete Notizen | `~/Library/Application Support/Folio Rust/settings.json` | Mit einer Testbibliothek (`FOLIO_LIBRARY`) stattdessen in deren `.folio/rust-settings.json`. |
| Offene Tabs und aufgeklappte Ordner | WebKit-Speicher von Folio | Nur die Kennungen der Notizen, keine Inhalte. |
| Passwörter von Datenbankverbindungen | macOS-Schlüsselbund (Dienst `de.daniel.folio.database`) | Nie in Notizen oder Einstellungen. |
| Anmeldungen in Web-Einbettungen (Cookies) | WebKit-Datenspeicher von Folio | Bleiben gespeichert, damit Seiten angemeldet bleiben. |

Folio verschlüsselt Notizen nicht selbst. Für Schutz auf dem Gerät sorgt FileVault; wer die Bibliothek mit iCloud Drive,
Dropbox o. Ä. synchronisiert, gibt die Dateien diesem Anbieter.

## Netzwerkzugriffe

| Wann | Wohin | Was übertragen wird |
| --- | --- | --- |
| Web-Block (Live-Ansicht) | Die Adresse, die in der Notiz steht | Die Seite wird wie in Safari geladen, sobald die Notiz offen ist. |
| Datenbank-Block | Der eingetragene Datenbankserver | Abfragen nur auf Knopfdruck. |
| KI (nur wenn eingeschaltet) | Ein lokales Ollama auf diesem Mac | Frage und passende Notizausschnitte; andere Rechner lehnt Folio ab. |
| Code-Blöcke | Was dein Programm tut | Folio selbst sendet nichts; ein ausgeführtes Programm darf aber wie jedes Programm ins Netz. |

Formeln (KaTeX) und Diagramme (Mermaid) werden **offline** aus der App heraus gerendert. Die Rust-Version prüft nicht
selbst nach Updates.

## Berechtigungen

| Berechtigung | Wofür |
| --- | --- |
| Mikrofon | Audioaufnahme in einer Notiz |
| Automation (Apple Events) | Import aus Apple Notizen |
| Lokales Netzwerk | Web-Blöcke mit `localhost` oder Geräten im eigenen Netz, lokales Ollama |

## Code-Ausführung

Code-Blöcke starten echte Programme (Python, Node, Swift, Shell, …) **mit deinen Benutzerrechten**. Das ist kein
Sandbox-Betrieb. Folio führt Code-Blöcke nie von selbst aus, nur auf Knopfdruck. Schutz vor Versehen: Zeitlimit
(Standard 30 Sekunden; erkannte Server ausgenommen), Ausgabelimit (1 MB), beim Abbrechen wird die ganze Prozessgruppe
beendet. Öffne Notizen anderer Leute wie Skripte aus dem Internet: Code erst lesen, dann ausführen.

## Signatur

Releases sind ad hoc signiert, bis eine Developer ID vorliegt (siehe [SIGNING.md](SIGNING.md)); die Release-Notizen
sagen jeweils, welcher Fall gilt.

## Was Folio nicht tut

- Keine Analyse, keine Telemetrie, keine Absturzberichte an Dritte, keine Werbung.
- Kein Konto und keine Synchronisation über eigene Server.
- Keine Weitergabe von Notizinhalten an KI-Anbieter.

## Sicherheitslücken melden

Siehe [SECURITY.md](../SECURITY.md).

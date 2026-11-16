# Sicherheit

## Unterstützte Versionen

Sicherheitskorrekturen gibt es für die jeweils neueste Version von Folio (Rust), z. B. `1.6.1r`.

## Eine Lücke melden

Bitte **kein öffentliches Issue** für Sicherheitslücken anlegen. Melde sie vertraulich über
[GitHub → Security → Report a vulnerability](https://github.com/gigafrosxh/folio-rust/security/advisories/new).

Hilfreich sind: betroffene Version, Schritte zum Nachstellen (am besten mit einer Beispielnotiz ohne private Daten) und
was ein Angreifer damit erreichen kann. Du bekommst innerhalb einer Woche eine Rückmeldung; die Korrektur erscheint als
eigenes Patch-Release.

## Was als Sicherheitslücke gilt

- Eine Notiz-, Markdown- oder Importdatei, die beim **Öffnen** Code ausführt, Dateien liest oder schreibt oder Daten ins
  Netz schickt (über das in [docs/PRIVACY.md](docs/PRIVACY.md) Beschriebene hinaus).
- Passwörter aus dem Schlüsselbund, die in Notizen, Logs, Backups oder Exporten landen.
- Ein Deep Link (`folio://…`), der ohne Zutun Inhalte verändert oder löscht.

Nicht dazu gehört, dass ein Code-Block nach Klick auf „Ausführen“ mit deinen Rechten läuft – das ist gewollt.

## Eingebaute Schutzmaßnahmen

- **Notizdateien**: Abschnitte über 1 GiB, Abschnitte über 128 MiB mit einem Kompressionsverhältnis über 1000:1, mehr als
  4 GiB entpackt oder mehr als 100.000 Anhänge werden abgelehnt, bevor etwas entpackt wird (`archive::limits`). Eine
  Datei, die mehr enthält als angegeben, gilt als beschädigt.
- **Links** (`folio://`): Nur `new` verändert die Bibliothek, und erst nach einer Rückfrage.
- **Importe**: Markdown liest Bilder nur aus seinem eigenen Ordner und übernimmt nur Links mit `http`, `https`, `mailto`
  und `folio`; beim Einfügen von HTML gilt dasselbe.
- **Geheimnisse**: Datenbank-Passwörter stehen nur im Schlüsselbund und werden aus Fehlermeldungen entfernt.
- **KI**: Ollama wird nur auf diesem Mac angesprochen (`localhost`, `127.0.0.1`, `::1`, `*.localhost`).
- **Testserver** `folio-dev` lauscht nur auf `127.0.0.1` und startet nur mit einer Testbibliothek.

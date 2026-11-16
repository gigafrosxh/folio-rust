# Signieren und Notarisieren

`scripts/build-app.sh` signiert mit der ersten „Developer ID Application“-Identität im Schlüsselbund (Hardened Runtime,
Zeitstempel, `Resources/Folio.entitlements`). Fehlt sie, wird ad hoc signiert. `scripts/make-installer.sh` notarisiert das
DMG, wenn zusätzlich das Notar-Profil `folio-notary` existiert.

## Einmalige Einrichtung

1. Apple Developer Program (bezahlte Mitgliedschaft).
2. Zertifikat **Developer ID Application** in den Anmelde-Schlüsselbund holen und prüfen:

   ```sh
   security find-identity -v -p codesigning | grep "Developer ID Application"
   ```

3. Zugang zum Notar-Dienst speichern:

   ```sh
   xcrun notarytool store-credentials folio-notary
   ```

## Entitlements

Folio braucht nur Apple Events (Import aus Apple Notizen), Mikrofon (Audioaufnahmen) und die beiden JIT-Ausnahmen, die
WebKit unter der Hardened Runtime für JavaScript benötigt.

## Ad hoc signierte Releases öffnen

Bis zur Notarisierung meldet macOS beim ersten Öffnen eines heruntergeladenen DMG, dass der Entwickler nicht verifiziert
ist. Öffnen über Rechtsklick › Öffnen oder unter Systemeinstellungen › Datenschutz & Sicherheit › „Trotzdem öffnen“.

# SC Architects Overlay — Anleitung

*Community-Tool für Star Citizen · v0.5.31 · Windows & Linux*

---

## Was ist das?

Das Architects Overlay ist ein schwebendes Fenster, das **über dem Spiel** liegt: Preise, Marktplatz, Handelsrouten, Inventar und Hangar — ohne das Spiel zu verlassen.

- Es startet **versteckt** und erscheint automatisch, sobald **Star Citizen läuft**
- `Strg+Shift+U` blendet es ein und aus
- Es liest nur **öffentliche Marktdaten** (UEX-API, SC-Wiki) — nichts aus eurem Spiel, keine Zugangsdaten vom Spiel, keine Telemetrie

---

## Installation

### Windows

1. Aktuelle `…_x64-setup.exe` laden: <https://github.com/SeBoOne/architects-overlay/releases/latest>
2. Ausführen. Windows warnt (die Datei ist **nicht signiert** — eine Signatur wäre unverhältnismäßig teuer): **„Mehr Informationen" → „Trotzdem ausführen"**. Das betrifft nur die erste Installation.
3. Fertig. Das Overlay startet versteckt und legt einen Desktop-Eintrag an.

**Updates:** Das Overlay prüft selbstständig auf neue Versionen. Klick auf „Jetzt aktualisieren" → das Setup läuft laut im Hintergrund und **startet das Overlay danach automatisch neu**. Kein manuelles Neunterladen nötig.

### Linux

Einzeiler (empfohlen — erkennt die Distribution und wählt `.deb` oder AppImage):

```bash
curl -fsSL https://raw.githubusercontent.com/SeBoOne/architects-overlay/main/scripts/install.sh | bash
```

Oder manuell: `…_amd64.deb` (Debian/Ubuntu — saubere Paket-Integration inkl. CLI) oder `…_amd64.AppImage` (alle anderen Distro, landet in `~/.local/bin` mit Desktop-Eintrag und Icon).

**Updates:** gleiche Update-Prüfung wie unter Windows. Bei `.deb` gibt es einen Hinweis mit dem apt-Befehl; bei AppImage und direkter Binary-Installation tauscht das Overlay die Datei selbst aus — danach das Overlay einmal neu starten.

---

## Erste Schritte

1. **Star Citizen starten** — das Overlay erscheint automatisch (solange SC nicht läuft, bleibt es hidden und frisst keine Ressourcen).
2. `Strg+Shift+U` zum Ein-/Ausblenden. `ESC` navigiert innerhalb des Overlays zurück (schließt es nicht).
3. Über die linke Rail wechselt ihr die Bereiche:

| Taste | Bereich |
|---|---|
| `F1` | Suche (fix) |
| `F2` | Marktplatz |
| `F3` | Handelsrouten |
| `F4` | Inventar |
| `F11` | Hangar |
| `F12` | Einstellungen (fix) |
| `F5` | Kontextsensitiver Resync (aktualisiert die Daten des aktuellen Bereichs) |

Alle Mode-Hotkeys (außer F1/F12) sind in den Einstellungen umbelegbar; `F5` ist fest reserviert.

4. **Optional — UEX-Konto verbinden** (Einstellungen → UEX): Damit könnt ihr eure **eigenen Listings und Trades** auf dem UEX-Marktplatz verwalten („Mein Shop"). App-Key anlegen auf <https://uexcorp.space> und im Overlay eintragen. Ohne Key funktioniert alles Öffentliche trotzdem.

---

## Typische Fragen

**Das Overlay erscheint nicht?** Es erscheint erst, wenn Star Citizen läuft (Prozess-Erkennung). Wenn SC läuft und es trotzdem fehlt: `Strg+Shift+U` drücken — es könnte nur versteckt sein.

**Woher kommen die Preise?** Öffentliche Marktdaten von UEX (uexcorp.space), Schiffsdaten von der SC-Wiki-API. Der Preisquelle-Fallback ist automatisch.

**Kostet was?** Nein. Open Source (MIT), Community-Projekt, keine Accounts, keine Tracking.

**Ist das Bann-gefährlich?** Das Overlay liest nichts aus dem Spiel und schreibt nichts hinein — es ist ein völlig unabhängiges Fenster mit Web-Daten, wie ein Browser auf dem zweiten Monitor, nur praktischer.

---

## Deinstallation

- **Windows:** Apps & Features → *SC Architects Overlay* entfernen. Nutzerdaten landen im App-Profilordner und bleiben bei Bedarf liegen.
- **Linux:** `bash <(curl … install.sh) --purge` entfernt das Tool inklusive Nutzerdaten; ohne `--purge` bleiben die Daten erhalten.

---

## Feedback

Fehler gefunden, etwas fehlt, Ideen? Meldet euch direkt bei **SeBoOne** — das Tool lebt vom Feedback der Mitspieler.

*Project site:* <https://github.com/SeBoOne/architects-overlay>

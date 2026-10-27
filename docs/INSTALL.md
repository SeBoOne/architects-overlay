# Installation — SC Architects Overlay

Plattform-Übersicht. Jede Sektion ist eigenständig; die Overlay-Funktion hinter
dem Spiel ist unter **Linux/Hyprland primär entwickelt** und dort am besten
ausgetestet. Windows-Builds entstehen über den GitHub-Actions-Mirror
(docs/CI-WINDOWS.md), macOS-Builds über denselben Kanal (C4 t_ee85f9d7).

---

## macOS (Apple Silicon / aarch64)

> **Status (C4 t_ee85f9d7):** Der macOS-Build ist über die CI vorbereitet,
> an v*-Tags entsteht ein `.app` + `.dmg`. Ein **Livetest bei Mitspielern ist
> noch ausstehend** — die Angaben hier gelten für den Stand der Vorbereitung,
> erwartete Restrisiken sind unter „Bekannte Einschränkungen“ notiert.

### Voraussetzungen

- macOS 13 (Ventura) oder neuer, **Apple Silicon (M1/M2/M3/M4)** — der Build
  ist `aarch64-apple-darwin` (siehe CI). Ein Intel-Mac läuft den aarch64-Build
  nicht nativ; ein Universal-Binary wird aktuell nicht gebaut.
- Die Tauri-App braucht keine besonderen Laufzeit-Abhängigkeiten (WebKit wird
  vom System gestellt).

### Installation

1. Von der Gitea-Release (https://git.sebo-home.de/sebo/architects-overlay/releases)
   die Datei `architects-overlay_<version>_aarch64.dmg` herunterladen.
2. `.dmg` öffnen und die `.app` in den **Programme**-Ordner ziehen.
3. Beim ersten Start meldet macOS „kann nicht geöffnet werden, da Apple es
   nicht auf Schadsoftware überprüfen kann“ — **keine App-Notarization**
   (privater Kontext, kein Apple-Developer-Programm, daher kein
   signed/notarized-Status). Gatekeeper-Freigabe notwendig:

```sh
# Signatur-Status/Target des heruntergeladenen Archivs löschen und erneut
# versuchen. Danach normal starten.
xattr -cr "/Applications/Architects Overlay.app"
```

   Alternativ: Kontextmenü auf der `.app` → **Öffnen** → nochmals **Öffnen**
   (single-file Opt-in) oder in den Systemeinstellungen → Datenschutz &
   Sicherheit die Warnung freigeben.

### Erste Schritte

- Beim Start legt das Overlay eine Systemleiste (Tray) an — dort läuft die
  Überblendung hinter einem laufenden Star Citizen automatisch.
- **Global-Shortcuts:** Das `global-shortcut`-Plugin funktioniert unter macOS
  nativ (Strg+Shift+U = Overlay umschalten, Strg+Shift+O = Click-through). Anders
  als unter Wayland ist auf macOS **kein** Socket-Fallback (`arch-overlay`)
  nötig — der Weg ist allein der globale Shortcut.
- **Prozesserkenung:** Star Citizen hat **keinen nativen macOS-Client**. Das
  Overlay erkennt das Spiel über `ps -ax -o command=`-Matches der bekannten
  Windows-Prozessnamen (StarCitizen.exe, RSI-Spectre …). Praktisch bedeutet
  das: Läuft SC in einem Kompatibilitäts-Layer (CrossOver/Wine) mit sichtbaren
  Exe-Namen, blendet sich das Overlay automatisch ein; sonst bleibt es über den
  Tray-„Beenden“-Pfad bzw. die globalen Shortcuts steuerbar, die Konfig
  erreicht man jederzeit übers Tray.

### Bekannte Einschränkungen

- **Keine Apple-Notarization / kein Code-Signing:** Der Download löst die
  Gatekeeper-Warnung aus; `xattr -cr` (bzw. Rechtsklick → Öffnen) ist der
  dokumentierte Workaround. macOS-Versionen können die Freigabe pro Update
  erneut verlangen.
- **Nur aarch64:** Kein Universal-Binary, kein Intel-x86_64-Nativbetrieb.
- **Kein Livetest durchgeführt:** Der macOS-Pfad ist CI-vorbereitet
  (baut sauber), aber noch nicht auf echter Apple-Hardware bei Mitspielern
  verifiziert. Insbesondere Gehäuse-/Geometrie-Besonderheiten (das Overlay
  läuft als Vollbild-Transparent-Fenster über dem Monitor) sind unter macOS
  noch unbestätigt (z. B. Platzierung auf dem Pointer-Monitor ist nicht
  implementiert — fällt auf Zentrierung zurück).
- **Prozesserkenung begrenzt:** Ohne nativen SC-Client zählt praktisch nur der
  Kompatibilitäts-Layer-Fall (CrossOver). Ein nativ-fensterloser SC-Exe unter
  macOS existiert nicht.
- **Cache/Konfig:** liegen im macOS-Konfig-Ordner
  (`~/Library/Application Support/com.sebo.architects-overlay/`); die
  `config.toml` (UEX-API-Key) wird mit 0600 angelegt.

---

## Windows (x64)

- Setup: NSIS-Installer (`architects-overlay_<version>_x64-setup.exe`) oder MSI
  aus der Gitea-Release.
- Build-Pipeline: docs/CI-WINDOWS.md. Beim Start erwartet Windows ggf. einen
  SmartScreen-Hinweis (kein signiertes Installer-Zertifikat) — „Trotzdem
  ausführen“.
- Cache/Konfig unter `%APPDATA%/com.sebo.architects-overlay/`.

---

## Linux (Hyprland / X11)

- deb/AppImage aus der Gitea-Release; Setup und Hyprland-Einrichtung:
  docs/WAYLAND.md. Primär ausgetestete Plattform.
# Installation — SC Architects Overlay

Plattform-Übersicht. Jede Sektion ist eigenständig; die Overlay-Funktion hinter
dem Spiel ist unter **Linux/Hyprland primär entwickelt** und dort am besten
ausgetestet. Linux- und Windows-Builds entstehen via GitHub Actions auf dem
öffentlichen Repo (bei Tag-Push) — Flow: docs/RELEASE.md; die alte
Selfhosted-Runner-Doku steht archiviert in docs/CI-WINDOWS.md.

---

## Schnell-Start Linux — Einzeiler

```sh
curl -fsSL https://raw.githubusercontent.com/SeBoOne/architects-overlay/main/scripts/install.sh | bash
```

Das Script erkennt die Distro automatisch:

- **Debian-basiert** (apt vorhanden): lädt das `.deb` aus dem neuesten Release
  und installiert es via `apt` — Abhängigkeiten, `arch-overlay`-CLI,
  Desktop-Datei und Icons kommen aus dem Paket.
- **Alle anderen Distros** (Arch, Fedora, …): AppImage nach `~/.local/bin`,
  Desktop-Datei nach `~/.local/share/applications`, Icon nach
  `~/.local/share/icons/hicolor/128x128/apps`, plus CLI-Wrapper
  `arch-overlay` (im PATH).

> **Voraussetzung:** Releases müssen öffentlich ladbar sein — der Default
> (öffentliches GitHub-Mirror-Repo, S5) erfüllt das. Für die private
> x86_64 erforderlich.

### Flags

| Flag | Wirkung |
| --- | --- |
| *(keine)* | Installieren (interaktiv, mit Rückfrage) |
| `--yes` / `-y` | Ohne Rückfrage (für `curl \| bash`) automatisch gesetzt |
| `--version vX.Y.Z` | Bestimmte Release-Version pinnen |
| `--uninstall` | App entfernen — Nutzerdaten bleiben |
| `--purge` | App **inkl.** Nutzerdaten/Cache entfernen
  (`~/.local/share/com.sebo.architects-overlay`) |
| `--help` | Hilfe |

### CLI

Nach der Installation steht `arch-overlay <toggle|show|hide|clickthrough|state>`
zur Verfügung (steuert die laufende Overlay-Instanz über ihren Control-Socket;
Wayland-Fallback statt globaler Shortcuts, siehe docs/WAYLAND.md).

---

## macOS (Apple Silicon / aarch64)

> **Status: PAUSIERT.** macOS-Builds werden derzeit **nicht gebaut** (der
> `macos-latest`-Job in `.github/workflows/build.yml` ist deaktiviert und
> bei Bedarf reaktivierbar — siehe „Was NICHT mehr passiert“ in
> docs/RELEASE.md). Die folgenden Angaben gelten für den Stand der
> Vorbereitung; ein **Livetest bei Mitspielern ist noch ausstehend**,
> erwartete Restrisiken sind unter „Bekannte Einschränkungen“ notiert.

### Voraussetzungen

- macOS 13 (Ventura) oder neuer, **Apple Silicon (M1/M2/M3/M4)** — der Build
  ist `aarch64-apple-darwin` (siehe CI). Ein Intel-Mac läuft den aarch64-Build
  nicht nativ; ein Universal-Binary wird aktuell nicht gebaut.
- Die Tauri-App braucht keine besonderen Laufzeit-Abhängigkeiten (WebKit wird
  vom System gestellt).

### Installation

1. Von der GitHub-Release (https://github.com/SeBoOne/architects-overlay/releases)
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
  aus der GitHub-Release (https://github.com/SeBoOne/architects-overlay/releases).
- Build-Pipeline: docs/CI-WINDOWS.md. Beim Start erwartet Windows ggf. einen
  SmartScreen-Hinweis (kein signiertes Installer-Zertifikat) — „Trotzdem
  ausführen“.
- Cache/Konfig unter `%APPDATA%/com.sebo.architects-overlay/`.
- **Prozesserkenung im Hintergrund:** Das Overlay fragt alle 5 s
  `tasklist /FO CSV /NH` mit dem Windows-Flag `CREATE_NO_WINDOW` ab — es
  flackert also **kein** Konsolenfenster mehr auf.

### Test ohne Star Citizen (Fake-Prozess)

Zum Prüfen des Game-Gates ohne installiertes SC reicht eine umbenannte
Exe-Kopie — die Erkennung matcht das **IMAGE-Feld** der tasklist-Ausgabe
(case-insensitive `STARCITIZEN.EXE`):

```bat
mkdir C:\Temp 2>nul
copy %SystemRoot%\System32\notepad.exe C:\Temp\StarCitizen.exe
start "" C:\Temp\StarCitizen.exe
```

Danach: Overlay-Game-Gate schaltet nach spätestens ~5 s ein; beenden des
Fensters schaltet das Gate wieder aus. Zur Diagnose, falls das Gate **nicht**
aufgeht:

```bat
:: 1) Läuft der Fake-Prozess überhaupt unter dem Namen?
tasklist /FO CSV /NH | findstr /i "StarCitizen.exe"
:: Erwartet: eine Zeile "StarCitizen.exe","<pid>",...

:: 2) Läuft die Exe unter anderem Namen (z.B. `start StarCitizen.exe` →
::    Titel statt Datei, oder ein Launcher-Renaming)? Besser:
wmic process where "ExecutablePath like 'C:\\Temp\\%'" get Name,ExecutablePath

:: 3) Hat Defender/SmartScreen die Kopie stummgeschickt oder gelöscht?
::    → prüfen, ob C:\Temp\StarCitizen.exe noch existiert, und
::    Defender-Log (SCHUTZVERLAUF) bzw. Quarantäne checken.
```

Die Zeile aus Schritt 1 **ist** das Match-Kriterium des Overlays: enthält sie
`StarCitizen.exe`, erkennt das Overlay den Prozess. Schlägt Schritt 1 fehl
(Prozess heißt anders oder ist sofort wieder beendet), kann das Overlay per
Definition nichts sehen. Das Overlay-Log protokolliert jede Änderung des
Detektionszustands (`game-detection: running=true/false`), sodass sich
beim Livetest direkt ablesen lässt, was erkannt wurde.

---

## Linux (Hyprland / X11)

- deb/AppImage aus der GitHub-Release (https://github.com/SeBoOne/architects-overlay/releases);
  Setup und Hyprland-Einrichtung:
  docs/WAYLAND.md. Primär ausgetestete Plattform.
- **Updates:** Der Auto-Updater behandelt alle drei Installationsformen:
  AppImage (atomarer Selbst-Swap + Relaunch), `.deb` (kein Self-Update —
  apt-Hinweis mit Download-Pfad) und **Bare-Binary** (das Release trägt das
  rohe Binary als Asset `architects-overlay_<version>_linux64`; Update =
  atomarer Swap der exe, danach Hinweis zum manuellen Neustart — kein
  Auto-Restart unter Linux). Details: docs/UPDATER.md.

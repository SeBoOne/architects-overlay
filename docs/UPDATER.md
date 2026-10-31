# Auto-Updater — UC t_d323be7e

Update-Check gegen die Releases-API des öffentlichen GitHub-Mirrors (S5:
`https://api.github.com/repos/SeBoOne/architects-overlay/releases/latest`, ohne
Auth) + on-demand Update aus der
laufenden App (Windows NSIS + Linux AppImage). Motivation: die unsignierte
.exe löst bei jedem manuellen Download eine SmartScreen-Warnung aus — der
Updater lädt/installiert das Update selbst.

## Update-Check (beide Plattformen)

- Beim Start + alle N Stunden (`[updater] interval_hours`, Default **6 h**,
  Clamp 1..24 h). Konfigurierbar in der Config-View („Aktualisierung“) oder
  direkt in `config.toml`:
  ```toml
  [updater]
  enabled = true
  interval_hours = 6
  repo = "SeBoOne/architects-overlay"
  base_url = "https://api.github.com"
  ```
- Genau **EIN GET** auf `repos/{repo}/releases/latest` — GitHub-Form ohne
  `/api/v1` (S5); eine Gitea-Instanz als `base_url` bleibt möglich (dort
  `/api/v1/repos/…`). Sonst KEIN
  Netzverkehr (kein Telemetrie/Phoning). Bei einem vorhandenen Update kommt
  optional EIN GET auf das `SHA256SUMS`-Sidecar hinzu.
- Versionsvergleich: semver mit Pre-release-Suffixen (`v0.6.0-rc.1` ist
  KEIN Update für eine Release-Version; `v0.6.0-rc.1` → `v0.6.0-rc.2`/`v0.6.0`
  ist eines). Unlesbare Tags werden als „nicht neuer“ behandelt.
- HTTPS-only: eine Base-URL ohne `https://` wird abgelehnt (UpdaterError::InsecureUrl).

## UI (Config-View „Aktualisierung“)

- Auto-Check an/aus + Intervall-Chips (2/6/12/24 h).
- Nach einem Check: Note „Kein Update gefunden — v0.5.13 ist aktuell.“ oder
  „Update v0.5.14 verfügbar.“ + Button „Jetzt aktualisieren“.
- **On-demand only:** nichts wird geladen, installiert oder neu gestartet
  ohne Klick auf „Jetzt aktualisieren“ (Karte Umsetzung 2).
- „Jetzt prüfen“ triggert den Check sofort.

## Windows-Update-Pfad (NSIS) — dokumentierte Entscheidung

Die laufende exe kann sich nicht selbst ersetzen. Zwei Varianten standen
zur Wahl (Karte Umsetzung 3):

1. App beendet sich, Setup startet danach (kein
   nachgelagerter Prozess möglich).
2. **App startet das Setup als CHILD-PROZESS mit `/S` (silent) und beendet
   sich dann selbst sauber — DAS IST IMPLEMENTIERT.**

Ablauf: `setup.exe` herunterladen → SHA256 verifizieren (s. u.) →
`setup.exe /S` detached starten (detached, kein Konsolenfenster — gleiche
Mechanik wie der hide_console-Fix t_d323b29e) → App `exit(0)`. Der
NSIS-Installer übernimmt das Selbst-Erstzen (Tauri-NSIS schließt eine
eventuell noch laufende Instanz ab und ersetzt die exe). **Kein Auto-Restart
nach dem Setup** — der Nutzer startet das Overlay selbst neu (Startmenü-
Eintrag des Setups). Damit ist nach dem Setup KEIN Fenster mehr offen, das
auf Installer-Exit warten müsste.

## Linux-Update-Pfad

- **AppImage** (env `APPIMAGE` gesetzt oder `.AppImage`-Pfad): neues AppImage
  herunterladen → SHA256 verifizieren → atomar ersetzen: alte Datei
  umbenennen (`.old`) → neue Datei rename → exec-Rechte (0o755) → alte
  Datei löschen → Overlay relaunchen + Exit. Das neue AppImage liegt
  ZUSAMMEN mit dem alten (gleiche Dateisystem — Rename ist garantiert
  atomar). Nutzerdaten (App-Config-Dir) sind unangetastet.
- **.deb-Installationen**: kein Self-Update — die App führt KEINE root-
  Aktionen aus (Umsetzung 4). Die UI zeigt einen apt-Hinweis mit dem
  Download-Pfad + `sudo apt install /tmp/<file>.deb`.

## Integrität (Umsetzung 5)

- Jeder Download wird VOR dem Installieren per SHA256 verifiziert.
- Prüfsummen-Quellen: `SHA256SUMS`-Sidecar (CI lädt es an v*-Tags hoch —
  Linux- UND Windows-Job) oder Release-Body. Fehlt BEIDES für das Asset:
  **Download wird abgelehnt** (`NoChecksum`), kein Trust-on-Download.
- Mismatch → temporäre Datei verworfen, Update abgelehnt.

## Wichtig: Sichtbarkeit der Releases

Der Check ist ein UNAUTHENTIFIZIERTES GET auf
`https://api.github.com/repos/SeBoOne/architects-overlay/releases/latest`
(S5: das öffentliche GitHub-Mirror-Repo ist die Release-Quelle — es ist
public, der Check funktioniert damit ohne Token). Damit der Updater funktioniert, muss die Release-Seite
der Instanz anonym lesbar sein (GitHub-Mirror ist public; Gitea: Repo public bzw. „public“-Rechte für
Releases) — auch die Asset-Download-URLs müssen ohne Token ladbar sein.
Wird ein Token nötig, gehört er in die `[updater]`-Sektion (Konfig-only),
nicht ins Frontend.

## Fehlerpfade (Umsetzung 6)

- Kein Netz / Rate-Limit (429) / HTTP-Fehler: Check-Note zeigt den Fehler,
  die alte Version bleibt intakt.
- Halber Download: `.part`-Datei wird verworfen (Download-Größenlimit
  500 MiB, Content-Length-Frühcheck).
- Download fehlgeschlagen: ALTE Version bleibt unangetastet (Windows:
  Setup wird nie gestartet; Linux: `.old`-Rename wird gerollt, bevor die
  neue Datei am Platz ist).

## Komponenten

- `crates/updater/` — versionslose Kernlogik: semver-Vergleich, Releases-API-Client (GitHub/Gitea),
  SHA256-Integrität, Asset-Auswahl/Plan, Streaming-Download (11 Unit-Tests).
- `crates/settings/` — `[updater]`-Sektion (`enabled`, `interval_hours`,
  `repo`, `base_url`).
- `src-tauri/src/updater.rs` — State + `updater_status` / `updater_check_now`
  / `updater_install` Commands + periodischer Check-Thread.
- `.gitea/workflows/build.yml` — SHA256SUMS-Sidecar an v*-Releases.
- Frontend: Config-View-Sektion „Aktualisierung“ (Chip + Button + Note).

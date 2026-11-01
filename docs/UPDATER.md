# Auto-Updater — UC t_d323be7e

Update-Check gegen die Release-Seite des öffentlichen GitHub-Mirrors — seit
TG OHNE Releases-API: der Check holt `latest.json` über den Redirect-Pfad
`https://github.com/SeBoOne/architects-overlay/releases/latest/download/latest.json`
(NICHT rate-limited — kein API-Endpoint). Fallback für alte Releases ohne
`latest.json`: Releases-API (`releases/latest`, ohne Auth). + on-demand
Update aus der
laufenden App (Windows NSIS + Linux AppImage). Motivation: die unsignierte
.exe löst bei jedem manuellen Download eine SmartScreen-Warnung aus — der
Updater lädt/installiert das Update selbst.

## Update-Check (beide Plattformen)

- Beim Start + alle N Stunden (`[updater] interval_hours`, Default **6 h**,
  Clamp 1..24 h). Genau **EIN GET** auf `latest.json`
  (`releases/latest/download/latest.json` — kein API-Call; jede
  Release-Pipeline hängt die Datei als Asset an, CI-Job `latest-json`).
  Fallback: fehlt die Datei (alte Releases vor TG), EIN GET auf die
  Releases-API — bei 429 dort: „Rate-Limit, später erneut“ statt generischem
  Fehler. Asset-Downloads laufen über `releases/download/<tag>/<name>` (ebenfalls
  ohne API). Konfigurierbar in der Config-View („Aktualisierung“) oder
  direkt in `config.toml`:
  ```toml
  [updater]
  enabled = true
  interval_hours = 6
  repo = "SeBoOne/architects-overlay"
  base_url = "https://api.github.com"
  ```
- Genau **EIN GET** auf `latest.json` (Redirect-Pfad, TG). Fallback (alte
  Releases ohne `latest.json`): EIN GET auf `repos/{repo}/releases/latest` —
  GitHub-Form ohne `/api/v1` (S5); eine Gitea-Instanz als `base_url`
  bleibt möglich (dort `/api/v1/repos/…` und direkt API, da Gitea den
  Redirect-Pfad nicht hat). Sonst KEIN Netzverkehr (kein Telemetrie/
  Phoning). Checksummen kommen aus `latest.json` (SHA256 pro Asset); beim
  API-Fallback optional EIN GET auf das `SHA256SUMS`-Sidecar.
- Versionsvergleich: semver mit Pre-release-Suffixen (`v0.6.0-rc.1` ist
  KEIN Update für eine Release-Version; `v0.6.0-rc.1` → `v0.6.0-rc.2`/`v0.6.0`
  ist eines). Unlesbare Tags werden als „nicht neuer“ behandelt.
- HTTPS-only: eine Base-URL ohne `https://` wird abgelehnt (UpdaterError::InsecureUrl).

## UI (Config-View „Aktualisierung“)

- Auto-Check an/aus + Intervall-Chips (2/6/12/24 h).
- Nach einem Check: Note „Kein Update gefunden — v{aktuell} ist aktuell.“
  oder „Update v{neu} verfügbar.“ + Button „Jetzt aktualisieren“.
- **On-demand only:** nichts wird geladen, installiert oder neu gestartet
  ohne Klick auf „Jetzt aktualisieren“ (Karte Umsetzung 2).
- „Jetzt prüfen“ triggert den Check sofort.

## Windows-Update-Pfad (NSIS) — dokumentierte Entscheidung

Die laufende exe kann sich nicht selbst ersetzen. Zwei Varianten standen
zur Wahl (Karte Umsetzung 3):

1. App beendet sich, Setup startet danach (kein
   nachgelagerter Prozess möglich).
2. **App startet das Setup als CHILD-PROZESS mit `/S /R` (silent + Auto-
   Restart nach der Installation) und beendet sich dann selbst sauber —
   DAS IST IMPLEMENTIERT.**

Ablauf: `setup.exe` herunterladen → SHA256 verifizieren (s. u.) →
`setup.exe /S /R` detached starten (silent + Reboot-Flag des NSIS-Templates:
die App wird nach Abschluss der Installation **automatisch neu gestartet**;
detached, kein Konsolenfenster — gleiche Mechanik wie der hide_console-Fix
t_d323b29e) → App `exit(0)`. Der NSIS-Installer übernimmt das Selbst-Erstzen
(Tauri-NSIS schließt eine eventuell noch laufende Instanz ab und ersetzt die
exe) und startet das Overlay danach selbst wieder — implementiert in
`src-tauri/src/updater.rs` (`spawn_nsis_silent`, Flags `CREATE_NO_WINDOW |
DETACHED_PROCESS`).

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
- **Bare-Binary (TH t_df9646a3)** — die exe wurde direkt nach `~/.local/bin`
  (oder sonstwo außerhalb `/usr`/`/opt`) kopiert, weder AppImage noch .deb:
  das Release trägt das rohe Binary als Asset `architects-overlay_<version>_linux64`
  (CI: Linux-Job lädt es mit exec-Rechten hoch; SHA256 via latest.json/
  SHA256SUMS wie die anderen Assets). Ablauf: Download → SHA256 verifizieren
  → atomares Rename über die laufende exe (rename über die aktive exe geht
  auf Linux) → `chmod 0o755`. **Kein Auto-Restart, kein systemctl-Eingriff
  aus der App** — die Note zeigt: „Update v{X} installiert — Neustart des
  Overlays anwenden lassen (systemd restart oder Overlay schließen/öffnen).“
  Die App läuft nach dem Swap weiter (Plan::LinuxBin, kind `linux-bin`).

## Integrität (Umsetzung 5)

- Jeder Download wird VOR dem Installieren per SHA256 verifiziert.
- Prüfsummen-Quellen (TG): `latest.json` (SHA256 pro Asset, CI-Job
  `latest-json` — der Normalpfad), beim API-Fallback `SHA256SUMS`-Sidecar
  oder Release-Body. Fehlt ALLES für das Asset:
  **Download wird abgelehnt** (`NoChecksum`), kein Trust-on-Download.
- Mismatch → temporäre Datei verworfen, Update abgelehnt.

## Wichtig: Sichtbarkeit der Releases

Der Check ist ein UNAUTHENTIFIZIERTES GET auf
`https://github.com/SeBoOne/architects-overlay/releases/latest/download/latest.json`
(TG: Redirect-Pfad, kein API-Endpoint — damit nicht von den 60 req/h
pro IP betroffen; Fallback: Releases-API wie oben).
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
- `.github/workflows/build.yml` — TG: Job `latest-json` (latest.json +
  SHA256SUMS an v*-Releases; SHA256-Manifests aus Linux/Windows-Jobs).
- Frontend: Config-View-Sektion „Aktualisierung“ (Chip + Button + Note).

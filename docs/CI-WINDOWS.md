> **ARCHIV (2026-10-31) — Historie.** Gitea-CI ist stillgelegt — Builds laufen
> via GitHub-Actions auf hosted Runnern (siehe docs/RELEASE.md). Diese Doku
> beschreibt den alten selfhosted-Runner-Weg (fuer Reaktivierung aufbewahrt).
>
# Windows-Runner für Gitea Actions (architects-overlay) (Historie)

> **STATUS: AUFGESCHOBEN (2026-10-21)** — kein permanent laufender Windows-PC
> verfügbar. Der Windows-Job ist in `build.yml` mit `if: false` geparkt;
> Linux-Builds (deb/AppImage) laufen normal weiter.
> **Geplanter Weg später: GitHub-Actions-Mirror** (private Repo; GitHub stellt
> Windows-Runner, kein eigener PC nötig — Details am Ende dieses Dokuments,
> Abschnitt 8). Die Selbsthosted-Anleitung unten bleibt als Alternative
> gültig, falls doch ein Windows-PC dauerhaft läuft.

Der Windows-Job in `.gitea/workflows/build.yml` trägt das Label (Historie)
`windows-selfhosted`. Solange kein Runner mit genau diesem Label registriert
ist, wartet der Job einfach in der Queue — es wird **nichts rot**. Erst wenn
der Runner online ist, baut er bei jedem `v*`-Tag den Windows-Installer
(NSIS/MSI) und hängt ihn an die Gitea-Release. (Historie)

Am Ende dieses Dokuments steht das Ziel: **Runner online → Windows-Exe
entsteht automatisch bei Tags.**

---

## 1. Voraussetzungen (einmalig, auf dem Windows-PC)

| Komponente | Warum | Installationsweg |
|---|---|---|
| **Git for Windows** | `actions/checkout` braucht `git` (liefert auch Git-Bash, das der Workflow nutzt) | `winget install Git.Git` |
| **Node.js 22 LTS** | Build-Laufzeit für Frontend + pnpm | `winget install OpenJS.NodeJS.LTS` — danach `node -v` prüfen |
| **rustup + MSVC-Werkzeugkette** | Tauri/Rust-Build für Windows | siehe Schritt 1.1 |
| **MSVC C++ Build Tools** | Linker (`link.exe`) für rustc-MSVC | `winget install Microsoft.VisualStudio.2022.BuildTools --override "--add Microsoft.VisualStudio.Workload.VCTools --passive --norestart"` |
| **WebView2 Runtime** | Laufzeit der Tauri-App (Win10/11 meist schon da) | Nur falls fehlt: https://developer.microsoft.com/microsoft-edge/webview2/ (Evergreen Installer) |
| **NSIS** | Installer-Bundler | Tauri v2 lädt NSIS automatisch beim ersten Build. Optional explizit: `winget install NSIS.NSIS` |

Alle Versionen prüfen, bevor es weitergeht (neues Terminal!):

```
node -v        # v22.x
npm -v
rustc -V
cargo -V
git --version
```

### 1.1 rustup mit MSVC-Toolchain

`rustup-init.exe` von https://rustup.rs herunterladen und ausführen. Im
Prompt **"Default host triple"** und **"Default toolchain"** bei
`stable-msvc` belassen (default), Profil *minimal* wählen (Alternativen: 2).
Danach:

```
rustc -V        # z. B. rustc 1.99.1 (stable-x86_64-pc-windows-msvc)
```

Falls die MSVC-Werkzeuge (link.exe) fehlen, meldet cargo beim ersten Build
`link.exe not found` — dann Schritt „MSVC C++ Build Tools" oben nachholen und
das Terminal neu öffnen.

---

## 2. act_runner.exe herunterladen

Von den offiziellen Release-Seiten (das Projekt heißt seit dem Rename offiziell
`gitea/runner`, die Datei **`gitea-runner-x.y.z-windows-amd64.exe`** — die (Historie)
Umbenennung per Hand macht jede Version kompatibel):

- https://gitea.com/gitea/runner/releases  (offiziell) (Historie)
- oder GitHub-Spiegel: https://github.com/Gitea/runner/releases (Historie)

**Mindestversion: v3.0.0** — erst ab dort unterstützt Gitea-Actions (Historie)
`actions/upload-artifact@v4`, das der Workflow nutzt (go-gitea/gitea#35706). (Historie)

In einen eigenen Ordner legen und die Datei zu `act_runner.exe` umbenennen:

```
C:\gitea-runner\ (Historie)
   act_runner.exe
```

**Wichtig:** Alle folgenden Befehle aus diesem Ordner ausführen — die
Registrierung legt `.runner` und `config.yaml` neben der EXE ab.

---

## 3. Registrierungstoken aus Gitea holen (Historie)

1. Browser: https://git.sebo-home.de/sebo/architects-overlay (Historie)
2. **Settings → Actions → Runners** (Repo-eigene Runner)
3. **Create new runner** → das angezeigte Einmal-Token kopieren (nur kopieren,
   nicht speichern — es verfällt nach Gebrauch).

Das Token darf nur in den nächsten Befehl — es gehört **nie** ins Repo oder
in die Workflow-Datei. Steht hier nur als Platzhalter:

```
TOKEN=__PLATZHALTER_TOKEN__
```

---

## 4. Runner registrieren (Label: windows-selfhosted)

In `C:\gitea-runner` (PowerShell): (Historie)

```
.\act_runner.exe register `
  --instance https://git.sebo-home.de ` (Historie)
  --token __PLATZHALTER_TOKEN__ `
  --name sebo-winpc `
  --labels windows-selfhosted `
  --no-interactive
```

- `--labels` **muss exakt** `windows-selfhosted` sein — nur mit diesem Label
  findet der Windows-Job aus `build.yml` den Runner.
- Prüfen: `.\act_runner.exe` (ohne Argumente) → „Runner is stopped"/Kein
  Fehler = Registrierung ok. Es erscheint `C:\gitea-runner\.runner`. (Historie)

---

## 5. Als Autostart einrichten (läuft dauerhaft)

Der Runner pollt Gitea aktiv — es muss kein Port freigegeben werden, eine (Historie)
Firewall-Freigabe für ausgehendes HTTPS reicht (und selbst die fragt Windows
beim ersten Lauf ab).

### Variante A: Task Scheduler (empfohlen, startet beim Anmelden)

Datei `C:\gitea-runner\start-runner.bat` anlegen: (Historie)

```
@echo off
cd /d C:\gitea-runner (Historie)
act_runner.exe daemon
```

Task anlegen (einmal):

```
schtasks /Create /TN "Gitea Act Runner" /TR "C:\gitea-runner\start-runner.bat" /SC ONLOGON /RL HIGHEST /F (Historie)
```

Danach abmelden/anmelden oder sofort testen:

```
schtasks /Run /TN "Gitea Act Runner" (Historie)
```

### Variante B: Autostart-Ordner

`Win+R` → `shell:startup` → Verknüpfung auf `start-runner.bat` (aus Variante A)
hineinlegen. Startet ebenfalls bei jeder Anmeldung.

Status des Daemons: Logs erscheinen im Konsolenfenster; mehr Debug mit
`act_runner.exe daemon --config config.yaml` und `--verbosity trace`.

---

## 6. Online-Status prüfen

Gitea → **Settings → Actions → Runners**: `sebo-winpc` muss **online** (Historie)
anzeigen (grüner Punkt). Das passiert wenige Sekunden nach dem Daemon-Start.

---

## 7. Tag pushen → Windows-Exe entsteht

Ablauf ab jetzt vollautomatisch:

1. Tag pushen, z. B. `v0.1.0` (oder ein weiterer Runner-Online-Check über
   einen bestehenden v-Tag).
2. Gitea Actions baut den Linux-Job (deb/AppImage) und legt die Gitea-Release (Historie)
   an.
3. Der Windows-Job läuft jetzt auf deinem PC und erzeugt
   `target/release/bundle/nsis/architects-overlay_<version>_x64-setup.exe`
   (+ MSI). NSIS wird von Tauri v2 automatisch mitgeladen.
4. Installer und MSI landen als Release-Assets auf
   https://git.sebo-home.de/sebo/architects-overlay/releases und zusätzlich (Historie)
   als CI-Artifact des Runs.

Damit ist der Kreislauf geschlossen:

**Runner online → Windows-Exe entsteht automatisch bei Tags.**

---

## 8. Alternative: Windows-Builds via GitHub-Actions-Mirror (geplanter Weg)

Kein eigener Windows-PC nötig — GitHub stellt Managed Windows-Runner
(`windows-latest`, auch für private Repos; Windows-Minuten zählen 2×, im
Free-Tier 2000 Minuten/Monat, ein Tauri-Build braucht ~10 Min ≈ 20 Windows-Minuten).

1. GitHub-Repo anlegen (private) und Gitea→GitHub spiegeln (Gitea: (Historie)
   Repo-Settings → Mirror → Push-Mirror auf GitHub; oder Ad-hoc-Remote).
2. Eine GitHub-Variante des Workflows anlegen (`.github/workflows/build.yml`
   im selben Repo — Gitea ignoriert `.github/`): identisch zum Linux-Job, (Historie)
   der Windows-Job dort mit `runs-on: windows-latest` und **ohne** `if: false`.
3. Windows-Secrets: `GITEA_TOKEN` (Repo-Write) als GitHub-Secret anlegen, (Historie)
   damit die Release-Assets zurück auf git.sebo-home.de hochgeladen werden (Historie)
   (gleiche API-Logik wie im Linux-Job).

   **Konto-Token (Stand 2026-10-21, angelegt als `gh-ci-release`):**
   Scope ist bewusst **minimal** auf `write:repository` beschränkt (reicht für
   das Anlegen/Ergänzen von Releases samt Asset-Upload zurück zu
   git.sebo-home.de/sebo/architects-overlay); kein `write:user`/Admin-Scope. (Historie)
   Das Secret trägt exakt den Namen `GITEA_TOKEN`. (Historie)

   **Ablauf & Rotation:** Gitea-Konto-Tokens haben **kein** festes Ablaufdatum — (Historie)
   sie laufen nicht von selbst ab. Einsatzdauer also bewusst begrenzen:
   - **Jährlich** (oder sobald der Token einem Dritten auffällt / geleakt ist)
     rotieren: in Gitea → Einstellungen → Anwendungen den alten Token löschen, (Historie)
     unter exakt dem Namen `gh-ci-release` einen neuen mit Scope
     `write:repository` anlegen, dann
     `gh secret set GITEA_TOKEN --repo SeBoOne/architects-overlay` mit den (Historie)
     neuen Token-Wert ausführen.
   - Das Gitea-Web-UI zeigt für jede Anwendung den Zeitpunkt der **letzten (Historie)
     Nutzung** (`updated_at`) — dort erkennen, ob ein Token noch gebraucht wird.
   - Niemals den Token in Versionskontrolle, Issues oder CI-Logs ablegen; nur
     als GitHub-Secret hinterlegen.
4. Tags an GitHub spiegeln (Push-Mirror mirrored Tags), GitHub baut NSIS/MSI,
   lädt die Windows-Assets an die Gitea-Release. (Historie)

### 8.1 Stand 2026-10-21: GitHub-Push-Mirror ist aktiv

Die Spiegelschaltung wurde umgesetzt und getestet:

- **GitHub-Repo (privat):** `SeBoOne/architects-overlay`
  (https://github.com/SeBoOne/architects-overlay) — dient als reiner
  Mirror des Gitea-Repos `sebo/architects-overlay` für die (Historie)
  GitHub-Actions-Windows-Builds.
- **Push-Mirror in Gitea** (Repo-Settings → Mirror → Push-Mirror): (Historie)
  - Remote: `https://github.com/SeBoOne/architects-overlay.git`
  - Authentifizierung: HTTPS über ein SeBoOne-GitHub-Token (mit `repo`-Scope),
    das Gitea im Mirror-Eintrag speichert. **Wichtig:** `gh-auth`/Account-Token (Historie)
    wird nicht in Repo-Dateien oder das Repo `.git/config` gelegt.
  - Interval: `1h`, `SyncOnCommit: true` → jeder Push/Tag löst einen sofortigen
    Sync aus (zuverlässiger als nur das Stunden-Intervall).
  - Es werden **alle Branches und Tags** gespiegelt (`--mirror=push`,
    konfiguriert als `+refs/heads/*` und `+refs/tags/*`).
- **Getestet (2026-10-21):** Ein Test-Tag (`test-mirror-…`) wurde zu Gitea (Historie)
  gepusht und erschien **sofort** im GitHub-Repo; seine Löschung wurde
  ebenfalls bidirektional nachgezogen. `v0.1.0`-Tag und alle Branches sind
  im GitHub-Repo vorhanden.
- **Hinweis:** Gitea **1.27 akzeptiert keine SSH-URLs** als Push-Mirror (Historie)
  (nur `http/https/git`) — SSH-Deploy-Keys funktionieren dafür nicht, die
  Authentifizierung läuft zwingend über den HTTPS-Token.

**Token-Rotation beim GitHub-Spiegeltoken** (separat vom Gitea-Token): (Historie)
Bei Rotation auf dem GitHub-Konto einen neuen Token (minimal `repo`-Scope)
erzeugen und im **Gitea**-Push-Mirror-Eintrag aktualisieren (Mirror löschen (Historie)
und mit neuem Token neu anlegen, oder via Gitea-Web-UI das Passwort-Feld (Historie)
erneuern). Wert nie in Versionskontrolle ablegen.

Aktivierung der Windows-Builds selbst erst, wenn sie gebraucht werden (siehe
Aufgabe „Windows-Build aktivieren"); bis dahin bleibt alles auf Gitea/Linux. (Historie)

### 8.2 Stand 2026-10-23: GITEA_TOKEN-Secret ist gesetzt (Historie)

- **Secret:** `GITEA_TOKEN` im GitHub-Repo `SeBoOne/architects-overlay` (Historie)
  (Settings → Secrets and variables → Actions), gesetzt via
  `gh secret set GITEA_TOKEN -R SeBoOne/architects-overlay` (gh 2.101.0, (Historie)
  auth als SeBoOne).
- **Token dahinter:** Gitea-Access-Token `github-mirror-release` (Historie)
  (Admin-CLI: `gitea admin user generate-access-token --username sebo (Historie)
  --scopes write:repository`) — Scope minimal auf Repo-Write (Release-Upload).
- **Verifiziert:** Token mit `Authorization: token …` gegen
  `/api/v1/repos/sebo/architects-overlay/releases` → HTTP 200;
  `gh secret list` zeigt das Secret.
- **Rotation:** Gitea-UI → Settings → Applications → Token löschen/neu (Historie)
  generieren, dann `gh secret set` erneut. Tokens nie in Repo/Logs.
- **gh-CLI-Hinweis:** `~/.local/bin/gh` ist ein mise-Wrapper und hängt bei
  jedem Aufruf (Registry-Lookup). Direkt verwenden:
  `~/.local/share/mise/installs/gh/2.101.0/gh_2.101.0_linux_amd64/bin/gh`.

### 8.3 Stand 2026-10-25: Windows-Builds AKTIVIERT und End-to-End verifiziert

Auf Sebos ausdrücklichen Wunsch aktiviert (t_413b5466). Die Pipeline ist
seitdem dauerhaft scharf: jeder Push auf `main` und jedes Tag `v*` auf Gitea (Historie)
wird vom Push-Mirror nach GitHub gespiegelt und dort auf `windows-latest`
gebaut.

**End-to-End-Nachweis (Run 40157806149, Tag `v0.3.1`):**

- GitHub-Actions-Run `windows-build` auf `windows-latest` → **success**
  (15 min). Auch die Runs auf `main` (u. a. 40145648022, 40157873393)
  grün.
- Gitea-Release `v0.3.1` enthält nach dem Lauf die Windows-Assets: (Historie)
  - `architects-overlay_0.3.1_x64-setup.exe` (NSIS, ~4.2 MB)
  - `architects-overlay_0.3.1_x64_en-US.msi` (~6.0 MB)
  plus die Linux-Assets (AppImage/deb) aus der Gitea-Pipeline. (Historie)
- Download-Test: NSIS-Asset per API/`releases/download/v0.3.1/...`
  geladen → HTTP 200, Größe stimmt.

**Wartungshinweise:**

- Beim Re-Setzen eines bereits gebauten Tags bleiben Assets des ersten
  Runs in der Release hängen (alte Versionsnummern). Vor Re-Tag alte
  Assets am Release löschen oder Tag komplett löschen/neu schieben.
- Kommt ein Versions-Bump ohne `Cargo.lock`-Nachzug, baut das Bundle
  trotzdem nach `tauri.conf.json` — Cargo.lock trotzdem mit committen
  (Follow-up c60833f).
- Watchdog: `gh run list -R SeBoOne/architects-overlay --limit 5`
  (direkt über die mise-Install-Path-Version, siehe §8.2).

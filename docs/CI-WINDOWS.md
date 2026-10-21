# Windows-Runner für Gitea Actions (architects-overlay)

Der Windows-Job in `.gitea/workflows/build.yml` trägt das Label
`windows-selfhosted`. Solange kein Runner mit genau diesem Label registriert
ist, wartet der Job einfach in der Queue — es wird **nichts rot**. Erst wenn
der Runner online ist, baut er bei jedem `v*`-Tag den Windows-Installer
(NSIS/MSI) und hängt ihn an die Gitea-Release.

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

Von den offiziellen Release-Seiten (jeweils `act_runner-x.y.z-windows-amd64.exe`):

- https://gitea.com/gitea/act_runner/releases  (offiziell)
- oder GitHub-Spiegel: https://github.com/Gitea/act_runner/releases

In einen eigenen Ordner legen und die Datei zu `act_runner.exe` umbenennen:

```
C:\gitea-runner\
   act_runner.exe
```

**Wichtig:** Alle folgenden Befehle aus diesem Ordner ausführen — die
Registrierung legt `.runner` und `config.yaml` neben der EXE ab.

---

## 3. Registrierungstoken aus Gitea holen

1. Browser: https://git.sebo-home.de/sebo/architects-overlay
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

In `C:\gitea-runner` (PowerShell):

```
.\act_runner.exe register `
  --instance https://git.sebo-home.de `
  --token __PLATZHALTER_TOKEN__ `
  --name sebo-winpc `
  --labels windows-selfhosted `
  --no-interactive
```

- `--labels` **muss exakt** `windows-selfhosted` sein — nur mit diesem Label
  findet der Windows-Job aus `build.yml` den Runner.
- Prüfen: `.\act_runner.exe` (ohne Argumente) → „Runner is stopped"/Kein
  Fehler = Registrierung ok. Es erscheint `C:\gitea-runner\.runner`.

---

## 5. Als Autostart einrichten (läuft dauerhaft)

Der Runner pollt Gitea aktiv — es muss kein Port freigegeben werden, eine
Firewall-Freigabe für ausgehendes HTTPS reicht (und selbst die fragt Windows
beim ersten Lauf ab).

### Variante A: Task Scheduler (empfohlen, startet beim Anmelden)

Datei `C:\gitea-runner\start-runner.bat` anlegen:

```
@echo off
cd /d C:\gitea-runner
act_runner.exe daemon
```

Task anlegen (einmal):

```
schtasks /Create /TN "Gitea Act Runner" /TR "C:\gitea-runner\start-runner.bat" /SC ONLOGON /RL HIGHEST /F
```

Danach abmelden/anmelden oder sofort testen:

```
schtasks /Run /TN "Gitea Act Runner"
```

### Variante B: Autostart-Ordner

`Win+R` → `shell:startup` → Verknüpfung auf `start-runner.bat` (aus Variante A)
hineinlegen. Startet ebenfalls bei jeder Anmeldung.

Status des Daemons: Logs erscheinen im Konsolenfenster; mehr Debug mit
`act_runner.exe daemon --config config.yaml` und `--verbosity trace`.

---

## 6. Online-Status prüfen

Gitea → **Settings → Actions → Runners**: `sebo-winpc` muss **online**
anzeigen (grüner Punkt). Das passiert wenige Sekunden nach dem Daemon-Start.

---

## 7. Tag pushen → Windows-Exe entsteht

Ablauf ab jetzt vollautomatisch:

1. Tag pushen, z. B. `v0.1.0` (oder ein weiterer Runner-Online-Check über
   einen bestehenden v-Tag).
2. Gitea Actions baut den Linux-Job (deb/AppImage) und legt die Gitea-Release
   an.
3. Der Windows-Job läuft jetzt auf deinem PC und erzeugt
   `src-tauri/target/release/bundle/nsis/architects-overlay_<version>_x64-setup.exe`
   (+ MSI). NSIS wird von Tauri v2 automatisch mitgeladen.
4. Installer und MSI landen als Release-Assets auf
   https://git.sebo-home.de/sebo/architects-overlay/releases und zusätzlich
   als CI-Artifact des Runs.

Damit ist der Kreislauf geschlossen:

**Runner online → Windows-Exe entsteht automatisch bei Tags.**

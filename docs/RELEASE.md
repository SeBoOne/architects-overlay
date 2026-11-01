# Release-Flow — GitHub ist die kanonische Plattform

Seit dem 31.10.2026 ist **GitHub die einzige Build-/Release-Plattform** —
Gitea wird nicht mehr gebaut, gespiegelt oder referenziert.

## Repos

| Repo | Sichtbarkeit | Inhalt | Zweck |
|---|---|---|---|
| `git.sebo-home.de/sebo/architects-overlay` (Gitea, privat) | privat | komplette Historie: main + Feature-Branches + Tags | **Dev-Quelle**: Builder-Bots arbeiten hier (Feature-Branches, Merges) |
| `SeBoOne/architects-overlay` (GitHub) | **öffentlich** | nur `main` + `v*`-Tags | **Öffentliche Heimat + Release-Quelle** (Updater + install.sh + Mitspieler-Downloads) |

## Ablauf

1. Builder-Bots: Feature-Branches im **dev-Repo**, Gates lokal (cargo
   test/check/fmt + pnpm check/build) — Reviewer prüft am Branch.
2. Omar merged nach `main` (dev-Repo) und bumped die Version
   (`Cargo.toml` + `tauri.conf.json`) **vor** dem Tag — der Tag-Commit muss
   die Version tragen (Assets heißen sonst falsch).
3. `git push gitea-sebo main --tags` auf das **dev-Repo**.
4. `git push public main --tags` auf das **öffentliche Repo** — der Push des
   `v*`-Tags triggert die GitHub-Actions (`.github/workflows/build.yml`,
   nur auf Tags).
5. GitHub-hosted Runner bauen **Linux** (deb/AppImage) und **Windows**
   (NSIS **+** MSI — hosted Runner haben MSVC/WiX komplett) und hängen die
   Assets ans Release an.
6. Checksums: `SHA256SUMS` als Release-Asset (Updater prüft).

## Was NICHT mehr passiert

- **Keine Gitea-CI-Builds** (`.gitea/` ist entfernt); der selfhosted
  Windows-VM-Runner (dockur/windows auf server-hermes) ist ausgeschaltet,
  bleibt aber eingerichtet — bei Bedarf einfach wieder hochfahren und das
  Label `windows-selfhosted` reaktivieren.
- **Kein Gitea→GitHub-Spiegel-Job** — das öffentliche Repo bekommt main +
  Tags direkt per Push (derselbe Push, kein nachträgliches Kopieren).
- **Keine macOS-Builds** (pausiert bis Mitspieler-Livetests; ein
  `macos-latest`-Job ist bei Bedarf reaktivierbar).

## Updater / install.sh

- Updater (TG): primär `https://github.com/SeBoOne/architects-overlay/releases/latest/download/latest.json`
  — Redirect-Pfad, NICHT rate-limited (keine API; die API ist unauth. auf
  60 req/h pro IP gedeckelt und bricht hinter CGNAT/Uni-Netzen). Jede
  Release-Pipeline (`.github/workflows/build.yml`, Job `latest-json`) hängt
  `latest.json` (tag, version, Assets + SHA256s, released_at) als Asset an.
  Fallback für alte Releases ohne `latest.json`: Releases-API
  (`releases/latest`, ohne Auth) — bei 429 zeigt der Check „Rate-Limit,
  später erneut“.
- Module-Overrides: bewusst WEITERHIN `raw.githubusercontent.com/SeBoOne/architects-overlay/main/src-tauri/resources/module-overrides.json`
  (TG-Entscheidung) — die Datei lebt im main-Branch und ändert sich
  unabhängig von Releases; raw.githubusercontent ist nicht an die
  API-Rate-Limits gebunden.
- install.sh: `raw.githubusercontent.com/SeBoOne/architects-overlay/main/scripts/install.sh`
  (öffentlich); die Release-Quelle ist ausschließlich GitHub — Details und
  Flags: [docs/INSTALL.md](INSTALL.md).

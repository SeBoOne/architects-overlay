# Release-Flow — GitHub ist die kanonische Plattform

Seit dem 31.10.2026 ist **GitHub die einzige Build-/Release-Plattform** —
Gitea wird nicht mehr gebaut, gespiegelt oder referenziert.

## Repos

| Repo | Sichtbarkeit | Inhalt | Zweck |
|---|---|---|---|
| `SeBoOne/architects-overlay-dev` | privat | komplette Historie: main + Feature-Branches + Tags | **Dev-Quelle**: Builder-Bots arbeiten hier (Feature-Branches, Merges) |
| `SeBoOne/architects-overlay` | **öffentlich** | nur `main` + `v*`-Tags | **Öffentliche Heimat + Release-Quelle** (Updater + install.sh + Mitspieler-Downloads) |

## Ablauf

1. Builder-Bots: Feature-Branches im **dev-Repo**, Gates lokal (cargo
   test/check/fmt + pnpm check/build) — Reviewer prüft am Branch.
2. Omar merged nach `main` (dev-Repo) und bumped die Version
   (`Cargo.toml` + `tauri.conf.json`) **vor** dem Tag — der Tag-Commit muss
   die Version tragen (Assets heißen sonst falsch).
3. `git push origin main --tags` auf das **dev-Repo**.
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

- Updater: `https://api.github.com/repos/SeBoOne/architects-overlay/releases/latest`
  (öffentlich, ohne Auth) — die Settings-Migration (TF) zieht
  Bestandsinstallationen automatisch vom alten Gitea-Wert rüber.
- install.sh: `raw.githubusercontent.com/SeBoOne/architects-overlay/main/scripts/install.sh`
  (öffentlich); Gitea bleibt als `RELEASE_BASE`-Opt-in möglich, ist aber
  nicht mehr Standard.

# Release-Flow — Gitea (Dev) → GitHub (öffentlich)

Seit S5 gilt:

- **Gitea** (`git.sebo-home.de/sebo/architects-overlay`, privat) bleibt die
  **Dev-Quelle**: Feature-Branches, Merges nach main, v*-Tags.
- **GitHub** (`github.com/SeBoOne/architects-overlay`, öffentlich) ist die
  **öffentliche Heimat/Release-Quelle**: nur `main` + `v*`-Tags — keine
  Feature-Branches. Releases (Assets) entstehen weiterhin in Gitea via CI;
  der öffentliche Repo-Spiegel transportiert Quellcode + Tags.

## CI-Ablauf (`.gitea/workflows/build.yml`)

1. Push auf `main` / Tag `v*` löst `build` aus: Job `linux` (deb/AppImage)
   und Job `windows` (NSIS, selfhosted Runner).
2. An `v*`-Tags hängen beide Jobs ihre Assets an die Gitea-Release.
3. **Danach** (needs: [linux, windows] — nur bei BEIDEN success) läuft
   `mirror-public`: force-push von `refs/heads/main` + allen `v*`-Tags nach
   GitHub via Secret `GITHUBMIRRORTOKEN` (nur im Job, nie im Repo).

## Downstream

- `scripts/install.sh` + `docs/INSTALL.md`-Einzeiler zeigen auf das öffentliche
  GitHub-Repo (Releases via GitHub-API, kein Token).
- Auto-Updater (docs/UPDATER.md) checkt
  `https://api.github.com/repos/SeBoOne/architects-overlay/releases/latest`.

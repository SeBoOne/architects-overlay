# SC Architects Overlay

Star Citizen Trade Overlay auf Basis von **Tauri v2 + SvelteKit** (Svelte 5,
TypeScript, Vite), Rust-Backend. Zeigt Preise/Marktplatz als Overlay über dem
Spiel; Datenquelle ist UEX (nur die erste von mehreren – siehe Architektur).

## Stack

- **GUI-Shell:** Tauri v2 (Rust, Workspace unten)
- **Frontend:** SvelteKit 3 (Svelte 5, runes-Modus), adapter-static → `build/`
- **Theme:** Dark Mode by default (CSS-Variablen, kein Light-Theme)

## Projektstruktur

```
Cargo.toml                  Cargo-Workspace (Wurzel)
src-tauri/                  Tauri-v2-App (Fenster, Lifecycle)
crates/
  provider/                 DataProvider-Trait + gemeinsame DTOs  ← Architektur-Seam
  uex-client/               UEX-API-Client (erste DataProvider-Impl)
  cache/                    SQLite-Cache (rusqlite), TTL pro Datentyp
  game-detection/           Star-Citizen-Prozess-Erkennung
  settings/                 Config-Persistenz (App-Config-Dir, API-Key)
src/                        SvelteKit-Frontend (Layout, Dark-Mode-Shell)
.gitea/workflows/build.yml  Gitea-Actions-Skeleton (Tag-Builds, noch nicht grün)
```

### Architektur-Grundsatz

**UEX ist nicht fest verdrahtet.** `crates/provider` definiert ein einziges
Interface (`DataProvider`: `search`, `prices`, `marketplace_offers`) — die UI
spricht nur dieses an. `uex_client` ist die erste Implementierung; weitere
Quellen kommen als zusätzliche `DataProvider`-Implementierungen dazu, ohne
dass sich die UI-Schicht ändert. Provider-Typen leaken nie durch den Seam
in die UI.

## Entwicklung

```sh
pnpm install          # Frontend- + Tauri-CLI-Dependencies
pnpm dev              # Vite-Dev-Server (Port 5173)
pnpm build            # Frontend → ./build (statisch, für Tauri)
pnpm tauri dev        # Voll-Dev: Vite + Tauri-Fenster
pnpm tauri build      # Release-Bundle (deb/AppImage bzw. exe/msi)
```

Rust-Workspace prüfen:

```sh
cargo check --workspace
```

## Status

Scaffold (Task t_65980b75): Struktur steht und kompiliert. UEX-Client,
Overlay-Fenster (Hotkey/Click-through/Prozess-Erkennung) sind nachfolgende Tasks.
CI-Skeleton ist bewusst noch nicht grün (Windows-Runner braucht Admin; Gitea
Action-Mapping muss im Runner eingerichtet sein).
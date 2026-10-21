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
.gitea/workflows/build.yml  Gitea Actions: Linux-Bundle bei push main + Tag v*
                            (Windows-Job wartet auf Label windows-selfhosted,
                            siehe docs/CI-WINDOWS.md — kein Rot ohne Runner)
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

## Shortcuts

| Taste | Kontext | Aktion |
|---|---|---|
| `M` | kein Eingabefeld fokussiert | Market-Pane togglen |
| `Strg+M` / `Meta+M` | überall, auch bei fokussierter Suchzeile | Market-Pane togglen |
| `O` | Error-Pane, kein Feld fokussiert | Offline mit Cache fortsetzen |
| `↑` / `↓` / `↵` / `Esc` | Palette | Navigation / öffnen / ausblenden-leeren |
| `Tab` | Palette | Suchfilter zyklisch wechseln |
| `↑` / `↓` / `↵` / `↹` / `→` / `Esc` | Market-Pane | Angebote navigieren / Detail öffnen / zurück |
| `↵` | Angebots-Detail | Listing auf uexcorp.space im Browser öffnen (Deep-Link) |

`M` und `O` sind druckbare Buchstaben: Solange ein Eingabefeld fokussiert ist
(die Suchzeile ist immer autofokussiert), werden sie als Zeichen getippt und
lösen **keinen** Shortcut aus — das `m` aus „titanium“ kommt an. Im Suchfeld
ist der Market-Shortcut deshalb `Strg+M`; ohne fokussiertes Feld schaltet das
bloße `M` (bzw. `O` im Error-Pane) weiter direkt.

## Leistung (Kriterium 4: „messen und dokumentieren“)

**Methode**

- *RAM:* Prozessbaum des laufenden Release-Builds — alle `architects-overlay`-
  PIDs inklusive WebKit-Childs; PSS und RSS je Prozess aus
  `/proc/<pid>/smaps_rollup` aufsummiert (entspricht der
  `smem -t`/psutil-Summierung).
- *Such-Latenz:* `search`-IPC gegen die lokale `cache.sqlite3`; pro Query der
  Cache-Call in `tokio::time::Instant` geklammert, Median/p95/Max über eine
  Sequenz von Einzel- und Mehrbuchstaben-Queries.
- Referenzumgebung: Release-Build, warme `cache.sqlite3` mit 8798 Entities.

**Referenzmessung** (QA-Review t_8ad24457, Branch @ 8b936c1, gleiche Maschine):

| Metrik | Overlay | Electron-Baseline (Hermes Desktop, 28 Prozesse) |
|---|---|---|
| PSS gesamt | 233 MiB | ~1031 MiB |
| RSS gesamt | 591 MiB | ~1693 MiB |
| Suche `a` — Median | 5,3 ms | — |
| Suche `a` — p95 | 14,0 ms | — |
| Suche `a` — Max | 30,2 ms | — |
| Suche mehrstellig — Median | ≤ 0,07 ms | — |

Ziel „< 50 ms Such-Latenz" und „deutlich unter Electron-Niveau" ist erfüllt.
Hinweis zur Messtiefe: Die Referenz lief mit **zwei** WebKit-Webviews
(Spotlight + nie genutzte Main-Shell, ~70 MiB PSS). Seit der Fix-Runde Phase 1
(t_861df10e) wird die Main-Shell nur noch in Debug/Dev-Builds erzeugt — der
Release-Prozessbaum hat damit genau einen WebKitWebProcess weniger.

**Nachgemessen (Fix-Runde, Release-Build, diese Maschine):** Prozessbaum mit
einer Instanz = App + `WebKitNetworkProcess` + ein `WebKitWebProcess` → PSS
**182 MiB** / RSS **453 MiB** (vorher 233/591 mit Main-Shell; weiterhin weit
unter der Electron-Baseline 1031 MiB PSS).

## Status

- **Datenbasis (t_7e0a8993, Branch `feature/uex-client-cache`):** Provider-Seam
  (`crates/provider` mit async `DataProvider`-Trait, typisierten Fehlern und
  Cluster-DTOs), UEX-HTTP-Client (`crates/uex-client`: reqwest+rustls, Timeout,
  Rate-Limit-Pacing, Fehler-Mapping), SQLite-Cache (`crates/cache`: Entities
  inkl. FTS5-Suche, Preise mit 30-min-TTL, Marketplace-Snapshot, TTL je
  Datentyp, Hintergrund-Sync-Task) und `crates/settings` (Config-Datei im
  App-Config-Dir, API-Key 0600). Alle Tests grün (Unit + Fixture + wiremock,
  Live-Tests `#[ignore]`).
- **Phase 1 (t_a19fe214, Branch `feature/suchpalette-ui`):** Suchpalette-UI im
  Spotlight, globales Hotkey/Click-through/Prozess-Erkennung, UEX-Verkabelung
  (Suche/Preise/Sync gegen lokalen Cache). Fix-Runde t_861df10e: der globale
  `m`-Shortcut schluckt keine Zeichen im Suchfeld mehr (Market = `Strg+M` im
  Feld), Kriterium-4-Messung hier oben dokumentiert, Offline-Erkennung läuft ab
  Boot (auch mit warmem Cache), Hyprland-Geometrie per `set_size` nach Map
  abgesichert, Click-through-Default konsistent (klarer Default, Hotkey toggelt
  in Fokus-Modus).
- **Phase 2 (t_7b8772e2, Branch `feature/marketplace-deeplinks`):** Marketplace
  lesen — Live-Offers anderer Spieler über `/marketplace_listings`, Filter
  (Operation, Region über `/star_systems`, Freitext über Titel/Beschreibung/
  Anbieter), Sortierung nach Preis/Neueste im Provider (die API bietet kein
  Server-Sort). Market-Pane im Spotlight (Strg+M) mit Angebotsliste (Preis,
  Menge, Ort, Region, Verkäufer, Alter), Detail-Ansicht pro Angebot und
  **Deep-Link**-Button, der die uexcorp.space-Listing-Seite
  (`/marketplace/item/info/{slug}/`) per Tauri-Opener im Standard-Browser
  öffnet — reine Lese-Pfade, keine Schreib-Endpunkte. Hintergrund-Sync mit
  kurzer TTL (5 min, eigener, rate-limit-schonender Zyklus: 1 Listing-Fetch je
  Fenster) in den bestehenden SQLite-Cache; Reads kommen aus dem frischen
  Snapshot, bei Ausfall greift ein Offline-Snapshot. Schema-Migration fügt
  `region`/`slug` zu bestehenden Phase-1-Caches hinzu.
- CI: `.gitea/workflows/build.yml` finalisiert — Linux-Job baut deb/AppImage bei
  push auf main und an `v*`-Tags (inkl. Gitea-Release-Upload). Windows-Job
  (Label `windows-selfhosted`, Einrichtung: `docs/CI-WINDOWS.md`) wartet
  sauber, bis ein Windows-Runner registriert ist — kein Rot ohne Runner.
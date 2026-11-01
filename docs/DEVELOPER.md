# SC Architects Overlay — Developer Notes

> Spieler-README: [README.md](../README.md) · Release-Flow: [RELEASE.md](RELEASE.md)

Entwickler-fokussierte Doku: Stack, Gates, Konventionen, Brand-Pipeline.

## Stack

- **GUI-Shell:** Tauri v2 (Rust, Workspace unten)
- **Frontend:** SvelteKit (Svelte 5, runes-Modus), adapter-static → `build/`
- **Theme:** Dark Mode by default (CSS-Variablen, kein Light-Theme)

## Projektstruktur

```
Cargo.toml                  Cargo-Workspace (Wurzel)
src-tauri/                  Tauri-v2-App (Fenster, Lifecycle, Icons, tauri.conf.json)
crates/
  provider/                 DataProvider-Trait + gemeinsame DTOs  ← Architektur-Seam
  uex-client/               UEX-API-Client (erste DataProvider-Impl)
  scwiki-client/            SC-Wiki-API-Client (Shipmatrix etc.)
  cache/                    SQLite-Cache (rusqlite), TTL pro Datentyp
  game-detection/           Star-Citizen-Prozess-Erkennung
  settings/                 Config-Persistenz (App-Config-Dir, API-Key)
  updater/                  Auto-Updater-Kernlogik (semver, Release-API, SHA256, Pläne)
  i18n-err/                 {error_key, params}-Nachrichten-Transport (Rust → Frontend)
  math-eval/                Sandbox-Ausdrucksauswertung (Rechen-Chips)
src/                        SvelteKit-Frontend (Layout, Dark-Mode-Shell)
.github/workflows/build.yml GitHub Actions: Linux (deb/AppImage) + Windows
                            (NSIS + MSI), nur bei v*-Tags; Job latest-json
                            hängt latest.json + SHA256SUMS ans Release
```

### Architektur-Grundsatz

**UEX ist nicht fest verdrahtet.** `crates/provider` definiert ein einziges
Interface (`DataProvider`: `search`, `prices`, `marketplace_offers`) — die UI
spricht nur dieses an. `uex_client` ist die erste Implementierung; weitere
Quellen kommen als zusätzliche `DataProvider`-Implementierungen dazu, ohne
dass sich die UI-Schicht ändert. Provider-Typen leaken nie durch den Seam
in die UI.

## Build-Gates

Jeder Feature-Branch muss lokal grün sein, bevor Review/Merge:

```sh
cargo test --workspace      # Unit- + Fixture-Tests (live-Tests sind #[ignore])
cargo check --workspace
cargo fmt --all -- --check
pnpm check                  # svelte-check — 0 Errors
pnpm build                  # Frontend → ./build (statisch, für Tauri)
```

Release-Bundle lokal: `pnpm tauri build` (deb/AppImage bzw. exe/msi).

## Release-Flow

Kurzform: Feature-Branches im **privaten Dev-Repo**
(`git.sebo-home.de/sebo/architects-overlay`, Gitea-Host, nur DEV-Kontext) →
Gates → Merge nach `main` → Version-Bump → Tag `v*` → Push auf dev +
public (`SeBoOne/architects-overlay`). GitHub ist die **einzige**
Build-/Release-Plattform (GitHub Actions auf dem öffentlichen Repo bauen bei
Tag-Push); Details, Updater-Endpoint und Asset-Benennung:
**[docs/RELEASE.md](RELEASE.md)**.

## Branch- & Worktree-Konvention

- Feature-Arbeit auf `feature/<kurzname>`-Branches, niemals direkt auf `main`.
- Worktrees unter `.worktrees/<task-id>/` (git-ignored), z. B.
  `git worktree add .worktrees/t_xxx -b feature/xyz origin/main`.

## Kanban-Workflow

Arbeit wird über das Hermes-Kanban gesteuert (Karten pro Aufgabe, Review
über die Review-Lane); Commit-Referenzen und Gate-Ergebnisse landen in der
jeweiligen Karte. Kein direktes Arbeiten ohne Karte.

## Brand-Ordner

`brand/logo/` — **`brand/logo/logo.svg` ist der Master.** Daraus abgeleitet:
`brand/logo/derived/` (PNG-Exporte, Lockup) und die Tauri-Icons
(`src-tauri/icons/`, Regeneration via `brand/logo/rebuild-icons.sh` nach
jeder Logo-Änderung). Es gibt **keine Rebuild-Colorways** mehr — ein Emblem,
eine Quelle.
**Wichtig:** `brand/` ist bewusst **nicht** Teil der MIT-Lizenz
(siehe [LICENSE](../LICENSE), „Brand and Logo Exclusion“) — keine
Weiterverwendung außerhalb des Projekts.

## Secrets

- **Keine Tokens/Keys im Repo** — weder committet noch in CI-Dateien.
- GitHub Actions (`.github/workflows/build.yml`) braucht keine eigenen
  Secrets — Releases werden mit dem eingebauten `GITHUB_TOKEN` des
  öffentlichen Repos angehängt.
- UEX-API-Key des Nutzers liegt nur in der App-Config-Dir (Dateirechte 0600),
  nie im Repo.

## Modul-Zuweisungen (module-overrides.json)

Seit S4 (t_66980b3c) kommt die Hangar-Modul-Kompatibilität **ausschließlich**
aus `src-tauri/resources/module-overrides.json` — die SC-Wiki-API liefert
keine Modul-Daten mehr (Hardpoints/Ports und Item-Katalog sind raus).

Pflege der Datei:
- Eintrag pro Schiff: `"ship"` = Schiffsname **exakt** wie im Hangar gepflegt
  (Match ist nur Case-/Whitespace-tolerant, z. B. `RSI Aurora MK II`).
- `"modules"` = Liste der Modul-Namen, so wie sie im Autocomplete erscheinen
  sollen (z. B. `Galaxy - Cargo Module`). Doppelte Namen pro Schiff werden
  beim Einlesen vereinigt.
- Schiffe **ohne** Eintrag in der Datei bekommen **gar kein** Modul-Feld im
  Hangar — die Datei ist die einzige Quelle (kein API-Fallback).
- Remote-Override: Die App lädt bei jedem Sync/Refresh (stündlicher Loop +
  Settings-Button „Modul-Zuweisungen aktualisieren“) die Datei von
  `raw.githubusercontent.com/SeBoOne/architects-overlay/main/src-tauri/resources/module-overrides.json`
  und cachet sie; Fallback-Kette bei Fehler: Remote-Cache → Repo-Baseline.
  Änderungen an der Datei im Repo greifen damit auch ohne App-Update.

## Connector-System (Planung)

Ein eigenes Connector-System (Spieldaten ohne Screen-Scraping) ist
konzeptioniert, aber **nicht implementiert** — der Entwurf liegt in
[docs/research/connector-system.md](research/connector-system.md). Status:
wartet auf die AAWA-API; bis dahin keine Umsetzung.

## i18n-System (T1/T2)

Das Overlay ist vollständig lokalisiert — Default-Sprache **en**, Deutsch
(`de`) mit dabei, Live-Switch ohne Neustart.

- **Key-Dateien:** `resources/locales/en.json` + `resources/locales/de.json`
  (flaches `"key" → "text"`-Map unter `strings`, `meta`-Block mit Code/Name;
  Key-Parität en/de wird per Skript sichergestellt — aktuell 924 Keys).
- **Fallback-Kette:** gewählte Sprache → `en` → roher Key. Fehlende Keys sind
  kein Fehler — sie werden im Debug-Log notiert (einmal pro Key).
- **Implementierung:** eigene Svelte-Store-`t()`-Funktion
  (`src/lib/i18n.ts`), bewusst OHNE svelte-i18n-Package. Built-in-Sprachen
  werden statisch importiert (Vite bündelt sie), Interpolation über
  `{name}`-Platzhalter; Plural per Key-Suffix `_one`/`_other`.
- **Rust-Seite:** Fehler/Nachrichten an das Frontend gehen als
  `error_key` + optionale Params (`err_key`/`err_keyp`, gemeinsame Crate
  `i18n-err`, re-exported via `src-tauri/src/i18n_err.rs`) — das Frontend
  übersetzt. Lokalisierung erfolgt nie als fertiger String im Rust-Code.
- **Community-Sprachen:** Nutzer importiert lokale Übersetzungsdateien über
  Einstellungen → Sprache (Rust validiert + persistiert unter
  `<app-local-data>/locales/`); Beispiel: `examples/locale.example.json`.
  Siehe auch README-Abschnitt „Translations“.

## Mode-Hotkeys

Die In-App-Modi wechseln per F-Taste (nur bei sichtbarem/fokussiertem
Overlay). **F1 (Suche) und F12 (Einstellungen) sind fix**, die anderen Slots
sind in den Einstellungen änderbar (F1–F12, F5 bleibt für den Resync
reserviert, keine Doppelbelegung):

| Modus | Default | änderbar |
|---|---|---|
| Suche | `F1` | nein (fix) |
| Marktplatz | `F2` | ja |
| Handelsrouten | `F3` | ja |
| Inventar | `F4` | ja |
| Hangar | `F11` | ja |
| Einstellungen | `F12` | nein (fix) |

Kanonische Quelle: `src/lib/hotkeys.ts` (Defaults/Validierung) +
`crates/settings/src/lib.rs` (Persistenz, `settings_set_mode_hotkey` /
`settings_reset_mode_hotkeys`). Details: [docs/SHORTCUTS.md](SHORTCUTS.md).

## Tastenkürzel

Die vollständige Tabelle (globale Kürzel, Mode-Hotkeys, In-App-Keys) liegt
in **[docs/SHORTCUTS.md](SHORTCUTS.md)** — diese Datei ist die kanonische
Referenz; hier keine Duplikate pflegen.

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

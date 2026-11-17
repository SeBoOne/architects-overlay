# Omar-W Onboarding — Windows/WSL2-Dev-Setup

> Für den Hermes-Agent **Omar-W** (WSL2 auf dem Windows-PC) und jeden Menschen,
> der am Windows-Seitigen des Overlays arbeitet. Spieler-README:
> [README.md](../README.md) · Release-Flow: [RELEASE.md](RELEASE.md) ·
> Windows-CI: [CI-WINDOWS.md](CI-WINDOWS.md) · Developer-Notes:
> [DEVELOPER.md](DEVELOPER.md)

Omar-W kloned das private Dev-Repo (Gitea) und arbeitet an Windows-spezifischen
Fehlern. Diese Datei bündelt die erlittenen Fallstricke, die sonst nur in
Session-Historie leben.

## Setup

```bash
git clone ssh://git@100.64.0.3:3222/sebo/architects-overlay.git
cd architects-overlay
pnpm install            # pnpm, kein npm/yarn
pnpm run check          # svelte-check — Gate 1
pnpm build              # Svelte-Frontend → build/ (frontendDist ist ../build!)
cargo test --workspace  # Gate 2 — VOR dem ersten eigenen Build purge-regeln siehe unten
```

## Rollen & Git-Regeln (verbindlich)

- **Nur eigene Branches pushen** (`fix/<thema>`, `feature/<thema>`), **NIE
  direkt nach `main`** — der Orchestrator (Omar) merged `--no-ff`.
- Version-Bump (`src-tauri/Cargo.toml` + `src-tauri/tauri.conf.json` +
  `Cargo.lock` via `cargo update -p architects-overlay --precise <ver>`)
  macht der Orchestrator **vor** dem Tag — der Tag-Commit muss die Version
  tragen, sonst heißen die CI-Assets falsch.
- **Tag NIEMALS in derselben Shell-Kette wie den Build** (`| tail` /
  `| grep`-Ketten stoppen bei Compile-Fehlern nicht) — erst isoliert bauen,
  auf `error`-Zeilen prüfen, dann taggen.
- Release-/Deploy-Entscheidungen (Tag, `git push public`) laufen nur über den
  Orchestrator auf explizites User-„raus".

## Gates (alles grün, bevor ein Branch fertig ist)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm svelte-check
pnpm build
```

- Maßstab ist `clippy --all-targets` (Test-Crate-Nits sonst unsichtbar).
- **Purge vor `cargo test --workspace`** (stale Workspace-Binaries): ein
  Builder-Checkout kann `CARGO_MANIFEST_DIR` eines gelöschten Workspaces
  eingebacken haben → Fixture-Tests brechen mit NotFound:
  `find target/debug/deps -maxdepth 1 -type f -executable -delete`
- Der patch-Tool-Eigenlint für `.rs` läuft ohne Crate-Edition und meldet
  FALSCH-Positive (`let chains only allowed in Rust 2024` etc.) — diese
  Meldungen ignorieren, Maßstab bleibt cargo fmt/check/clippy/test.

## Hart erlittene Build-/Runtime-Fallen

1. **custom-protocol-Falle:** Ein lokaler Release-Build NIEMALS mit plain
   `cargo build --release` — ohne `--features tauri/custom-protocol` lädt das
   WebView die devUrl (`http://127.0.0.1:5173`) statt der eingebetteten Assets
   → transparentes Fenster + „Connection terminated unexpectedly".
   Korrekt: `cargo build --release -p architects-overlay --features
   tauri/custom-protocol` oder `pnpm tauri build` (wie CI).
2. **Frontend vor Cargo:** `pnpm build` VOR dem Cargo-Build — cargo kompiliert
   das Svelte-Frontend NICHT mit, es bettet die vorhandenen `build/`-Assets
   ein. Ohne frisches `pnpm build` ist der Feature-Code im Repo, aber die App
   zeigt den alten Stand (Symptom: „Feature fehlt", obwohl Merge grün war).
3. **f64-Geometrie-Contract:** Fenstereigenschaften/Zeiger-Koordinaten in
   Tauri-Command-Args immer `f64` + Runden im Backend. WebKitGTK liefert
   fraktionale `clientX/Y` — Integer-Contract-Args werden STILL verworfen
   (kein Fehler!). `ev.movementX/Y` ohne Pointer-Lock = 0.
4. **v2-Ban (Linux/Hyprland):** Der Overlay-Prozess darf NIEMALS aktive
   Workspace-/Fokus-Korrektur-Dispatches ausführen (Polling, `workspace`-,
   `movetoworkspacesilent`, Fokus-„Korrektur"-Loops) — der Versuch, das
   Overlay aktiv an den SC-Workspace zu binden, wirft den Nutzer bei jedem
   Workspace-Wechsel ins Spiel zurück (lebte Erfahrung, Rollback erzwungen).
   Reaktiv-only: socket2-Events, Einzel-Dispatch mit Kühlgrenze
   (`src-tauri/src/overlay.rs`, M6-F5). Der v2-Regression-Test im selben Modul
   greppt die Quelle — muss grün bleiben.
5. **Hyprland-Selektoren (Linux-Pfad):** `hl.dsp.focus({window="…"})`-Selektoren
   sind Regex; AND-Kriterien (`pid:X,title:Y`) matchen auf Hyprland 0.56.2
   nicht — reine `class:`-Selektoren sind verlässlich, exakte `title:`-Matches
   scheitern am SC-Titel (`"Star Citizen "`, trailing Space → „window not
   found"). Titel-Fallback nur mit `.*`-Suffix.
6. **Release-Logger:** Release-Build loggt nur **Warn+** (tauri_plugin_log,
   LevelFilter::Warn, Stdout → Journal). Diagnose-Logs müssen `log::warn!`
   sein, sonst Blindrunde. Ein else-Zweig von `cfg!(debug_assertions)` wird
   von debug-Gates nie kompiliert — zusätzlich
   `cargo check -p architects-overlay --release` fahren.

## Windows-spezifisch (Omar-Ws Kernfeld)

- **Game-Detection:** Erkennung per nativem Toolhelp32-Snapshot statt
  tasklist-Kindprozess — AppLocker/WDAC-Richtlinien blockieren das Spawnen von
  tasklist still (Hotkey tot, Auto-Hide kaputt bei Policy-Usern). tasklist
  bleibt Fallback.
- **MSI-Diagnose:** Installationsprobleme per
  `msiexec /i <paket> /l*v install.log` — Log grob nach "error"/"return value 3"
  durchsuchen.
- **VM-Livetest ohne echtes SC:** Game-Gate matched die Prozess-Commandline
  case-insensitive auf `STARCITIZEN.EXE` — ein getarnter Prozess reicht:
  `copy C:\Windows\System32\notepad.exe C:\Temp\StarCitizen.exe && start
  C:\Temp\StarCitizen.exe`. Chip springt auf live, komplette UI testbar.
  Wird der Fake nicht erkannt: erst `tasklist`-Ausgabe gegen das Matching
  debuggen (Defender/SmartScreen blockt gern umbenannte System-Exe-Kopien).
- **Hotkey-Diagnose (Windows):** Debug-Panel per `:d` in der Suchleiste
  an/aus (persistiert). Diagnose-Snapshot (`shortcut_diag_status`-IPC):
  registrierte Combos, Plugin aktiv, letzter OS-Empfang mit matched-Flag,
  fokussiertes Fenster inkl. Elevation. Entschlüsselung: LL-Hook-Zähler
  steigen bei fokussiertem SC = Hook-Kette greift → Fehler in Combo-Erkennung;
  Zähler auf 0 = Hook sieht nichts. **Parser-Regel:** Wenn EIN persistierter
  Wert von MEHREREN Codepfaden geparst wird (Plugin + Fallback-Parser,
  Renderer + Validator), den Fallback gegen das REAL persistierte Format
  testen — Persistierte Slot-Combos liegen im `KeyboardEvent.code`-Format
  (`shift+control+KeyU`), der Fallback kannte nur das kanonische
  `ctrl+shift+u` und warf stumm weg.
- **Diagnose nie nur Backend-seitig bauen** — ein Settings-Diagnose-Panel mit
  Poll + „Als JSON kopieren" gehört dazu, sonst ist die Diagnose nur per
  Journal lesbar und die nächste User-Runde blind.
- **Code-Signatur/Authenticode ist bewusst später** (Weg 2) — nicht einbauen.

## Report-Konvention

Windows-Befunde als Repro + Symptom + (wenn möglich) Diagnose-Snapshot
zurückmelden; der Orchestrator merged und deployed. Jede Bugrunde: EINE
entscheidende Frage, im Snapshot beantwortbar.

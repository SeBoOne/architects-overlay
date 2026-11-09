# FENSTERMODUS-PLAN — Fenstermodus (Wunsch 1) + Dauerhafte Shellboxen „Pins" (Wunsch 2)

**Status:** Technische Sonde (Architekturanalyse, kein Feature-Code). Basis: `main` c5a6bf6.
**Zweck:** Entscheidungsgrundlage für Sebo. Umsetzung erst nach Freigabe + Mockups (Designer).
**Wünsche (Sebo, 2026-11-09):**

1. **Fenstermodus:** Overlay-Funktionen zusätzlich als eigenes normales Fenster (verschiebbar, ver-/größerbar, eigener Monitor, unabhängig vom Spiel). Overlay selbst bleibt unverändert — alle Tastenkombos wirken weiter, auch bei offenem Fenster. Fenster: per Tray öffnen/verstecken, Schließen nur ins Tray, Beenden via Tray oder Fenster-Menü-Button.
2. **Dauerhafte Shellboxen („Pins"):** beliebig markierte Boxen bleiben dauerhaft sichtbar (z. B. Handelsroute), Deckkraft einstellbar, universell für künftige Plugins.

---

## 1. Ist-Architektur (mit Belegen)

### 1.1 Fenster / Rendering

- **Ein einziges Overlay-Webview-Fenster** mit Label `spotlight` (`src-tauri/src/overlay.rs:41`). Erzeugt in `create_spotlight` via `WebviewWindowBuilder` (`overlay.rs:232–253`): `decorations(false)`, `transparent(true)`, `always_on_top(true)`, `skip_taskbar(true)`, `resizable(true)`; auf Windows zusätzlich `shadow(false).resizable(false)` (`overlay.rs:256–258`, Kommentar t_bd8cbdfd: randlos OHNE jede Chrome, keine Drag-Ränder/Aero-Snap).
- **Vollbild-Fenster pro Screen, kompakte Shellbox innen:** das Fenster spannt den ganzen Monitor auf (`place_on_pointer_monitor`, vor + nach dem Map re-asserted, `overlay.rs:627–645`); die sichtbare Palette/Shell schwimbt als CSS-Box darin (app.css: `html[data-window="spotlight"] .shell`, `src/app.css:187–198`; Kommentar `overlay.rs:882`: „the palette UI floats inside"). `PANEL_W/H` (660×600) ist nur Fallback (`overlay.rs:59–60`).
- **Immer-on-top:** statisch beim Bau (`overlay.rs:242`) und re-asserted bei jedem Show auf Windows (`overlay.rs:631`, t_bd8cbdfd Punkt 4: Topmost geht nach Spiel-Fokuswechseln verloren). Linux: bewusst **kein** `pin`-Dispatch mehr — v2-Verbot, geprüft per Grep-Test `overlay.rs:1732–1747` („NO pin anywhere", Kommentar `overlay.rs:1054`).
- **Click-Through:** `set_ignore_cursor_events` nur auf das `spotlight`-Fenster (`overlay.rs:483`, 644, Kommando `overlay_set_click_through` `overlay.rs:1389`+). GTK-Gate: darf erst nach erstem Map gesetzt werden (`overlay.rs:478–481`).
- **Fenster-Instanz-Erkennung im Frontend:** `getCurrentWindow().label` → `document.documentElement.dataset.window` (`src/routes/+page.svelte:3392`); CSS schaltet darauf um (`data-window="spotlight"` vs. `"main"` — opaker Grund, `app.css:151–159`). Das ist der vorhandene Mechanismus, an den ein Fenstermodus andocken kann.
- **Zwei Fenster gibt es bereits konzeptionell:** die Capabilities-Liste nennt `main` und `spotlight` (`src-tauri/capabilities/default.json`), faktisch läuft aber nur `spotlight` (der „main"-Eintrag ist der Webview-Default-Platzhalter; `open_config_from_tray` zeigt das Spotlight, `overlay.rs:514–520`).

### 1.2 Shellboxen

- Shellboxen sind **keine eigenen Fenster**, sondern CSS-Boxen (`.shell`, `app.css:198`) innerhalb des einen Webviews. Views (FlowRenderer `src/lib/FlowRenderer.svelte`, ConnectorViews `src/lib/ConnectorViews.svelte`, Inventory, Hangar) rendern in die Shell; Modals positionieren sich relativ zur Shell-Box, nicht am Viewport (`ConnectorViews.svelte:1184–1189`).
- Shell-Größe ist **fix, nicht inhaltsgetrieben**: „K2 (t_5fe8eb4c): width FIXED and identical for BOTH windows" (`app.css:170`), Breite über `--w-fixed/--w-cap` mit Zoom-Kompensation `/ var(--ui-scale)` (`app.css:214`).

### 1.3 Tray (`src-tauri/src/tray.rs`, 73 Zeilen)

- Rust-seitiges Menü mit genau **drei Punkten**: `toggle` (Overlay ein-/ausblenden), `config` (Config öffnen), `quit` (`tray.rs:35–39`); Event-Dispatch `tray.rs:49–56`: toggle → `overlay::toggle_visible`, config → `open_config_from_tray`, quit → `app.exit(0)` (sauberer Exit, systemd-Transient-Unit `Restart=no`).
- **Fail-soft:** fehlender Tray-Daemon blockiert den Start nicht (`tray.rs:59–66`). Labels folgen der Sprache, Wechsel erst nach Neustart (`tray.rs:30–34`).
- **Was fehlt für Wunsch 1:** ein eigener Menüpunkt „Fenster zeigen/verstecken" und (optional) eine Beenden-Frage/Quit nur via Tray — heute existiert kein Fenster-Lifecycle-Begriff, der das App-Fenster vom Overlay trennt.

### 1.4 Hotkey-Pfade

- **Global-Shortcuts (app-weit):** `tauri_plugin_global_shortcut`, Registration in `register_global_shortcuts` (`overlay.rs:285`+), Re-Registration bei Rebind (`overlay.rs:375+`, C2a). Diese gelten **fensterunabhängig** (OS-Level `RegisterHotKey` bzw. Linux-Backend des Plugins).
- **Windows-LL-Hook-Fallback:** `src-tauri/src/hotkey_llhook.rs` — eigener `WH_KEYBOARD_LL`-Hook, installiert wenn SC läuft (`game_running=true` → `install`, `false`/Shutdown → `uninstall`, `hotkey_llhook.rs:21–23, 311–317`). Dispatch auf `ShortcutKind::Toggle`/`ClickThrough` mit Rebind-Refresh (`hotkey_llhook.rs:540–546`). Intercept erfolgt **unabhängig vom Fensterfokus** — solange SC läuft, feuern Toggle/Klick-Combo auch dann, wenn SC fokussiert ist.
- **Linux/Hyprland:** Toggle- und Click-Combos werden in den Hyprland-Bind-Store synchronisiert (`hyprland_sync.rs:470–496` `sync_combo`, Drift-Check beim Start `:616`). Das sind **Compositor-Binds**, also ebenfalls fensterunabhängig.
- **Modi F1–F12:** reine **Frontend-Handler** im Webview-Keydown (`+page.svelte:~4625–4650`, Lookup via `lib/hotkeys.ts` `modeKeyMap/hotkeyFor`; ausdrücklich „KEINE Hyprland-Binds — greifen nur solange das Overlay sichtbar+fokussiert ist", `+page.svelte:4618–4620`). Eigene Connector-Hotkey-Slots im selben Handler. → Im Fenstermodus funktioniert der F-Key-Moduswechsel, **solange das Fenster fokussiert ist**.

### 1.5 Game-Detection

- Eigener Crate `crates/game-detection` (`is_game_running`): Linux `/proc/<pid>/cmdline`-Scan, Windows Toolhelp32-Snapshot mit `tasklist`-Fallback (`crates/game-detection/src/lib.rs:5–13, 70, 136+`).
- Poller-Thread alle 5 s (`overlay.rs:1347–1373`), setzt `OverlayState.game_running`; **effektive Sichtbarkeit = `user_visible && game_running`** (`overlay.rs:542–546`).

---

## 2. Entscheidungspunkt 1 — Zweites Fenster vs. Decoration-Toggle am Overlay

### Option A: Eigenes dekorierbares WebviewWindow (EMPFEHLUNG)

Neues Fenster mit Label `window` (z. B. `window`/`appwin`), `WebviewUrl::App("index.html")` — dasselbe dist — plus Marker (Hash-Route `#window` oder Query-Flag `?mode=window`), damit das Frontend `data-window="app"` setzt (Muster existiert, `+page.svelte:3392`).

- Builder: `decorations(true)`, `resizable(true)`, `always_on_top(false)`, `skip_taskbar(false)`, `minimizable(true)`, `closable(true)`, eigene `inner_size`/`position` aus `app_settings`, Monitor per Auswahl (`current_monitor`/Monitor-Enumeration).
- Overlay-Logik (Toggle, Click-Through, Poller, always-on-top) bleibt komplett unberührt — beide Fenster leben nebeneinander, Tauri-Events (`app.emit`) erreichen beide Webviews, `app_settings`-Snapshot ist SharedState.

**Pro:** echte Fenstereigenschaften ohne Eingriff in die Overlay-Verträge; Frontend erkennt Kontext über das vorhandene `label`-Muster; Capabilities-Eintrag ist eine Zeile; Risiken trennbar testen (Phase M1 kann am Overlay vorbei entwickelt werden).
**Contra:** zweiter Webview-Kontext = zweiter Speicher-/Renderkontext (Risiko R1/R2); State-Sync muss explizit bleiben (Emitter ist app-global — ok, `overlay.rs` nutzt `app.emit` bereits); doppelte Boot-Logik im Frontend (Settings-Load, Connector-Refresh laufen pro Webview — Dedup erst mal bewusst in Kauf nehmen, dokumentieren).

### Option B: Decoration-Toggle am bestehenden Overlay-Fenster (VERWORFEN)

Overlay-Fenster dynamisch dekorieren/machen statt zweites Fenster.

**Warum schlecht (belegt):**
1. Das Overlay-Fenster ist **full-monitor, transparent, always-on-top** — der Fenstermodus müsste beim Wechsel fünf Eigenschaften gleichzeitig umschalten (decorations, transparency, always_on_top, Größe/Monitor-Placement, Skip-Taskbar) und sie beim Zurückschalten exakt wiederherstellen — inklusive des Windows-`resizable(false)`+`shadow(false)`-Vertrags (`overlay.rs:247–258`).
2. **Click-Through + GTK-Gate:** `set_ignore_cursor_events` darf erst nach dem ersten Map gesetzt werden (`overlay.rs:478–481`); Zustandswechsel am lebenden Fenster hatten wiederholt Edge-Cases (v2-Ban für workspace/pin-Dispatches `overlay.rs:1732–1747` zeigt, wie anfällig die Placement-Kette ist).
3. **Linux/Wayland:** `resizable(true)` ist am Overlay Pflicht (min=max-Geometrie-Problem, `overlay.rs:245–249`); ein dekorierbares Fenster am selben Label vermischt zwei Widersprüchliche Geometrie-Verträge. Kompositor-Regeln (hyprland.lua, `overlay.rs:927–1054`) sind statisch auf die Overlay-Form gebaut.
4. **Sichtbarkeits-Zustandsmaschine:** `user_visible && game_running` (`overlay.rs:546`) — ein User-Fenster darf NICHT mit dem Spiel verschwinden; im selben Fenster wäre das ein Sonderfall-DSG (State Machine) im Kernpfad.

**Empfehlung: Option A.** Der Overlay-Code ist bewusst klein und hart vertraglich dokumentiert; ein zweites Fenster fügt nur hinzu, statt Verträge umzubiegen.

---

## 3. Entscheidungspunkt 2 — Responsive-Strategie im Fenstermodus

**Frontend-Befund (fix-größenabhängig):**

| Bereich | Befund | Reflow-Kosten |
|---|---|---|
| `.shell` | Breite fix (`--w-fixed/--w-cap`), Zoom-Kompensation `/ --ui-scale`, max-height shell-fähig (`app.css:170, 214`) | **niedrig** — für Fenstermodus `--w-cap` an Fensterbreite koppeln (ein CSS-Zweig unter `data-window="app"`) |
| Rail (Suche/Nav) | flexibler Footer/Leisten-Aufbau, Breite folgt der Shell | niedrig–mittel |
| FlowRenderer (2781 Z.) | Screens/Tabellen rendern in Shell-Kontext, Scroll statt Breiten-Reflow | mittel (Tabellen mit Spaltensätzen) |
| ConnectorViews (1221 Z.) | Modals/Detail-Views shell-konform dimensioniert (`ConnectorViews.svelte:1184–1189`), z. T. Mindestbreiten | mittel–hoch (Detail-/Tabellenscreens breit) |
| Inventory/Hangar | eigene Handler-Ketten, Grid-Layouts | mittel |
| Modals (`shell-cockpitmodal`) | absolute inset:0 an der Shell-Box verankert (`ConnectorViews.svelte:1187–1189`) | niedrig ( folgen der Shell) |

**Bewertung Skalierung vs. Reflow:**
- **Skalierung** (`--ui-scale` hoch/runter je Fensterbreite) ist der billige Weg: vorhandener Mechanismus, kein Layout-Code, konsistentes Aussehen — aber verschwendet Platz auf breiten Fenstern (gleiche Proportionen, mehr Pixel).
- **Reflow** (echte Breit-Layouts, Multi-Column, breite Tabellen) sieht auf 2. Monitor besser aus, kostet aber pro View Layout-Arbeit und Testflächen.

**Empfehlung: gestuft.** M2 liefert „flexibler Container + kompakt/breit-Breakpoint auf Shell-Ebene" (`data-window="app"`-Zweig: Shell darf bis Fensterbreite wachsen, Tabellen/double-column wo trivial). Eine tiefe Per-View-Reflow-Umbau-Planung ist **nicht** Teil dieser Sonde — als eigene Karte nach M1-Probelauf entscheiden, was in welcher View breit darf.

---

## 4. Entscheidungspunkt 3 — Hotkeys + Interaktion im Fenstermodus

**Verträglichkeits-Matrix:**

| Hotkey/Aktion | Mechanismus | Fensterverträglichkeit |
|---|---|---|
| Overlay-Toggle (Strg+Shift+U o. ä.) | GlobalShortcut (`overlay.rs:285+`) / Win-LL-Hook bei laufendem SC (`hotkey_llhook.rs:311+`) / Hyprland-Bind (`hyprland_sync.rs:470`) | ✅ fensterunabhängig (OS-Ebene) — funktioniert auch bei offenem+fokussiertem Fenster |
| Klick-Modus-Toggle | dito (ShortcutKind::ClickThrough, `hotkey_llhook.rs:546`) | ✅ betrifft nur das `spotlight`-Fenster (`set_ignore_cursor_events` nur dort, `overlay.rs:483`) — Fenster bleibt normal klickbar |
| Modi F1–F12 (Suche/Markt/Routen/…) | Frontend-Keydown im Webview (`+page.svelte:4625+`) | ✅ im Fenstermodus wirksam, solange das Fenster fokussiert ist; verhalten sich wie heute im Overlay |
| Connector-Hotkey-Slots | selbe Keydown-Kette (`+page.svelte:4639–4646`) | ✅ wie F-Keys |
| F5 Resync | Frontend (`hotkeys.ts` reserviert) | ✅ wie F-Keys |
| Esc im Fenster | Frontend | ✅ verhält sich wie Overlay (Modus verlassen) — Fenster selbst schließt NICHT (nur Tray), Wunsch-Konform |

**Interaktion:**
- Fokus-Kampf Overlay ↔ Fenster: Overlay-Show dispatcht Compositor-Fokus (`overlay.rs:488, 657+`); öffnet der User das Fenster, gewinnt normal das zuletzt fokussierte Fenster — kein Eingriff nötig. Windows: always-on-top-Reassert gilt nur für Spotlight; das Fenster darf **nicht** on-top sein (Builder-Flag), damit das Overlay über dem Spiel bleibt und das Fenster unter normalen Apps lebt.
- Klick-Modus (`ignore_cursor_events=true`) ändert nichts am Fenster (Eigenschaft ist pro Fenster).

**Kondensat:** keine Pfadkonflikte. Einziger dokumentierter Sonderfall: F-Keys im Fenster sind **fokusgebunden** (wie heute im Overlay) — das ist gewolltes Verhalten, im Fenster-Menü/Help-Text zu dokumentieren.

---

## 5. Entscheidungspunkt 4 — Tray + Fenster-Lebenslauf

**Tray-Erweiterung (`tray.rs`):**
- Neuer Menüpunkt `window` („Fenster zeigen/verstecken") unterhalb `toggle`; Handler → `window::toggle_visible(app)` (neues Modul, analog `overlay::toggle_visible`).
- `quit` bleibt wie gebaut (`app.exit(0)`, `tray.rs:54`) — Wunsch „Beenden weiterhin via Tray oder Button im Fenstermenü": zweiter Beenden-Pfad = Button im Fenster (ruft `app.exit(0)`-IPC oder einen `app_quit`-Command).
- i18n-Label-Pattern beibehalten (Labels beim Build, Neustart-Caveat wie heute dokumentiert).

**Lebenslauf:**
- **CloseRequested → hide, nicht destroy:** `on_window_event`-Handler für das `window`-Label: `CloseRequested` → `api.prevent_close()` + `hide()`. Schließen-Nur-In-Tray ist damit erfüllt.
- **Minimize:** Windows: dekorierter Minimize-Button reicht (`minimizable(true)`). Linux: X11 funktioniert über Dekorationen; **Wayland-Caveat** — GTK-Dekorationen bieten Minimize i. d. R., aber Compositor-Verhalten (CSD vs. SSD, Hyprland-Klient-Decos) ist nicht garantiert; dokumentieren als „Minimieren wo die Desktopumgebung es zulässt" (exakt Wunschtformulierung).
- **Persistenz:** Fensterposition/Größe/Monitor in `app_settings` (neue Felder `window_geometry: {x,y,w,h,monitor_name}`). Beim Wiederherstellen Monitor-Validität prüfen (Monitor weg → Primärmonitor). Speichern on move/resize (debounced).
- **Beide offen gleichzeitig:** unabhängig — Overlay folgt `user_visible && game_running`, Fenster folgt nur Tray. `config_open`-Flag (`overlay.rs:515–519`) bleibt Overlay-lokal. Events erreichen beide Fenster; Visibility-Events des Overlays dürfen den Fenstermodus **nicht** anfassen (gewollte Trennung dokumentieren).

---

## 6. Entscheidungspunkt 5 — Dauerhafte Shellboxen („Pins")

**Ist:** Shellboxen leben im `spotlight`-Webview. Overlay versteckt = Fenster ungemappt → **nichts** sichtbar. Pins brauchen also einen eigenen Sichtbarkeitskanal.

**Optionen:**

| Option | Beschreibung | Bewertung |
|---|---|---|
| **A: Pins-Overlay-Layer (EMPFEHLUNG)** | Ein dediziertes, per-Screen Pin-Webview-Fenster (decorations=false, transparent, always_on_top, skip_taskbar), das nur die gepinnten Boxen rendert; sichtbar, sobald SC läuft ODER User-Pins aktiv — unabhängig vom Overlay-Toggle | ein Fenster (nicht N), komplexe Placement-Logik bleibt überschaubar; Click-Through separat steuerbar |
| B: Pro Pin ein Mini-Fenster | jedes Pin = eigenes 100×80-px-Fenster | N Fenster = N Mal Placement/Compositor-Regeln; Linux-Regeldruck; verwerfen |
| C: Pins im Overlay-Fenster dauerhaft rendern | Overlay nie ganz hide, nur Palette ausblenden | verletzt den sauberen Hide-Vertrag (`apply_visibility`/State-Machine), Mix mit Click-Through gefährlich; verwerfen |

**Mechanik (universell, keine SCLM-Hardcode — Schema-U-Disziplin):**
- Pin-Eintrag = `{ view_id, screen, box_index, opacity, pinned: bool }` in `app_settings` — rein **mechanisch** („diese Ansicht ist gepinnt"), keine Plugin-Semantik. Plugins/Connectors sagen nur, welche Ansichten pin-fähig sind (Deklaration im Manifest oder an der View-Komponente — im Umsetzungsschritt entscheiden; die Pin-Verwaltung selbst ist generisch).
- **Deckkraft:** CSS `opacity` auf dem **Inhalt** der Box (Panel-Container), nicht auf der Hit-Box — Regel: `opacity < 1` dimmt Darstellung; Interaktivität bleibt solange der Box-Rahmen nicht selbst click-through ist.
- **Interaktion/Klick-Modus:** Pins-Fenster startet im **Fokus-Modus** (klickbar) mit pro-Box-Opt-out? Nein — einfacher Vertrag: das Pins-Fenster ist **immer input-transparent** (Click-Through), gepinnte Boxen sind reine **Anzeige** (Handelsroute, Timer, Status). Wer interagieren will, öffnet Overlay/Fenster. Das entspricht der Wunschformulierung („anzeigen") und vermeidet Click-Through-Einzel-Boxen (tao/Wayland kann keine per-Region-Input-Shapes).
  - **Alternative für später (ausdrücklich offen):** „interaktive Pins" via per-Pin-Fenster nur bei echtem Bedarf neu bewerten.
- **Persistenz:** Pins-Set + Deckkraft in `app_settings`, Restore beim Start (sofern SC-Regeln es erlauben — Pins sichtbar erst bei `game_running`, konsistent zur Overlay-Metapher; Off-Schalter in Settings).

---

## 7. Phasenplan (Meilensteine, keine Stundenschätzung)

### M1 — Fenstermodus Grundgerüst
- **Dateien:** `overlay.rs` (oder neues `window_mode.rs`: Builder analog `overlay.rs:232`, label `window`, decorations/on-top-Kontrastverträge dokumentiert), `lib.rs` (Modul + Setup), `tray.rs` (Menüpunkt), `capabilities/default.json` (Label-Eintrag), `+page.svelte`/Frontend (`data-window="app"`-Zweig, Mode-Renderer-Reuse).
- **Abhängigkeiten:** keine (am Overlay vorbei entwickelbar).
- **Risiken:** doppelte Boot-Logik im Frontend (Settings/Connector-Load pro Webview); Hyprland-Regeln treffen das neue Fenster (WM_CLASS/Regel-Scope prüfen — v2-Ban „no pin" gilt nur fürs Overlay, Fenster braucht normale Rules).

### M2 — Responsive Anpassung
- **Dateien:** `app.css` (Shell-Breite `--w-cap` an Fenster binden, kompakt/breit-Breakpoint), gezielte View-Tweaks (FlowRenderer-Tabellen, ConnectorViews-Details).
- **Abhängigkeiten:** M1.
- **Risiken:** Regressionsflächen in der Overlay-Ansicht — der `data-window="spotlight"`-Zweig muss bytegleich bleiben; per Screenshot-Diff-Abnahme (Sebo: Eigenverifikation vor Sicht).

### M3 — Tray + Lebenslauf
- **Dateien:** `tray.rs` (Window-Menüpunkt, i18n-Label), `window_mode.rs` (CloseRequested→hide, Geometry-Persistenz), `app_settings.rs` (neue Felder + Commands), Frontend-Beenden-Button im Fenstermenü.
- **Abhängigkeiten:** M1.
- **Risiken:** Wayland-Minimize-Verhalten (dokumentierte Limitation); Geometry-Restore bei verschwundenem Monitor.

### M4 — Pins + Deckkraft
- **Dateien:** neues `pins.rs` (Pins-Window pro Screen, Placement re-use von `place_on_pointer_monitor`-Logik), Frontend-Pin-Verwaltung (An/Aus + Opacity-Slider in Settings), `app_settings.rs` (Pins-Set), Manifest-Deklaration „pin-fähige Ansichten" (generisch).
- **Abhängigkeiten:** M1 (Fenster-Kontext-Muster), unabhängig von M2/M3 möglich.
- **Risiken:** Click-Through + Always-on-top-Kombination auf Windows (VM-Livetest Pflicht, analog t_bd8cbdfd); zweite transparente Webview = Speicher; Mehrfach-Monitor-Pflege.

---

## 8. Risiko-Tabelle

| # | Risiko | Wahrscheinlichkeit | Auswirkung | Minderung |
|---|---|---|---|---|
| R1 | Zweiter Webview-Kontext: RAM/Renderlast bei Overlay+Fenster+Pins gleichzeitig | mittel | mittel | Fenster hidden → Frontend „sleep"-Zweig (keine Poll-Loops); Pins-Fenster nur bei Bedarf |
| R2 | Hyprland-/Wayland-Regeln greifen falsch aufs neue Fenster | mittel | mittel | eigene WM_CLASS/Regel-Scope, Livetest am Omarchy-PC; Overlay-Verträge (v2-Ban) unangetastet halten |
| R3 | Windows: Topmost-Interplay Overlay vs. Fenster (Fenster unter Spiel?) | niedrig | niedrig | Fenster nie on-top; VM-Livetest mit SC fullscreen |
| R4 | Doppelte Frontend-Boot-Logik (Settings/Connectors zweimal geladen) | sicher | niedrig | bewusst akzeptieren in M1; Dedup nur wenn messbar störend |
| R5 | F-Key-Verwirrung: F-Keys im Fenster fokusgebunden vs. Toggle global | niedrig | niedrig | dokumentieren (Help-Text, SHORTCUTS.md-Erweiterung in M1) |
| R6 | Click-Through+Deckkraft bei Pins (Lesbarkeit über hellem Spiel) | mittel | niedrig | Deckkraft auf Inhalt; Kontrast-Livetest; Regeldoku in M4 |
| R7 | Overlay-Fenster-Verträge versehentlich angeraten (transparent/resizable/placement) | niedrig | hoch | Pins/Fenster in eigenen Modulen; Grep-Tests analog `overlay.rs:1732–1747` erweitern |

---

## 9. Offene User-Fragen (an Sebo)

1. **Fenster-Design:** eigenes Fenster-Icon/Titlebar-Theming (Dark-Mode-konform, Drake-Brand?) oder neutrale OS-Dekorationen?
2. **Standard-Geometrie:** Default-Größe/Position des Fensters (z. B. 1200×800 zentriert auf Primärmonitor) oder direkt „letzte Position merken" ab M1?
3. **Standardmonitor:** Fenster eröffnet auf Monitor unter Mauszeiger, auf Primärmonitor oder auf demselben Monitor wie das Overlay?
4. **Pins-Scope:** Pins für alle Shellboxen/Views freischalten oder zuerst nur eine Whitelist (z. B. Handelsrouten-Detail)?
5. **Pins-Sichtbarkeit ohne Spiel:** sollen Pins auch bei geschlossenem Star Citizen sichtbar bleiben (z. B. Route auf 2. Monitor zum Nachschlagen) oder strikt an `game_running` gekoppelt?
6. **Interaktive Pins:** Einverständnis mit dem Anzeige-Only-Vertrag (§6), oder sollen interaktive Pins (Klick in die gepinnte Box) als spätere Ausbaustufe eingeplant werden?
7. **Tray-Sprachwechsel:** bleibt beim Altwert (Label-Wechsel erst nach Neustart, wie heute dokumentiert) oder sollen die neuen Fenster-/Pin-Labels live nachziehen?

---

### 9.1 Beschlüsse (Sebo, 2026-11-09)

1. **Fenster-Chrom: Variante B** — eigenes Titelband im Overlay-Stil (`decorations: false`), Modus-Umschalter Overlay|Fenster, Beenden im Fenster-Menü, kein ×. Mockup: `mockup-fenstermodus-b.html`.
2. **Geometrie**: sinnvolle Default-Größe/Position; nach Move/Resize wird **immer der letzte Stand beibehalten** (Persistenz ab M1).
3. **Standardmonitor**: Primärmonitor als Default; **zuletzt genutzter Monitor wird gemerkt** und wiederverwendet (Validität prüfen, sonst Primärmonitor).
4. **Pins-Scope: alle** Shellboxen/Views pin-fähig; Feinschliff nach Nutzer-Feedback.
5. **Pins-Sichtbarkeit: strikt an `game_running` gekoppelt.** Der **Fenstermodus ist das einzige Fenster, das ohne Spiel offen bleiben darf** — Pins haben keinen Sichtbarkeitskanal ohne laufendes SC.
6. **Pins-Interaktion: anzeige-only, wenn das Overlay nicht offen ist** (kein Hover/Klick im Click-Through-Kanal). Während das Overlay sichtbar ist, darf Pin-Interaktion (Verschieben/Deckkraft/Schließen) erlaubt sein — präzisierter Vertrag gegenüber §6: Interaktivität nur im Overlay-sichtbaren Zustand.
7. **Tray-Sprachwechsel: live nachziehen, wo technisch möglich** (Tauri-Tray-Labels zur Laufzeit austauschen); wo nicht möglich: beim Sprachwechsel **anbieten**, die App neu zu starten — nicht erzwingen.

## 10. Fazit

Beide Wünsche sind mit vertretbarem Risiko umsetzbar und **ohne** Eingriff in die Overlay-Kernverträge:

- **Fenstermodus** = zweites WebviewWindow (Option A), Andockung über das vorhandene `data-window`-Label-Muster, Hotkey-Verträglichkeit ist per Architektur gegeben (OS-Level-Shortcuts + LL-Hook sind fensterunabhängig).
- **Pins** = dedizierter Pin-Layer pro Screen mit rein mechanischer, generischer Pin-Verwaltung und Deckkraft auf Inhaltsebene (Anzeige-Only-Vertrag).

Entscheidungen, die Omar/Sebo treffen müssen: die offenen Fragen in §9. Umsetzung dann in der Reihenfolge M1 → M2 → M3 → M4 (M3/M4 teilweise parallelisierbar).

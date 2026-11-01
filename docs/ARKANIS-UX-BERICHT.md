# Abschlussbericht — Arkanis-UX-Rework (Branch feature/arkanis-ux)

> **Historisch, Stand 2026-10 (Momentaufnahme zum Zeitpunkt des Berichts).**

Task: t_1a0b0acb — UX-Rework: Arkanis-ähnliche Bedienung (Esc-Toggle, Präfixe, Inline-Expansion, Alt+Zahl)
Branch: `feature/arkanis-ux` (von main @ 441ecc7, kein Push auf main — merge durch Omar)
Commit: `250e4b3` (Feature + Tests + Smoke-Tooling), gepusht nach ssh://100.64.0.3:3222/sebo/architects-overlay.git
Design: Amber/COCKPIT (Vorlage aus Parent t_18dc3c67, app.css als Drop-in übernommen)

## Umgesetzte Features

### 1. Suchfeld (SearchBox)
- Palette: `min-width: min(60vw, 94vw)`, zentriert, `padding-top: 7.5vh` (Vollbild-Spotlight), max 90vw; `.list` scrollt intern ⇒ Suchzeile bleibt oben stehen (sticky-Effekt)
- Suchschrift `max(2.5vh, 1.5rem)`, Debounce 180 ms, AutoFocus beim Öffnen + beim Window-Focus (Arkanis window-focus handler)
- Helper-Zeile unter dem Feld: `:c commodities · :i items · :l locations · = math · % ÷100` (% = /100, sichtbar dokumentiert statt stillschweigend)

### 2. Kategorie-Präfixe + Inline-Rechner (SearchView)
- Jeder Term einzeln geparst: `:c`→commodity, `:i`→item, `:l`→location (mehrfach kombinierbar, dedupliziert); übrige Terme = Fuzzy-Suchtext
- Bare Präfix (`:c` ohne Freitext) = Kategorie-Browse: neuer Cache-Browse-Modus (erste N Einträge der Kategorie, Arkanis-Verhalten), statt leerem Short-Circuit
- `=…` → Inline-Rechner: neues Crate `crates/math-eval` (0 externe Crates; +,-,*,/, postfix %, Klammern, unär -; deutsche Fehlermeldungen, nie Panic), Ergebnis-Chip `= 500.15` statt Ergebnisliste
- Chip `Found N results in X ms` über der Liste (performance.now um den invoke)

### 3. Ergebnisliste (SearchResultStack/Item)
- Vollbreite Zeilen ~66px, Badge `1..9,0` an den ersten 10 Treffern (Badge ausgegraut bei fokussierter Zeile)
- `Alt+Zahl` fokussiert Treffer inkl. Expansion; erneut → zurück ins Suchfeld (Select-All). Badge-Logik: 1→Index 0 … 9→Index 8, 0→Index 9
- Klick/Enter → Inline-Expansion (~3 Zeilen) mit Kauf- und Verkauf-Preistabelle **nebeneinander** (wie PriceTagTable); zweiter Klick/Enter klappt zu
- Stale-Response-Guard (Token) gegen asynchrone get_prices-Antworten, die eine neuere Expansion/geleerte Liste überschreiben

### 4. Fokus-Modell
- Esc-Zustände (Spec §3/§4): ① Ergebnis fokussiert (Alt+Zahl) oder Expansion offen → erst zuklappen + Fokus zurück ins Suchfeld, **kein Hide**; ② Suchfeld fokussiert → Overlay hide (Spotlight) bzw. leeren (Shell); ③ Fokus woanders → Fokus zurück ins Suchfeld (Select-All); nie hide bei fremdem Fokus
- Alt+Zahl/Klick/Enter setzen `document.activeElement` **wirklich** in die Ergebniszeile (`.row` mit `tabindex="-1"`, `focusRow()` in `openRow`/`focusOnly`) — der Zustand „Ergebnis fokussiert" ist nicht mehr nur CSS (`is-focus`), sondern echter DOM-Fokus
- ↑/↓ Navigation, Enter expandieren; Klick auf Hintergrund: kein Auto-Hide
- Zustände: Suchfeld / Ergebnis / nichts

## Gate (alle grün)

- `cargo fmt --all --check` OK
- `cargo check --workspace` OK
- `cargo test --workspace`: **63 Tests, 0 failed** (davon neu: math-eval 8, Cache-Browse 1, search-Filter-Set 1, Prefix-Browse-Command 1)
- `pnpm check`: **0 errors** (28 a11y-Warnungen, vorbestehend: span-Chips, autofocus laut Spec gewollt)
- `pnpm build`: OK (adapter-static, build/ geschrieben)

Behobener Latent-Bug: `Cache::search` band rusqlite-Platzhalter als wiederholtes `?1` — bei Multi-Kind-Queries (2+ Kategorien) schlug das Statement fehl ("Got 3, needed 2"). Platzhalter jetzt eindeutig je Kind (`?1..?N`).

## Smoke-Tests (Live, CDP-Stub-Harness, 1920×1080@2x)

Harness: `docs/scripts/smoke-stub.js` (Tauri-IPC-Stub + Svelte-Logik) + `docs/scripts/smoke-shots.py` (Headless-Chromium/CDP). Screenshots in `docs/screens/`:

| Datei | Zustand | Verifiziert |
|---|---|---|
| 01-idle.png | Leerzustand | Palette ~60vw zentriert, 7.5vh oben, Helper-Zeile, Footer-Hints, FOKUS/KLICK-Toggle |
| 02-search-gold.png | Suche "gold" | 3 Treffer, Badges 1,2,3, Found-Chip "Found 3 results in 1 ms" |
| 03-expanded-gold.png | Zeile 1 expandiert | Kauf/Verkauf-Tabellen nebeneinander, beste Preise hervorgehoben |
| 04-calc.png | `=2*250+15%` | Ergebnis-Chip `= 500.15` |
| 05-prefix-item.png | `:i gold` | nur Gruppe ITEMS (2 Treffer) |
| 06-alt3-focus.png | Alt+3 | dritte Zeile is-focus (Badge 3 ausgegraut), Expansion offen |

Keine Console-Exceptions im Smoke-Lauf. Screenshots zusätzlich per Vision geprüft (Layout, keine Überläufe/Fehlausrichtung).

**Wichtige Einschränkung (Review R1, N2):** `docs/screens/01..06` belegen **nur die Svelte-Logik** — `docs/scripts/smoke-stub.js` erfindet die Suchtreffer (`pool` in Zeile 51ff.) und spiegelt den Rechner in JS (Zeile 89ff.). Der Rust-Pfad und echte UEX-Daten sind damit NICHT belegt; der echte Datenlauf steht im Abschnitt „Review-Runde 2" unten.

## Review-Runde 2 — Auflagen A1/A2 (Task t_30be86f3)

Commit: Fix A1/A2 + Bericht auf `feature/arkanis-ux` (kein Push auf main; Commit-Hash im Kanban-Handoff t_30be86f3). R1-Gate war grün; nach R2 erneut vollständig verifiziert: `cargo fmt/check/test` (63/0) + `pnpm check` (0 errors) + `pnpm build` OK.

### A1 — Unbekanntes Präfix als Fuzzy-Term (Spec §2 / QA #3)
`parseQuery()` wirft unbekannte `:`-Tokens nicht mehr weg (`+page.svelte`, Branch `else if (tok.startsWith(':'))` → `terms.push(tok)`). Vorher/nachher (eigener CDP-Probe, invoke-Mitschnitt):

| Eingabe | Vorher (R1) | Nachher |
|---|---|---|
| `:x gold` | `search({query:"gold"})` — `:x` verschwunden | `search({query:":x gold", filters:null})` |
| `:s ships` | `search({query:"ships"})` — still gemischt | `search({query:":s ships", filters:null})` → ehrlich 0 Treffer |
| `:c gold` | `filters:["commodity"]` + `gold` | unverändert (Kategorie-Pfad intakt) |

Rust-Pfad gegen den echten Cache: `":x gold"` und `":s ships"` → **0 Treffer, keine Exception** (FTS5 verkraftet die Terme; keine neue Fehlerkante, wie im Review angenommen).

### A2 — „Ergebnis fokussiert" ist jetzt ein echter Zustand (Spec §3/§4 / QA #5)
- `escSearch()` behandelt zuerst den dritten Zustand: `focusIdx !== null || expanded !== null` → `clampIdx()` + `focusSearch(true)` + return; erst der nächste Esc blendet aus/leert.
- `.row` hat `tabindex="-1"`; `focusRow(idx)` in `openRow`/`focusOnly` setzt `document.activeElement` wirklich in die Zeile.

Eigene CDP-Messungen (nachher): Alt+3 → `document.activeElement` ist die `.row` (vorher `INPUT`); Esc im Ergebnis-Zustand → 0 `overlay_set_visible(false)`-Calls (vorher 1), Focus zurück im Suchfeld, Expansion zugeklappt (Enter- und Klick-Pfad); vier Verpflichtungs-Wege grün: Esc bei Suchfeld-Fokus → hide; Esc bei Fokus außerhalb → Fokus zurück, kein Hide; Alt+3 erneut → zurück ins Suchfeld; Hintergrund-Klick → kein Hide. Probe: `arkanis-a1a2-probe.py` (20 Checks, 0 failed; vorher 10 failed = exakt die A1/A2-Defekte).

### N1 — `%`-Semantik dokumentiert (nicht stillschweigend)
Entscheidung: Rechner-Grammatik **unverändert gelassen** (`%` = postfix `/100`, per Unit-Test `percent_is_postfix_fraction` fixiert: `=2*250+15%` = 500.15) und die Semantik **sichtbar dokumentiert**: Helper-Zeile zeigt jetzt `% ÷100` (zusätzlich dieser Bericht). Ein stiller Semantikwechsel auf „Prozent vom linken Operanden" (575) hätte den evaluator-getesteten Vertrag geändert und wäre für die Re-Review-Proben überraschend gewesen.

### N2 — Behauptung korrigiert + echter Datenlauf
Siehe Einschränkung oben: Screenshots = Svelte-Logik. Der **echte Datenlauf** (eigener Rust-Probe `real_data_probe.rs` gegen eine SQLite-Backup-Kopie des echten User-Cache `~/.local/share/com.sebo.architects-overlay/cache.sqlite3`, konsistent via `sqlite3 .backup`):

```
count commodities: 205      (Review-Baseline: 205)
count items: 7767           (Review-Baseline: 7.767)
count terminals: 826        (Review-Baseline: 826)
search 'laranite' (real FTS): 4 Treffer (Item #5923/#5924, Commodity #47/#48)
prices 'Laranite' (commodity id 47): 26 Preiszeilen   (Review-Baseline: 26)
  Admin - ARC-L1 buy=7400 sell=0   | Admin - CRU-L1 buy=0 sell=8500   | …
search ":x gold": ok, 0 hits (no exception)
search ":s ships": ok, 0 hits (no exception)
search 'gold': 10 Treffer — "Gold" item, "Gold" commodity, "Gold (Ore)" commodity, …
```

Alle Zahlen decken sich mit der R1-Review-Messung (205/7.767/826 Entities, Laranite 26 Preiszeilen) — echter Rust-Pfad, echte UEX-Cache-Daten.

## Smoke-Screenshots — Wiki-Provider, echte Daten (Task t_7423fa73)

Visueller Nachweis zu Abnahmekriterium 5 (Wiki-Provider, Branch `feature/wiki-chain-fix2` @ `a1a31b0`,
echter App-Lauf mit `ARCH_OVERLAY_SMOKE=1`, echter SQLite-Cache, Quelle WIKI+UEX — **kein Stub**):

| Datei | Inhalt | Belegzahlen |
|---|---|---|
| `docs/screenshots/wiki-provider/levski-location-detail.png` | Levski-Location-Detail: Breadcrumb `Nyx > Levski` (Wiki-Location), Terminal-Aggregat gruppiert (`Conscientious Objects - Levski`, KAUF-Unterbereich mit 7 Preiszeilen), Best Kauf 2 / Best Verkauf 1.300.000, Footer `ZEILE (1/895)` | 895 Preiszeilen über die Levski-Terminals |
| `docs/screenshots/wiki-provider/laranite-detail.png` | Laranite-Commodity-Detail: Bestpreis-Chips BEST KAUF 7.047 / BEST VERKAUF 9.700, Summary-Boxen (Kauf 7.047 @ ArcCorp Mining Area 056 · 806 SCU, Verkauf 9.700 @ Admin - Dudley & Daughters, Marge 2.653/SCU), Kauf-/Verkaufstabelle | `ZEILE (1/26)` — exakt Review-Baseline: 26 Preiszeilen, 6 Kauf-Terminals (7.047–7.541), 20 Verkaufs-Terminals (6.500–9.700) |

Aufnahme: Monitor-Crop auf die Palette (Hyprland/`grim`), bewusst ohne umliegende Desktop-Fenster.

## Offen / Hinweise

- Rechner-Grammatik Phase 1 (kein `**`, keine Funktionen) — bewusst klein gehalten; `%` = `/100` (in Helper-Zeile dokumentiert), Aufschlag via `×1.15` eingeben
- Live-Suche gegen echte UEX-Daten = Rust-Pfad gegen den echten Cache (Abschnitt „Review-Runde 2"); im Browser-Smoke (`docs/screens/01..06`) nur die Svelte-Logik stubbasiert belegt
- QA-Review: Runde 1 t_964fb6ad → Rework t_30be86f3 (A1/A2) → Re-Review nach Freigabe offen

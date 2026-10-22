# Abschlussbericht — Arkanis-UX-Rework (Branch feature/arkanis-ux)

Task: t_1a0b0acb — UX-Rework: Arkanis-ähnliche Bedienung (Esc-Toggle, Präfixe, Inline-Expansion, Alt+Zahl)
Branch: `feature/arkanis-ux` (von main @ 441ecc7, kein Push auf main — merge durch Omar)
Commit: `250e4b3` (Feature + Tests + Smoke-Tooling), gepusht nach ssh://100.64.0.3:3222/sebo/architects-overlay.git
Design: Amber/COCKPIT (Vorlage aus Parent t_18dc3c67, app.css als Drop-in übernommen)

## Umgesetzte Features

### 1. Suchfeld (SearchBox)
- Palette: `min-width: min(60vw, 94vw)`, zentriert, `padding-top: 7.5vh` (Vollbild-Spotlight), max 90vw; `.list` scrollt intern ⇒ Suchzeile bleibt oben stehen (sticky-Effekt)
- Suchschrift `max(2.5vh, 1.5rem)`, Debounce 180 ms, AutoFocus beim Öffnen + beim Window-Focus (Arkanis window-focus handler)
- Helper-Zeile unter dem Feld: `:c commodities · :i items · :l locations · = math`

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
- Esc: Suchfeld fokussiert → Overlay hide (Spotlight) bzw. leeren (Shell); Fokus woanders → Fokus zurück ins Suchfeld (Select-All); nie hide bei fremdem Fokus
- ↑/↓ Navigation, Enter expandieren; Klick auf Hintergrund: kein Auto-Hide
- Zustände: Suchfeld / Ergebnis / nichts

## Gate (alle grün)

- `cargo fmt --all --check` OK
- `cargo check --workspace` OK
- `cargo test --workspace`: **63 Tests, 0 failed** (davon neu: math-eval 8, Cache-Browse 1, search-Filter-Set 1, Prefix-Browse-Command 1)
- `pnpm check`: **0 errors** (29 a11y-Warnungen, vorbestehend: span-Chips, autofocus laut Spec gewollt)
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

## Offen / Hinweise

- Rechner-Grammatik Phase 1 (kein `**`, keine Funktionen) — bewusst klein gehalten
- Live-Suche gegen echte UEX-Daten läuft über die bestehende IPC-Pipeline (Cache); im Browser-Smoke nur stubbasiert
- QA-Review: Kind-Task t_964fb6ad (reviewer) prüft Spec-Konformität

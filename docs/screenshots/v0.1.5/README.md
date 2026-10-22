# Screenshots v0.1.5 — Belegstatus

Alle PNGs in diesem Ordner stammen aus dem **Smoke-Harness** (Tauri-Stub
`docs/scripts/smoke-stub.js` + Headless-Chromium via `frontend-feedback2-probe.py`
bzw. `verify-detail-marking.py`), NICHT aus einem echten Live-Lauf.

Betroffen (Punkte 3/7 betrafen die Live-Pfade):
- `00-item-detail-csp68h.png` — Stub-Pfad. Der Stub sourct seit QA-Runde-2-Fix
  eine `price_sell: null`-Item-Zeile und mappt sie per `sel0()` auf 0; der echte
  Live-Pfad ist separat durch `cargo test -p uex_client --test live_api
  -- --ignored` (6/6 grün inkl. item/terminal/location) belegt — dieses Screenshot
  ist ein UI-Beleg, kein Live-Daten-Beleg.
- `01-detail-commodity-43-rows.png`, `02-lock-filtered-detail.png`,
  `03-market-15-load-more-30.png` — Stub-Fixtures (Commodity 43 Zeilen,
  Location-Lock 30, Market 15→30), unverändert kontrakt-treu wie vor Runde 2.
- `02b-detail-marking.png` — CDP-Nachweis für die deklarative `is-sel`/
  `is-chosen`-Markierung (Punkt 6, QA-Runde-2-Fix), Stub-Harness.

Für Punkt 3/7 gibt es keinen Screenshot aus dem Live-Lauf — der Beleg ist der
grüne Live-Test (live_api.rs), nicht ein Bild.

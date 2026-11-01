# PS: Preisquellen — Belege (t_c84e545d)

Branch: `feature/price-sync-policy` @ a533fad (Historie, Stand 2026-10-29: Gitea-Push per ls-remote verifiziert)

## 1. WIKI entfernt — vorher/nachher

| Datenfluss | Vorher | Nachher |
|---|---|---|
| Locations-Suche (Search) | Wiki-Hierarchie (source=wiki) + UEX-Terminals | SYNTHESIZED aus UEX-Terminals (source=uex) + Terminals |
| Location-Detail | wiki-id → Terminal-Resolver (`terminal_match_tier`/`best_terminal_match`, ~150 LOC) | Direkt (Location-id = Terminal-id) |
| Commodity-Preise | Wiki primär / UEX A/B-Switch + Fallback | UEX (TTL 1 h) |
| Settings „Preis-Quelle (A/B)“ | Wiki/UEX-Chips + Live-Switch | entfernt; alter `commodity_source`-Wert wird ignoriert (unbekannter TOML-Key) |
| wiki-Sync-Task (`WikiSync`) | Commodities + Locations pro 12 h | entfernt (Crate `crates/wiki-client` gelöscht) |

Live-Beleg (`cargo test -p uex_client --test ps_live -- --ignored --nocapture`):
`locations synthesized: 158` · `Levski → id 778 (Nyx > Delamar > Levski, Monetär-Terminal)`.
Funktionserhalt: Suche deckt denselben ID-Raum ab (jeder Wiki-Preisort war UEX-Terminal);
Location-Klick öffnet das Preis-Aggregat direkt (kein Resolver mehr nötig).

## 2. Sync-Policy nach Datentyp

- `TTL_ITEM_PRICES = 7 Tage` (neu, crates/cache) — Item-Preise pro Patch konstant, kein periodischer Fetch.
- `TTL_PRICES = 1 h` — Commodity-Preise.
- **Stündlicher Background-Sweep** in `start_background_refresh` (eigener Tokio-Task, NICHT an
  Overlay-Sichtbarkeit gekoppelt): erneuert jede STALE Commodity-Preistabelle mit einem
  rate-limit-gewachten Call (UEX-Client paces 300 ms/Request). Keine Sichtbarkeits-Gates im Code
  (der Task läuft im Prozess, nicht im Overlay-Show/Hide-Pfad).
- „Zuletzt gesynced“ getrennt: `sync_status` enthält jetzt `item_prices` + `commodity_prices`
  Datensätze (Age aus den `fetched_at`-Markern, `cache::price_sync_ages`).

## 3. Force-Refresh / Detail-F5

- Provider-Trait: `refresh_prices(commodity)` / `refresh_item_prices(item_id)` — Force umgeht den
  Frische-Marker (Test: `refresh_prices_bypasses_freshness_cache` — frischer Marker + gecachte
  Zeile, Force muss die API erreichen, Fehler statt Cache-Bedienung).
- `get_prices(force=true)`: Item-/Commodity-Detail — genau EIN Datensatz live.
- Frontend: Detail-Seiten (Commodity + Item) haben ↻-Knopf; F5 auf offener Detail-Seite ruft
  denselben Pfad (`refreshDetailLive`), Anzeige zeigt „Stand: vor X“.

## 4. Settings „Datenquellen“-Sektion

Alter pro Datentyp + „Commodity-Preise jetzt aktualisieren“ (`refresh_commodity_prices_now`,
derselbe Sweep wie der Stundenzial-Lauf) + „Item-Preise jetzt aktualisieren“
(`refresh_item_prices_now`, sequenziell gepaced, alle gecachten Item-Tabellen).
Kein neuer Sync-Marker `prices_sync_ages` nötig — Age kommt direkt aus dem Cache.

## 5. Gate

- `cargo test`: 262 passed / 0 failed (alle Crates), `cargo fmt --check` clean, `cargo build` clean
- `svelte-check`: 0 Errors (75 Vorab-Warnings unverändert), `pnpm build` OK
- Live: UEX-Locations-Synthese 158 Orte; Force-Refresh gegen echten Feed grün

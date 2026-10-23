# Report — Fix QA-Runde-3-Auflagen (t_0fb56efd), Integrations-Stand `integration/v0.1.8`

**Branch:** `integration/v0.1.8` (gepusht auf origin)
**Basis:** `origin/main` @ **e241339** (Task nannte b868a31; main ist inzwischen um die Windows-CFG-Gate-Fixes 85d9e64/e241339 weiter — Integrat baut bewusst auf dem aktuellen main-HEAD auf)

## Commits auf dem Integrat
| Commit | Inhalt |
|---|---|
| e241339 | Basis (origin/main HEAD) |
| `bf75052` | merge `feature/market-fixes` (96b48d6) en bloc |
| `cad26e2` | merge `feature/ux-feedback3` (+Doku-Commit a0399b8 aus t_0642b2ca, lokal nicht gepusht — als a0399b8 gemergt) |
| `6d2c7ce` | merge `feature/config-view` (bdd6e4c) |
| `5a0bc29` | fix(P3): fetched_at persistiert (localStorage) — Auflage C |
| `3d43eb6` | fix(uex-client): ListingRow String-Felder null-tolerant + Regressionstest — Auflage B |
| `c8a3c96` | fix(integration): Rest-Konfliktmarker/mLimit — Auflage A |
| `7eae1ff` | fix(P8): Variant-Umschaltung sofort auch ohne Backend (lokaler Fallback, Persistenz via settings) |
| `ba9a0f0` | p3-stale-probe (11/11) + ux-Probe-Store-Check angepasst |

## Auflösungsentscheidungen der Konflikte (Auflage A)

### `src/routes/+page.svelte` — merge market-fixes ← ux-feedback3 (3 Hunks)
1. **toggleMarket()**: ux-Phasenlogik (`typing`/`market`) + P3-Stale-Check (`stale → loadMarket('bg')`) **und** market-fixes' P5-Rückweg: `input = ''`, voller Reset auf Defaults (`mSort='newest'`, `mOp='all'`, `mqMin/mqMax=''`, `offers=[]`, `mAllLoaded=false`, Liste/Detail/Calc geräumt) **+ ux' `runSearch()`** auf dem Rückweg (Ergebnisliste wird am (geleerten) Suchfeld ausgerichtet). `phase` geht auf dem Rückweg auf `'typing'` (ux-Semantik), nicht `'idle'`.
2. **fetchMarket()**: `mFetchedAt = resp.fetched_at || nowSec()` (P3) **plus** market-fixes' `mAllLoaded = true` (Provider liefert komplettes abrufbares Fenster — ux' `offers.length < limit` verworfen, sonst „Weitere laden" ohne Wirkung). `marketRegions`/Region-Logik entfernt (P2: `region: ''` im Filter bleibt als Backend-Kontraktfeld, ohne UI/Filterwirkung). ux-Signatur `bg`-Param + market-fixes-Spinnersuppression kombiniert (`if (showAll) mLoadingMore; else if (!bg) mLoading`).
3. **loadMarket()**: ux-Signatur `loadMarket(mode: 'normal'|'bg' = 'normal')` beibehalten; ux' `mLimit`-Progressiv-Logik entfallen (market-fixes paginiert im In-Memory-Snapshot, kein zweiter Fetch — `mLimit` war im Integrat tot und wurde entfernt).

### `src/routes/+page.svelte` + `src/app.css` — merge config-view
- **P8-Detail-Variante**: Der localStorage-Store aus ux-feedback3 wurde durch den **Backend-Settings-Store** aus config-view ersetzt (`settings_get`/`settings_set_detail_variant`, `cfg?.detail_variant`, `data-detail-variant` am html). Die A/C-Buttons im Detail-Kopf blieben, sind jetzt an `chooseVariant('a'|'c')` gebunden; ein lokaler Fallback (`detailVariant = derived(cfg?.detail_variant ?? localVariant ?? 'c')`) macht die Umschaltung auch ohne Backend (statischer Build) sofort wirksam. Kein `architects-overlay.p8.variant`-localStorage-Key mehr.
- **onKey**: config-view's `inConfig`-Guard (Esc → closeConfig, alle anderen Shortcuts gesperrt) VOR ux' P4-F5-Handler — im Config-View ist F5 gesperrt.
- **app.css**: reine Additiv-Kollision — P8-CSS (f5-msg, vC-hero/-kpi/-r) und Config-CSS (gear, pane-config, cfg-*) wurden beide übernommen.

## Auflage B — Live-Markt-Fetch null-tolerant
- `crates/uex-client/src/dto.rs`: alle `String`-Felder des `ListingRow` (`title, description, operation, r#type, price, currency, unit, location, user_name, slug`) jetzt `#[serde(default, deserialize_with = "de_string_null_to_empty")]` (neuer Helper: `null → ""`; `#[serde(default)]` allein fängt explizites `null` nicht ab). Andere Structs unverändert.
- Neues Fixture `tests/fixtures/marketplace_listings_null.json` (2/3 Zeilen `location: null`, 1 Kontrolle mit echter Location, 1× `user_name: null`) + Regressionstest `marketplace_offers_null_location_tolerated` in `tests/integration_uex.rs` (nicht-ignored → fester Teil des Gates).

## Auflage C — P3-Persistenz
`mFetchedAt` ist jetzt **persistiert** (localStorage-Key `architects-overlay.market.fetchedAt`, beim Fetch geschrieben, beim Start geladen) — der Claim „persistiert (localStorage)" stimmt damit, und der Stale-Check überlebt App-Starts.

## Gate-Ergebnisse (alles auf integration/v0.1.8)
| Check | Ergebnis |
|---|---|
| `cargo fmt --check` | OK (leer) |
| `cargo check --workspace --all-targets` | OK, 0 Warnungen |
| `cargo test --workspace` | OK — 65 passed / 0 failed (u.a. 7 ignored-Live via eigenes Crate) |
| `cargo test -p uex_client --test live_api -- --ignored` (`--test-threads=1`) | **7/7 passed** gegen die Live-UEX-API (vorher FAILED: `invalid type: null` → jetzt null-tolerant) |
| `pnpm check` | 0 Errors / 12 Warnings (Bestand, wie auf den Einzelbranches gemeldet) |
| `pnpm build` | OK (adapter-static) |
| `docs/scripts/market-fixes-probe.py` | **23/23 PASS** |
| `docs/scripts/ux-feedback3-probe.py` | **34/34 PASS** (P8-Store-Check an Backend-Persistenz angepasst, siehe oben) |
| `docs/scripts/p3-stale-probe.py` (NEU) | **11/11 PASS**: Fresh-Einstieg normal (1 get_marketplace), fetched_at persistiert + überlebt Reload, stale (>5 min) Re-Entry nach Markt-Verlassen → neuer `get_marketplace` im **bg-Pfad, ohne Spinner**, Liste danach gefüllt, fetched_at aktualisiert |

Screenshots: `docs/screens/` (15–18 market, p3-01…03) und `docs/screenshots/ux-feedback3/` (01–04, inkl. P8 Variante C/A).

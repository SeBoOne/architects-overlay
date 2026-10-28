//! Trade-data IPC layer: the commands the frontend palette calls.
//!
//! Three public commands plus one event:
//!
//! - `search(query, filter)` — cache-only FTS lookup, grouped into
//!   Commodities / Items / Locations. Never touches the network, so the
//!   overlay stays instant (< 50 ms is the goal; FTS5 prefix search is
//!   typically sub-ms). Freshness of the underlying reference data is the
//!   background sync task's job, not the keystroke path's.
//! - `get_prices(commodity, id?, kind?)` — price detail behind one expanded
//!   search row. `kind: "location"` (+ terminal id) returns the location
//!   aggregate grouped per terminal; `kind: "item"` (+ item id) returns an
//!   empty table (items are no commodities); otherwise per-commodity price
//!   points (provider policy: serve cache while ≤ 30 min old, otherwise
//!   fetch).
//! - `sync_status()` / `sync_now()` — reference-data freshness, entity
//!   counts, sync-in-progress and last error; `sync_now` triggers the
//!   on-demand sweep the UI needs for the first sync and the "Erneut
//!   versuchen" action.
//! - `get_marketplace(filters)` — live/cached marketplace offers with
//!   client-side region filter + price sort; `source` tells the UI whether the
//!   data came from the fresh short-TTL snapshot ("cache"), a live fetch
//!   ("live") or the last snapshot after a failed live fetch ("offline").
//! - `open_marketplace_listing(slug)` — opens the listing's uexcorp.space page
//!   in the system default browser (tauri-plugin-opener, external URL — no app
//!   navigation). Read-only, no write endpoints called anywhere in Phase 2.
//!
//! Errors cross the IPC boundary as human-readable German strings (the
//! design contract wants rate limits etc. in the user's language, e.g.
//! `HTTP 429 · Rate-Limit (Retry in 34 s)`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cache::{Cache, CacheError};
use provider::{DataProvider, EntityKind, ProviderError, SearchHit};
use tauri::{AppHandle, Emitter, State};

/// Tauri event fired after every background sync sweep run (start or finish)
/// so the frontend can drop its spinner without polling.
pub const SYNC_EVENT: &str = "trade-sync";

/// Group order and labels per the design contract: Commodities → Items →
/// Locations, each with a counter. Each group maps onto ONE OR MORE cache
/// datasets: the location group searches BOTH the wiki location hierarchy
/// (EntityKind::Locations, source=wiki, rich parent-child data) and the UEX
/// terminal dataset (trade terminals). Commodities stay on the UEX id space
/// — the wiki commodity rows in the cache are internal resolution data for
/// the wiki price path, not search results.
const GROUPS: &[(&str, &str, &[EntityKind])] = &[
    ("commodity", "Commodities", &[EntityKind::Commodities]),
    ("item", "Items", &[EntityKind::Items]),
    (
        "location",
        "Locations",
        &[EntityKind::Locations, EntityKind::Terminals],
    ),
];

/// Max hits served per group (keeps the palette compact and the IPC tiny).
const PER_GROUP_LIMIT: u32 = 12;

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

/// Bookkeeping for the sync sweeps, shared between the spawned sweep task and
/// the status command.
pub struct SyncTracker {
    pub syncing: AtomicBool,
    pub last_error: Mutex<Option<String>>,
    /// Guards the one-time start of the background TTL refresh loop.
    pub background: AtomicBool,
}

impl Default for SyncTracker {
    fn default() -> Self {
        Self {
            syncing: AtomicBool::new(false),
            last_error: Mutex::new(None),
            background: AtomicBool::new(false),
        }
    }
}

/// App-managed trade state. The provider is behind `Arc<dyn DataProvider>`
/// so a future second source slots in behind the same seam.
pub struct TradeState {
    pub provider: Arc<dyn DataProvider>,
    pub cache: Arc<Cache>,
    pub tracker: Arc<SyncTracker>,
    /// Extra wiki-dataset sync (commodities + locations, own TTLs). `None`
    /// in tests with stub providers.
    pub wiki_sync: Option<Arc<dyn crate::providers::WikiSync>>,
    /// M2b-Write-Gate (Rate-Limit-Sicherheit, Anforderung 5): verhindert, dass
    /// die UI mehr als ~2 Schreibaktionen pro Minute an UEX feuert, auch wenn
    /// sie theoretisch mehr erlaubt. Jede schreibende Aktion (Anlegen/Abmelden)
    /// muss vor dem Request hier passieren.
    pub write_gate: Arc<WriteGate>,
}

/// Clientseitiges Rate-Limit-Fenster für MARKTPLATZ-WRITES (M2b, Anforderung
/// 5). Erlaubt höchstens `MAX_WRITES_PER_WINDOW` Writes in
/// `WINDOW_SECS` (Fenster gleitet: ein Kandidat wird abgelehnt, wenn im
/// Zeitfenster [now-WINDOW, now] schon `MAX` erfolgreiche Writes registriert
/// sind). Das UI darf theoretisch mehr erlauben — dieses Gate greift als
/// letzte Hürde, inklusive deutscher Meldung mit Wartezeit.
///
/// Thread-sicher (std Mutex), nie über await gehalten.
#[derive(Debug)]
pub struct WriteGate {
    /// Unix-Sekunden der durchgelassenen Writes (Zeitfenster gleitet).
    recent: Mutex<Vec<u64>>,
    max_per_window: usize,
    window_secs: u64,
}

/// Default-Fenster gemäß Entscheidung: max 2 Writes pro Minute.
pub const WRITE_WINDOW_SECS: u64 = 60;
pub const WRITE_MAX_PER_WINDOW: usize = 2;

impl WriteGate {
    pub fn new() -> Self {
        WriteGate {
            recent: Mutex::new(Vec::new()),
            max_per_window: WRITE_MAX_PER_WINDOW,
            window_secs: WRITE_WINDOW_SECS,
        }
    }

    /// Test-Konstruktor mit explizitem Fenster (winziges Fenster ohne echtes
    /// Warten). Nur im Test-Build — die App nutzt immer `new()` (2/60).
    #[cfg(test)]
    pub fn with_window(max_per_window: usize, window_secs: u64) -> Self {
        WriteGate {
            recent: Mutex::new(Vec::new()),
            max_per_window,
            window_secs,
        }
    }

    /// Versucht, einen Write zu registrieren. `Err` = Rate-Limit greift (die
    /// Meldung enthält die Sekunden bis zum nächsten freien Slot). `Ok` = der
    /// Write läuft; `record_write` nach dem request braucht nur bei Erfolg
    /// aufgerufen zu werden, damit fehlgeschlagene Writes das Fenster NICHT
    /// belegen (kein Retry-Spam + kein Selbst-Sperren nach Fehlern).
    pub fn try_acquire(&self, now_secs: u64) -> Result<(), String> {
        let mut recent = self.recent.lock().expect("write gate mutex poisoned");
        // Fenster gleiten lassen: alte Einträge rauswerfen.
        let cutoff = now_secs.saturating_sub(self.window_secs);
        recent.retain(|&t| t > cutoff);
        if recent.len() >= self.max_per_window {
            // Nächster freier Slot = ältester Eintrag + Fensterlänge.
            let oldest = recent.iter().min().copied().unwrap_or(now_secs);
            let wait = oldest + self.window_secs - now_secs;
            return Err(format!(
                "Rate-Limit: maximal {MAX} Schreibaktionen pro Minute — bitte {wait} s warten.",
                MAX = self.max_per_window
            ));
        }
        Ok(())
    }

    /// Nach einem ERFOLGREICHEN Write registrieren (bewusst NUR Erfolg —
    /// Fehler belegen das Fenster nicht, sonst würde ein 429 den Client für
    /// das ganze Fenster blockieren und einen Retry-Spam erzwingen).
    pub fn record_write(&self, now_secs: u64) {
        let mut recent = self.recent.lock().expect("write gate mutex poisoned");
        let cutoff = now_secs.saturating_sub(self.window_secs);
        recent.retain(|&t| t > cutoff);
        recent.push(now_secs);
    }
}

impl Default for WriteGate {
    fn default() -> Self {
        WriteGate::new()
    }
}

// ---------------------------------------------------------------------------
// IPC payloads
// ---------------------------------------------------------------------------

#[derive(Clone, serde::Serialize)]
pub struct GroupHits {
    /// "commodity" | "item" | "location".
    pub kind: String,
    pub label: String,
    pub hits: Vec<SearchHit>,
}

#[derive(Clone, serde::Serialize)]
pub struct SearchResponse {
    pub groups: Vec<GroupHits>,
    pub total: usize,
}

#[derive(Clone, serde::Serialize)]
pub struct PriceDetail {
    pub commodity: String,
    pub source: String,
    /// unix seconds when the response was assembled.
    pub fetched_at: u64,
    pub prices: Vec<provider::PricePoint>,
    /// Set (and `prices` flattened from it) when `kind: "location"` was
    /// requested: the location aggregate grouped by terminal. Null on the
    /// commodity/item path.
    pub location: Option<provider::LocationPrices>,
}

#[derive(Clone, serde::Serialize)]
pub struct DatasetStatus {
    pub kind: String,
    pub label: String,
    pub fresh: bool,
    /// Seconds since the dataset was fetched (`None` = never).
    pub age_secs: Option<u64>,
    pub count: u64,
}

#[derive(Clone, serde::Serialize)]
pub struct SyncStatus {
    pub provider: String,
    pub syncing: bool,
    /// `true` until a sync attempt fails (the UI shows the Offline banner on
    /// `false`). Optimistic at boot so a fresh install is not immediately
    /// labelled offline before the first sweep has run.
    pub online: bool,
    pub last_error: Option<String>,
    pub datasets: Vec<DatasetStatus>,
}

/// Marketplace response payloads (mirrored as TS types in +page.svelte).
#[derive(Clone, Debug, serde::Serialize)]
pub struct MarketResponse {
    /// "cache" = served from the fresh short-TTL snapshot · "live" = freshly
    /// fetched · "offline" = last snapshot after the live fetch failed.
    pub source: String,
    /// unix seconds the snapshot/response was assembled.
    pub fetched_at: u64,
    pub offers: Vec<provider::Offer>,
    /// Non-fatal note (e.g. last live fetch failed, showing stale snapshot).
    pub note: Option<String>,
}

/// „Mein Shop" — eigene Offers (M2a): gleiche Quellenkennung wie
/// [`MarketResponse`]. `key_set` teilt der UI mit, ob ein API-Key gesetzt ist
/// (ohne ihn bricht der User-scoped Read; die UI zeigt dann den Settings-Hin-
/// weis statt einer rohen Fehlermeldung).
#[derive(Clone, serde::Serialize)]
pub struct OwnOffersResponse {
    pub source: String,
    pub fetched_at: u64,
    pub offers: Vec<provider::Offer>,
    pub note: Option<String>,
    pub key_set: bool,
}

/// „Mein Shop" — letzte Trades (M2a): letzte-5-Logik übernimmt die UI (der
/// Cache liefert den vollständig geladenen Verlauf); hier kommen die Trades
/// des Caches/Live-Fetches.
#[derive(Clone, Debug, serde::Serialize)]
pub struct TradesResponse {
    pub source: String,
    pub fetched_at: u64,
    pub trades: Vec<provider::Trade>,
    pub note: Option<String>,
    pub key_set: bool,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Instant, cache-only search. `filters`: subset of ["commodity"|"item"|
/// "location"] — the palette's category chips AND `:c`/`:i`/`:l` prefix terms
/// (Arkanis-style). Empty = all categories.
#[tauri::command]
pub fn search(
    state: State<'_, TradeState>,
    query: String,
    filters: Option<Vec<String>>,
) -> Result<SearchResponse, String> {
    let query = query.trim();
    let filter_set = filters
        .unwrap_or_default()
        .into_iter()
        .collect::<std::collections::HashSet<String>>();
    // Empty query without category filters = nothing to show. With filters
    // (bare `:c`/`:i`/`:l`) the cache browse-mode lists the category instead.
    if query.is_empty() && filter_set.is_empty() {
        return Ok(SearchResponse {
            groups: Vec::new(),
            total: 0,
        });
    }

    let mut groups = Vec::new();
    let mut total = 0usize;
    for (kind, label, entity_kind) in GROUPS {
        if !filter_set.is_empty() && !filter_set.contains(*kind) {
            continue;
        }
        // Commodities/Items serve only UEX rows (source filter); the
        // location group serves both datasets (wiki locations + UEX
        // terminals) — kinds and sources are disjoint per kind.
        let sources: &[&str] = if *kind == "location" {
            &[]
        } else {
            &[cache::SOURCE_UEX]
        };
        // P1 (Feedback-Runde 4): in der Location-Gruppe zählt für die
        // Location-ENTITY nur der NAME (Subtitle/Kette matcht nicht mehr —
        // Mercy Hospital taucht bei `levski` nicht mehr auf). Terminals
        // behalten den vollen FTS-Match (Name-Suffix "- Levski" folgt dem
        // Ort).
        let name_only: &[EntityKind] = if *kind == "location" {
            &[EntityKind::Locations]
        } else {
            &[]
        };
        let mut hits = state
            .cache
            .search_sourced(entity_kind, sources, query, name_only, PER_GROUP_LIMIT)
            .map_err(|e| format!("Cache-Fehler: {e}"))?;
        // Location-Gruppe (Kriterium 2 / QA-Auflage A): die Standort-ENTITY
        // selbst ist kein eigenes Dataset — `/terminals` liefert nur Terminal-
        // Reihen (EntityKind::Terminals). Damit der Standort-Klick das Aggregat
        // (je Terminal eine Gruppe) öffnen kann, synthetisieren wir je Standort-
        // Kette (subtitle, "… > Checkmate Station") EINE klickbare Standort-
        // Zeile mit kind='location' + id des Monetär-/Handels-Terminals der
        // Kette. Die Terminal-Zeilen bleiben kind='terminal' (eigene Preise,
        // Kriterium 1). Ohne diese Zeile wäre `kind='location'` im Frontend
        // toter Code (Reviewer t_9608f33c, Auflage 1).
        if *kind == "location" {
            hits =
                with_location_rows(&state.cache, hits).map_err(|e| format!("Cache-Fehler: {e}"))?;
        }
        total += hits.len();
        groups.push(GroupHits {
            kind: (*kind).to_string(),
            label: (*label).to_string(),
            hits,
        });
    }

    Ok(SearchResponse { groups, total })
}

/// Standort-Synthese (Kriterium 2 / QA-Auflage A): liefert je Standort-Kette
/// eine zusätzliche `kind='location'`-Trefferzeile. Der Cache-Datensatz kennt
/// nur Terminals (EntityKind::Terminals), keine Standort-Entity; damit der
/// Standort-Klick das Aggregat (je Terminal eine Gruppe) öffnen kann, gruppiert
/// diese Funktion die Terminal-Treffer nach ihrer Kette (subtitle,
/// „… > Checkmate Station") und stellt pro Kette EINE `kind='location'`-Zeile
/// voran, deren `id` ein Terminal der Kette ist (das Monetär-/Handels-Terminal
/// der Kette, so es erkennbar ist; sonst das erste der Kette). Die Terminal-
/// Zeilen bleiben unangetastet (`kind='terminal'`, Kriterium 1).
fn with_location_rows(
    cache: &Cache,
    hits: Vec<provider::SearchHit>,
) -> Result<Vec<provider::SearchHit>, CacheError> {
    if hits.is_empty() {
        return Ok(hits);
    }
    // P1.2 (Feedback-Runde 4): ein getroffener Standort listet seine
    // Station-Shops als eigene Terminal-Zeilen — auch wenn das Terminal
    // selbst nicht per FTS matcht. Membership generisch: die Ortsname ist
    // ein Segment der Terminal-Kette ("Pharmacy - Levski" ↔ "Nyx > Levski").
    let wiki_locations: Vec<provider::SearchHit> = hits
        .iter()
        .filter(|h| h.kind == "location")
        .cloned()
        .collect();
    let mut shops: Vec<provider::SearchHit> = Vec::new();
    if !wiki_locations.is_empty() {
        let terminals: Vec<provider::SearchHit> = cache
            .entities_of(EntityKind::Terminals)?
            .into_iter()
            .map(|e| provider::SearchHit {
                id: e.id,
                name: e.name,
                kind: "terminal".to_string(),
                subtitle: e.subtitle,
                code: e.code,
            })
            .collect();
        let present: std::collections::HashSet<u64> = hits.iter().map(|h| h.id).collect();
        for loc in &wiki_locations {
            for t in &terminals {
                if present.contains(&t.id) || shops.iter().any(|s| s.id == t.id) {
                    continue;
                }
                if chain_has_location(&t.subtitle, &loc.name) {
                    shops.push(t.clone());
                }
            }
        }
    }
    // Kette (subtitle) → Terminals der Kette, in Treffer-Reihenfolge zuerst.
    // Wiki-Location-Zeilen (kind='location', source=wiki) sind bereits
    // Standort-Zeilen und werden nicht noch einmal synthetisiert — sie
    // bleiben unangetastet in der Trefferliste.
    let wiki_names: Vec<String> = wiki_locations
        .iter()
        .map(|h| h.name.to_lowercase())
        .collect();
    let mut by_chain: std::collections::BTreeMap<String, Vec<provider::SearchHit>> =
        std::collections::BTreeMap::new();
    for h in hits.iter().filter(|h| h.kind == "terminal") {
        let chain = h.subtitle.trim();
        if chain.is_empty() {
            continue;
        }
        by_chain
            .entry(chain.to_string())
            .or_default()
            .push(h.clone());
    }
    // Pro Kette eine Standort-Zeile. `id` = Monetär-/Handels-Terminal der Kette
    // (Namen mit Handels-Hinweis bevorzugt; Fallback: erster Treffer der Kette).
    // Der Name der Standort-Entity ist das letzte Kettenglied (z. B.
    // "Checkmate Station") — nicht der Terminal-Name —, damit er klar von den
    // Terminal-Zeilen (Kriterium 1) unterschieden wird.
    //
    // P1: matcht die Wiki-Location bereits per Name, wird die synthetische
    // Zeile nicht DUPLIZIERT (gleicher Ortsname → Wiki-Treffer reicht).
    let mut locations: Vec<provider::SearchHit> = Vec::new();
    for (chain, members) in &by_chain {
        let display = location_name(chain);
        if wiki_names.iter().any(|n| *n == display.to_lowercase()) {
            continue;
        }
        let (chain_lookup, first) = chain_lookup(cache, chain)?;
        let repr = members
            .iter()
            .chain(chain_lookup.iter())
            .find(|t| is_money_terminal(&t.name))
            .or_else(|| members.first());
        let id = match repr {
            Some(t) => t.id,
            None => first.id,
        };
        locations.push(provider::SearchHit {
            id,
            name: display,
            kind: "location".to_string(),
            subtitle: chain.clone(),
            code: None,
        });
    }
    // Reihenfolge: Wiki-Location-Treffer mit ihren Station-Shops zuerst
    // (das Aggregat ist der präferierte Einstieg, die Shops folgen direkt),
    // dann die synthetischen Ketten-Standorte, dann die restlichen
    // Terminal-Treffer (die als Shop-Zeile ergänzten wurden nicht doppelt
    // geführt).
    let shop_ids: std::collections::HashSet<u64> = shops.iter().map(|s| s.id).collect();
    let mut out: Vec<provider::SearchHit> = Vec::new();
    for loc in &wiki_locations {
        out.push(loc.clone());
        out.extend(
            shops
                .iter()
                .filter(|s| chain_has_location(&s.subtitle, &loc.name))
                .cloned(),
        );
    }
    out.extend(locations);
    out.extend(hits.into_iter().filter(|h| match h.kind.as_str() {
        "terminal" => !shop_ids.contains(&h.id),
        // Wiki-Location-Zeilen sind oben schon eingereiht.
        "location" => false,
        _ => true,
    }));
    Ok(out)
}

/// Ist `location_name` ein Segment der Terminal-Kette `chain`
/// ("Stanton > Nyx > Levski" enthält "Levski")? Generische Orts-Mitgliedschaft
/// für die Station-Shop-Erweiterung (P1.2) — kein Namens-Blacklist.
fn chain_has_location(chain: &str, location_name: &str) -> bool {
    let name = location_name.trim();
    !name.is_empty()
        && chain
            .split('>')
            .map(str::trim)
            .any(|seg| seg.eq_ignore_ascii_case(name))
}

/// Anzeigename der Location-Entity. Pro Standort-Kette steht der Name der
/// Station (das letzte Kettenglied, z. B. "Checkmate Station") statt eines
/// Terminal-Namens — das ist die zusätzliche Aggregations-Metapher, die
/// `kind='location'` klickbar macht, ohne sich mit den Terminal-Zeilen
/// (Kriterium 1) zu vermengen.
///
/// QA Runde 2 (N2): der frühere Wert war eine Konstante ("Admin Office") für
/// JEDE Kette — zwei Stationen in der Liste hießen gleich. Jetzt kommt der
/// Name aus der Kette selbst, damit sich Standorte unterscheiden. Fallback auf
/// "Station" bei leerer Kette (sollte nie eintreten: leere Ketten werden oben
/// übersprungen).
fn location_name(chain: &str) -> String {
    chain
        .rsplit('>')
        .map(str::trim)
        .find(|s| !s.is_empty())
        .unwrap_or("Station")
        .to_string()
}

/// Bestes Monetär-/Handels-Terminal einer Kette. Bevorzugt einen Treffer der
/// Live-Suche (FTS-Rank); fehlt dort ein Handels-Hinweis, wird die volle
/// Terminal-Menge der Kette aus dem Cache herangezogen, damit die repräsentative
/// id nie außerhalb liegt (Checkmate: 436 „Commodity Shop - Admin").
fn chain_lookup(
    cache: &Cache,
    chain: &str,
) -> Result<(Vec<provider::SearchHit>, provider::SearchHit), CacheError> {
    let entities = cache.entities_of(EntityKind::Terminals)?;
    let mut members: Vec<provider::SearchHit> = Vec::new();
    for e in entities {
        if e.subtitle == chain {
            members.push(provider::SearchHit {
                id: e.id,
                name: e.name,
                kind: "terminal".to_string(),
                subtitle: e.subtitle,
                code: e.code,
            });
        }
    }
    members.sort_by_key(|m| m.id);
    let first = members
        .first()
        .cloned()
        .unwrap_or_else(|| provider::SearchHit {
            id: 0,
            name: chain.to_string(),
            kind: "location".to_string(),
            subtitle: chain.to_string(),
            code: None,
        });
    Ok((members, first))
}

/// Heuristik für „Handels-/Monetär-Terminal": Name enthält einen Handels-,
/// Shop-, Verwaltungs- oder Konsolen-Hinweis. Docks (Fuel/Refinery) fallen
/// durch — reine Fracht-Terminals sollen das Aggregat nicht anführen.
fn is_money_terminal(name: &str) -> bool {
    let n = name.to_lowercase();
    [
        "commodity",
        "admin",
        "trading",
        "shop",
        "console",
        "store",
        "terminal",
    ]
    .iter()
    .any(|k| n.contains(k))
}

/// Evaluate a math expression for the inline calculator (`=2*250+15%`).
/// Returns the computed value as a human-readable German string (never an
/// exception — malformed input becomes an error string the UI can show).
#[tauri::command]
pub fn math_eval(expr: String) -> Result<String, String> {
    let expr = expr.trim_start_matches('=').trim();
    match math_eval::evaluate(&expr) {
        Ok(v) if v.is_finite() => Ok(fmt_number(v)),
        Ok(_) => Err("Ergebnis ist nicht endlich (Division durch 0?)".to_string()),
        Err(e) => Err(e),
    }
}

/// German number formatting for the calculator result (no trailing zeros).
fn fmt_number(v: f64) -> String {
    let mut s = format!("{v}");
    // Collapse "0.30000000000000004"-style float noise to a clean short form.
    if s.contains('.') {
        let truncated = format!("{:.10}", v);
        let mut t = truncated.trim_end_matches('0');
        if t.ends_with('.') {
            t = &t[..t.len() - 1];
        }
        // Prefer the short form only when it round-trips (avoids 14 → 13.99999).
        s = t.to_string();
    }
    s
}

/// Price details behind one expanded search row. The frontend routes by the
/// hit's `kind`:
///
/// * `kind = "terminal"` (+ `id` = terminal id): prices of THIS terminal
///   (buy/sell per commodity) — the row-specific detail, NOT the location
///   chain aggregate.
/// * `kind = "location"` (+ `id` = terminal id): the location aggregate —
///   every terminal of the location chain with its own buy/sell table, in
///   `location.groups` (feedback fix "Preise nicht geladen").
/// * `kind = "item"` (+ `id` = item id): items that are not commodities
///   carry their shop prices in `/items_prices` — fetched via
///   `provider.item_prices(id)` (cached 30 min). Point 3: items now show
///   real buy/sell prices instead of the empty "nicht kaufbar" table.
/// * default (or `kind = "commodity"`): per-commodity price points, cached
///   30 min, refetched live when stale. May also resolve to `Ok(empty)` for
///   a known item/terminal passed by name (defensive fallback).
#[tauri::command]
pub async fn get_prices(
    state: State<'_, TradeState>,
    commodity: String,
    id: Option<IdArg>,
    kind: Option<String>,
) -> Result<PriceDetail, String> {
    let fetched_at = now_unix();
    let id: Option<u64> = id.and_then(IdArg::into_u64);
    match kind.as_deref() {
        Some("terminal") => {
            // One terminal's OWN price table (run: prices of THIS terminal,
            // not the shared location chain aggregate). Resolve the display
            // name by id; the provider caches per-terminal (30 min).
            let terminal_id =
                id.ok_or_else(|| "Terminal öffnen: fehlende Terminal-ID".to_string())?;
            let name = terminal_name(&state, terminal_id, &commodity).await?;
            let prices = state
                .provider
                .terminal_prices(terminal_id)
                .await
                .map_err(friendly)?;
            Ok(PriceDetail {
                commodity: name,
                source: state.provider.name(),
                fetched_at,
                prices,
                location: None,
            })
        }
        Some("location") => {
            let terminal_id =
                id.ok_or_else(|| "Location öffnen: fehlende Terminal-ID".to_string())?;
            let location = state
                .provider
                .location_prices(terminal_id)
                .await
                .map_err(friendly)?;
            let mut prices = Vec::new();
            for group in &location.groups {
                prices.extend(group.prices.iter().cloned());
            }
            Ok(PriceDetail {
                commodity: location.terminal.clone(),
                source: state.provider.name(),
                fetched_at,
                prices,
                location: Some(location),
            })
        }
        Some("item") => {
            // Items that are NOT commodities carry their shop prices in the
            // UEX `/items_prices` table (weapons, armor, MedPens, etc.).
            // Fetch those live (cached 30 min) instead of returning an empty
            // table — Point 3: "falsches 'nicht kaufbar'".
            let item_id = id.ok_or_else(|| "Item öffnen: fehlende Item-ID".to_string())?;
            let item_name = item_name(&state, Some(item_id), &commodity).await?;
            let prices = state
                .provider
                .item_prices(item_id)
                .await
                .map_err(friendly)?;
            Ok(PriceDetail {
                commodity: item_name,
                source: state.provider.name(),
                fetched_at,
                prices,
                location: None,
            })
        }
        _ => {
            let prices = state.provider.prices(&commodity).await.map_err(friendly)?;
            Ok(PriceDetail {
                commodity: prices
                    .first()
                    .map(|p| p.commodity.clone())
                    .unwrap_or(commodity),
                source: state.provider.name(),
                fetched_at,
                prices,
                location: None,
            })
        }
    }
}

/// t_925a577a: RPC-ID-Argument, das sowohl die bisherigen JSON-Numbers als
/// auch Strings akzeptiert (Wiki-Entity-IDs > 2^53 müssen als String durch
/// die JS-WebView, sonst runden sie still).
#[derive(serde::Deserialize)]
#[serde(untagged)]
pub enum IdArg {
    Num(u64),
    Str(String),
}

impl IdArg {
    fn into_u64(self) -> Option<u64> {
        match self {
            IdArg::Num(v) => Some(v),
            IdArg::Str(s) => s.parse().ok(),
        }
    }
}

/// Resolve an item's display name (by id, falling back to the raw query).
async fn item_name(state: &TradeState, id: Option<u64>, raw: &str) -> Result<String, String> {
    let Some(item_id) = id else {
        return Ok(raw.to_string());
    };
    let entities = state
        .cache
        .entities_of(EntityKind::Items)
        .map_err(|e| format!("Cache-Fehler: {e}"))?;
    Ok(entities
        .iter()
        .find(|e| e.id == item_id)
        .map(|e| e.name.clone())
        .unwrap_or_else(|| raw.to_string()))
}

/// Resolve a terminal's display name (by id, falling back to the raw query).
async fn terminal_name(state: &TradeState, id: u64, raw: &str) -> Result<String, String> {
    let entities = state
        .cache
        .entities_of(EntityKind::Terminals)
        .map_err(|e| format!("Cache-Fehler: {e}"))?;
    Ok(entities
        .iter()
        .find(|e| e.id == id)
        .map(|e| e.name.clone())
        .unwrap_or_else(|| raw.to_string()))
}

/// Snapshot of cache freshness / sync state for the footer status line.
#[tauri::command]
pub fn sync_status(state: State<'_, TradeState>) -> SyncStatus {
    build_status(&state.provider, &state.cache, &state.tracker)
}

/// Trigger the on-demand sync sweep (first sync, "Erneut versuchen"). Runs as an
/// async command so the sweep's tokio-based provider futures execute on Tauri's
/// async runtime; the [`SYNC_EVENT`] event announces completion so the UI can
/// drop its spinner.
#[tauri::command]
pub async fn sync_now(app: AppHandle, state: State<'_, TradeState>) -> Result<SyncStatus, String> {
    let provider = state.provider.clone();
    let cache = state.cache.clone();
    let tracker = state.tracker.clone();
    let handle = app.clone();

    // Only one sweep at a time; a concurrent call just re-reads the status.
    if !tracker.syncing.swap(true, Ordering::SeqCst) {
        let outcome = sweep(
            provider.as_ref(),
            cache.as_ref(),
            state.wiki_sync.as_deref(),
            false,
        )
        .await;
        {
            let mut guard = tracker.last_error.lock().expect("tracker mutex poisoned");
            *guard = outcome.err();
        }
        tracker.syncing.store(false, Ordering::SeqCst);
        let _ = handle.emit(SYNC_EVENT, build_status(&provider, &cache, &tracker));
    }
    Ok(build_status(&state.provider, &state.cache, &state.tracker))
}

/// Marketplace offers, filtered client-side (q/location/region/sort) by the
/// provider. Served from the short-TTL cache when fresh; falls back to the last
/// snapshot (marked `source: "offline"`) when the live fetch fails but a
/// snapshot exists — the market view stays usable without a connection.
#[tauri::command]
pub async fn get_marketplace(
    state: State<'_, TradeState>,
    filters: provider::MarketplaceFilter,
) -> Result<MarketResponse, String> {
    // Was the snapshot already fresh before the call? Then the provider serves
    // it from cache (live only when item_id/operation filter forces a fetch).
    let cache_fresh = state.cache.marketplace_fresh().unwrap_or(false);
    match state.provider.marketplace_offers(filters.clone()).await {
        Ok(offers) => Ok(MarketResponse {
            source: if cache_fresh {
                "cache".to_string()
            } else {
                "live".to_string()
            },
            fetched_at: now_unix(),
            offers,
            note: None,
        }),
        Err(e) => {
            // Offline / rate-limited: serve the last snapshot if we have one,
            // else surface the error.
            match state.cache.marketplace_snapshot() {
                Ok(snapshot) if !snapshot.is_empty() => Ok(MarketResponse {
                    source: "offline".into(),
                    fetched_at: now_unix(),
                    // The offline fallback must obey the same filter + sort
                    // contract as every live/cache path (apply_local lives
                    // behind the provider seam) — a raw snapshot here looked
                    // like "filters ignored" whenever a live fetch failed.
                    offers: provider::apply_marketplace_filters(snapshot, &filters),
                    note: Some(friendly(e)),
                }),
                _ => Err(friendly(e)),
            }
        }
    }
}

/// Eigene Offers + Trades (M2a, Read-Phase, „Mein Shop") — je eigener Command
/// mit Quellenkennung cache/live/offline wie get_marketplace. Kein Schreib-
/// zugriff in dieser Phase; Deep-Links gehen an die UEX-Seite.
#[tauri::command]
pub async fn get_my_offers(
    state: State<'_, TradeState>,
    username: String,
) -> Result<OwnOffersResponse, String> {
    get_own_offers_cmd(&state, &username).await
}

/// Eigene Offers laden mit stale-first-Cache + offline-Fallback. `key_set`
/// wird bei true nur dann zur Fehlerdifferenzierung benutzt (kein Key → die
/// UI zeigt den Settings-Hinweis statt der rohen UEX-Auth-Fehlermeldung).
async fn get_own_offers_cmd(
    state: &State<'_, TradeState>,
    username: &str,
) -> Result<OwnOffersResponse, String> {
    let key_set = api_key_set();
    // Gleiche username-bewusste Frischeprüfung wie im Provider-Fresh-Shortcut:
    // nur wenn TTL frisch UND der Snapshot zu genau diesem User gehört,
    // liefert der Provider aus dem Cache (source "cache"). Sonst läuft
    // der LIVE-Fetch — die Kennung stimmt also immer mit dem tatsächlichen
    // Datenweg überein.
    let cache_fresh = state.cache.own_offers_fresh_for(username).unwrap_or(false);
    match state.provider.my_offers(username.to_string()).await {
        Ok(offers) => Ok(OwnOffersResponse {
            source: if cache_fresh {
                "cache".into()
            } else {
                "live".into()
            },
            fetched_at: now_unix(),
            offers,
            note: None,
            key_set,
        }),
        Err(e) => {
            // Offline / Auth: letzter Snapshot NUR, wenn er zu genau diesem
            // Username gehört (username-bewusst wie der Fresh-Pfad). Ein nach
            // Kontowechsel mit Netz-/Auth-Fehler aufgefrischter View darf die
            // Offers des VORHERIGEN Accounts nicht als „offline"-Eigendaten
            // bekommen — sonst als leerer Fall an den Fehlerpfad.
            match state.cache.own_offers_snapshot_for(username) {
                Ok(Some(snapshot)) if !snapshot.is_empty() => Ok(OwnOffersResponse {
                    source: "offline".into(),
                    fetched_at: now_unix(),
                    offers: snapshot,
                    note: Some(friendly(e)),
                    key_set,
                }),
                _ => Err(friendly(e)),
            }
        }
    }
}

/// Eigene Trades laden (M2a) — stale-first + offline-Fallback.
#[tauri::command]
pub async fn get_my_trades(state: State<'_, TradeState>) -> Result<TradesResponse, String> {
    let key_set = api_key_set();
    let cache_fresh = state.cache.own_trades_fresh().unwrap_or(false);
    match state.provider.user_trades().await {
        Ok(trades) => Ok(TradesResponse {
            source: if cache_fresh {
                "cache".into()
            } else {
                "live".into()
            },
            fetched_at: now_unix(),
            trades,
            note: None,
            key_set,
        }),
        Err(e) => match state.cache.own_trades_snapshot() {
            Ok(snapshot) if !snapshot.is_empty() => Ok(TradesResponse {
                source: "offline".into(),
                fetched_at: now_unix(),
                trades: snapshot,
                note: Some(friendly(e)),
                key_set,
            }),
            _ => Err(friendly(e)),
        },
    }
}

/// Open the user's own trade history page on uexcorp.space (the UEX
/// marketplace manager / account page). A per-trade deep-link isn't
/// documented; the trade LOG page is the continuation point. Pure external
/// navigation, nothing written.
#[tauri::command]
pub fn open_marketplace_trades(app: AppHandle) -> Result<String, String> {
    let url = "https://uexcorp.space/marketplace/".to_string();
    let _ = &app;
    tauri_plugin_opener::open_url(&url, None::<&str>)
        .map_err(|e| format!("Browser nicht erreichbar: {e}"))?;
    Ok(url)
}

/// M3a: ein Listing-Foto als Base64-Data-URI holen (für den Angebots-Detail).
/// Der CDN liefert `application/octet-stream` — Chromiums ORB blockt direkte
/// <img>-Ladungen in der Webview (live belegt), daher proxied das Backend.
/// Hardening + Sniff liegen in `uex_client::fetch_listing_photo`.
#[tauri::command]
pub async fn marketplace_photo(_app: AppHandle, url: String) -> Result<String, String> {
    uex_client::fetch_listing_photo(&url)
        .await
        .map_err(|e| friendly(e))
}

/// Open one listing's page on uexcorp.space in the system default browser.
/// URL scheme verified against the live site (October 2026): both item and
/// service listings live under `/marketplace/item/info/{slug}/`. No slug → the
/// marketplace home. Pure external navigation; nothing is written to UEX.
#[tauri::command]
pub fn open_marketplace_listing(app: AppHandle, slug: String) -> Result<String, String> {
    let url = listing_deep_link(&slug);
    // Free-form opener API (no scope check — the URL comes from our own cache,
    // not from user/remote input). Opens in the system default browser.
    let _ = &app; // AppHandle kept for signature symmetry with sibling commands
    tauri_plugin_opener::open_url(&url, None::<&str>)
        .map_err(|e| format!("Browser nicht erreichbar: {e}"))?;
    Ok(url)
}

/// Build the external deep link for a listing slug.
fn listing_deep_link(slug: &str) -> String {
    let slug = slug.trim();
    if slug.is_empty() {
        "https://uexcorp.space/marketplace/".to_string()
    } else {
        format!("https://uexcorp.space/marketplace/item/info/{slug}/")
    }
}

/// M2b-Review-Fix (t_6e26ec82 R2): öffnet den Permalink eines frisch
/// ANGELEGTEN Listings (Create-Antwort liefert eine volle URL, kein Slug).
/// Nur vertrauenswürdige uexcorp.space-URLs werden geöffnet — kein externes
/// Öffnen beliebiger URLs. Grundlage: `POST /marketplace_advertise/` antwortet
/// mit `{id, url}` (nicht mit der vollständigen ListingRow).
#[tauri::command]
pub fn open_created_listing(_app: AppHandle, url: String) -> Result<String, String> {
    let url = url.trim().to_string();
    if !url.starts_with("https://uexcorp.space/") {
        return Err("Nur uexcorp.space-Links werden geöffnet.".into());
    }
    tauri_plugin_opener::open_url(&url, None::<&str>)
        .map_err(|e| format!("Browser nicht erreichbar: {e}"))?;
    Ok(url)
}

// ---------------------------------------------------------------------------
// M2b — Write-Layer: Anlegen + Abmelden (2-Schritt-Bestätigung, kein Edit,
// Rate-Limit-Gate, kein Retry-Spam; Fehlerpfad: Meldung + Deep-Link-Button).
// ---------------------------------------------------------------------------

/// Write-Layer (M2b): ein neues Marketplace-Listing anlegen
/// (`POST /marketplace_advertise/`). Payload exakt nach Doku-Schema
/// ([`provider::AdvertisePayload`]) — kein Edit (Bearbeiten = Abmelden + Neu).
///
/// Sicherheits- & Fehlerverhalten (beschlossen):
/// * 2-Schritt-Bestätigung erledigt die UI (diese Karte: Settings-Einstellung
///   `marketplace_confirm`, Default every_action); das Backend erzwingt nur
///   das Rate-Limit-Gate.
/// * Rate-Limit: max ~2 Writes/min via [`WriteGate`] — auch wenn das UI mehr
///   erlaubt. Fehlgeschlagene Writes belegen das Fenster NICHT.
/// * KEIN Retry bei 429/403/Fehler — der Fehler steigt als deutsche Meldung
///   durch; die UI zeigt die Meldung + Deep-Link-Button zur passenden
///   UEX-Seite (hier: Marketplace-Home, da das Listing evtl. nicht existiert).
async fn advertise_offer_cmd(
    state: &State<'_, TradeState>,
    payload: provider::AdvertisePayload,
) -> Result<provider::AdvertiseResult, String> {
    let now = now_unix();
    state.write_gate.try_acquire(now)?;
    let res = state
        .provider
        .advertise_offer(payload)
        .await
        .map_err(friendly)?;
    // Nur erfolgreiche Writes registrieren (Fehler belegen das Fenster nicht).
    state.write_gate.record_write(now);
    Ok(res)
}

/// Tauri-Command: Anlegen (M2b). Validierung der Pflichtfelder liegt im
/// Provider (deutsche Config-Meldung); sonstige Fehler = deutsche Meldung.
#[tauri::command]
pub async fn marketplace_advertise(
    state: State<'_, TradeState>,
    payload: provider::AdvertisePayload,
) -> Result<provider::AdvertiseResult, String> {
    advertise_offer_cmd(&state, payload).await
}

/// M2b: ein Listing ABMELDEN (`DELETE /marketplace_listings/?id=…`). Kein
/// Edit — nur löschen. Rate-Limit-Gate wie beim Anlegen; kein Retry-Spam.
#[tauri::command]
pub async fn marketplace_delete(state: State<'_, TradeState>, id: u64) -> Result<(), String> {
    let now = now_unix();
    state.write_gate.try_acquire(now)?;
    state.provider.delete_offer(id).await.map_err(friendly)?;
    state.write_gate.record_write(now);
    Ok(())
}

/// M2b-Review-Fix (t_6e26ec82): Marketplace-Kategorien für die Anlegen-Form.
/// `POST /marketplace_advertise/` verlangt `id_category` für `type=item` —
/// die Form braucht die Auswahl (id + Anzeigename), damit jeder Item-Create
/// ein gültiges Payload bekommt (vorher hardcodiert `id_category: null` →
/// UEX 400 missing_id_category).
#[tauri::command]
pub async fn get_categories(
    state: State<'_, TradeState>,
) -> Result<Vec<provider::Category>, String> {
    state.provider.categories().await.map_err(friendly)
}

/// M2b: gibt das persistierte Bestätigungs-Modell zurück (UI entscheidet den
/// 2-Schritt). Der volle Settings-Snapshot (`settings_get`) trägt
/// `marketplace_confirm` bereits — dieser einfache Command ist für Probes.
#[tauri::command]
pub fn marketplace_confirm_mode(state: State<'_, TradeState>) -> String {
    let _ = &state;
    settings::Settings::load()
        .map(|s| s.marketplace_confirm().as_str().to_owned())
        .unwrap_or_else(|_| "every_action".to_owned())
}

/// Item-Auswahl für das Listing-Formular (MC, t_5faaf946): Typeahead über
/// `EntityKind::Items` aus dem lokalen Cache (UEX /items wird beim Sync
/// kategorienweise geladen). Analog `location_suggest` (routes.rs): Ranking
/// starts-with vor contains, Case-insensitiv, max 12 Treffer — nur Items,
/// keine Commodities (die /items-Rows mit is_commodity=1 werden beim Import
/// schon gefiltert, siehe uex-client). Leerer Query → leere Liste.
#[tauri::command]
pub fn item_suggest(
    state: State<'_, TradeState>,
    query: String,
) -> Result<Vec<provider::ItemSuggestion>, String> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let entities = state
        .cache
        .entities_of(EntityKind::Items)
        .map_err(|e| format!("Cache-Fehler: {e}"))?;
    let mut starts: Vec<provider::ItemSuggestion> = Vec::new();
    let mut contains: Vec<provider::ItemSuggestion> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for e in entities {
        let name = e.name.trim();
        let key = name.to_lowercase();
        if key.is_empty() || seen.contains(&key) {
            continue;
        }
        seen.insert(key.clone());
        let hit = provider::ItemSuggestion {
            id: e.id,
            name: name.to_string(),
            subtitle: e.subtitle.clone(),
        };
        if key.starts_with(&q) {
            starts.push(hit);
        } else if key.contains(&q) {
            contains.push(hit);
        }
    }
    starts.append(&mut contains);
    starts.truncate(12);
    Ok(starts)
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// Start the background freshness/health loop, exactly once (guarded by
/// `tracker.background`). Safe from any thread: it spawns onto Tauri's own
/// async (Tokio) runtime via `tauri::async_runtime`, so the tokio-based
/// provider futures have a reactor even when the loop is booted from the
/// app-setup (main) thread — the setup thread itself has no Tokio context.
///
/// Booted once from `setup_trade` — **not** lazily from a command. That was the
/// B1 bug: with a warm cache and a dead network the loop never started, so the
/// `online` flag (driven by `tracker.last_error`) stayed optimistic green.
///
/// The first tick probes reachability almost immediately, independent of the
/// cache TTL, and every outcome is written into `tracker.last_error` and
/// broadcast via [`SYNC_EVENT`], so the frontend's `OFFLINE · CACHE …` banner
/// and `online` status update live.
pub fn start_background_refresh(
    app: &AppHandle,
    provider: &Arc<dyn DataProvider>,
    cache: &Arc<Cache>,
    wiki_sync: Option<&Arc<dyn crate::providers::WikiSync>>,
    tracker: &Arc<SyncTracker>,
) {
    if tracker.background.swap(true, Ordering::SeqCst) {
        return;
    }
    let provider = provider.clone();
    let cache = cache.clone();
    let tracker = tracker.clone();
    let handle = app.clone();
    // Marketplace-Sync (Phase 2): kurze TTL (5 min) → eigener, schneller Zyklus,
    // damit der Boards-Snapshot nahezu frisch bleibt. Rate-Limit-schonend: ein
    // /marketplace_listings-Request pro TTL-Fenster (≈12/h), plus der einmalige
    // Star-System-Fetch pro 12h. Fehler sind hier nicht fatal — der nächste Tick
    // versucht es erneut und die UI fällt auf den letzten Snapshot zurück.
    let mprovider = provider.clone();
    let mcache = cache.clone();
    let mwiki_sync = wiki_sync.cloned();
    _ = tauri::async_runtime::spawn(async move {
        // Erste Probe kurz nach Boot: Netz-Erreichbarkeit prüfen, unabhängig
        // von der TTL — ein frischer Cache mit totem Netz muss trotzdem als
        // offline erkannt werden.
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        background_tick(&tracker, &provider, &cache, mwiki_sync.clone(), &handle).await;
        loop {
            tokio::time::sleep(cache::sync::DEFAULT_INTERVAL).await;
            background_tick(&tracker, &provider, &cache, mwiki_sync.clone(), &handle).await;
        }
    });

    _ = tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(MARKET_REFRESH_INTERVAL).await;
            match (
                mcache.marketplace_fresh(),
                mprovider
                    .marketplace_offers(provider::MarketplaceFilter::default())
                    .await,
            ) {
                (Ok(true), _) => log::debug!("market: snapshot fresh, skip"),
                (_, Err(e)) => log::warn!("market: refresh failed: {e}"),
                (_, Ok(offers)) => log::info!("market: cached {} offers", offers.len()),
            }
        }
    });
}

/// Cadence of the background marketplace refresh (= the marketplace TTL, so we
/// never hammer the API: exactly one listing fetch per fresh window + failures
/// are retried on the next tick).
const MARKET_REFRESH_INTERVAL: std::time::Duration = match cache::TTL_MARKETPLACE {
    Some(d) => d,
    None => std::time::Duration::from_secs(300),
};

/// One background iteration: refresh stale datasets (computed via
/// [`sweep`] with `probe_if_fresh`), record the outcome and broadcast it.
async fn background_tick(
    tracker: &Arc<SyncTracker>,
    provider: &Arc<dyn DataProvider>,
    cache: &Arc<Cache>,
    wiki_sync: Option<Arc<dyn crate::providers::WikiSync>>,
    handle: &AppHandle,
) {
    let outcome = sweep(
        provider.as_ref(),
        cache.as_ref(),
        wiki_sync.as_deref(),
        true,
    )
    .await;
    {
        let mut guard = tracker.last_error.lock().expect("tracker mutex poisoned");
        *guard = outcome.err();
    }
    let _ = handle.emit(SYNC_EVENT, build_status(provider, cache, tracker));
}

/// Re-fetch every reference dataset older than its TTL. Stops at the first
/// failure and reports it (the sweep is all-or-nothing so the tracker stays
/// truthful about the "online" flag).
///
/// `probe_if_fresh`: when *nothing* is stale (warm cache), re-fetch the
/// commodities dataset once anyway as a real connectivity probe — the background
/// loop uses this so a blocked network is noticed even right after a clean
/// sync; the on-demand `sync_now` path passes `false` (it may not waste an
/// API quota on fresh data).
async fn sweep(
    provider: &dyn DataProvider,
    cache: &Cache,
    wiki_sync: Option<&dyn crate::providers::WikiSync>,
    probe_if_fresh: bool,
) -> Result<(), String> {
    let mut fetched = false;
    for (_kind, label, entity_kinds) in GROUPS {
        for entity_kind in *entity_kinds {
            let fresh = cache
                .is_fresh(*entity_kind)
                .map_err(|e| format!("Cache-Fehler: {e}"))?;
            if fresh {
                continue;
            }
            fetched = true;
            log::info!("sync: fetching {label} (stale)");
            let entities = provider
                .fetch_entities(*entity_kind)
                .await
                .map_err(friendly)?;
            // Wiki datasets (Locations) store under their own source so they
            // coexist with the UEX terminal rows in one table.
            let source = match entity_kind {
                EntityKind::Locations => cache::SOURCE_WIKI,
                _ => cache::SOURCE_UEX,
            };
            cache
                .store_entities_sourced(*entity_kind, source, &entities)
                .map_err(|e| format!("Cache-Fehler: {e}"))?;
            log::info!("sync: {label} stored ({} entries)", entities.len());
        }
    }
    // Wiki reference datasets beyond the GROUPS kinds (wiki commodities for
    // the price path + an extra locations pass with its own TTL marker).
    if let Some(wiki_sync) = wiki_sync {
        wiki_sync
            .sync_wiki_datasets(cache)
            .await
            .map_err(friendly)?;
    }
    if probe_if_fresh && !fetched {
        // Warm cache → only a forced round-trip detects an outage. Stored, so
        // the probe is not pure waste (keeps commodities fresh).
        let entities = provider
            .fetch_entities(EntityKind::Commodities)
            .await
            .map_err(friendly)?;
        cache
            .store_entities(EntityKind::Commodities, &entities)
            .map_err(|e| format!("Cache-Fehler: {e}"))?;
    }
    Ok(())
}

fn build_status(
    provider: &Arc<dyn DataProvider>,
    cache: &Arc<Cache>,
    tracker: &SyncTracker,
) -> SyncStatus {
    let mut datasets = Vec::new();
    for (kind, label, entity_kinds) in GROUPS {
        // A group's freshness = ALL its datasets fresh (worst case); the
        // count sums across datasets (wiki locations + UEX terminals).
        let mut fresh = true;
        let mut age_secs = None;
        let mut count = 0u64;
        for entity_kind in *entity_kinds {
            if !cache.is_fresh(*entity_kind).unwrap_or(false) {
                fresh = false;
            }
            if let Some(a) = cache.dataset_age(*entity_kind).unwrap_or(None) {
                age_secs = Some(age_secs.map_or(a, |old: u64| old.max(a)));
            }
            count += cache.count_entities(*entity_kind).unwrap_or(0);
        }
        datasets.push(DatasetStatus {
            kind: (*kind).to_string(),
            label: (*label).to_string(),
            fresh,
            age_secs,
            count,
        });
    }
    // Live snapshot of the worker's last error. The GUI thread (commands run on
    // the main thread under WebKitGTK) must NEVER block on the async worker's
    // mutex — a stuck sync must not freeze the overlay, so read with try_lock:
    // if the worker is mid-write, show no error and stay optimistic.
    let last_error: Option<String> = tracker
        .last_error
        .try_lock()
        .ok()
        .and_then(move |guard| guard.clone());
    let online = last_error.is_none();
    SyncStatus {
        provider: provider.name(),
        syncing: tracker.syncing.load(Ordering::SeqCst),
        last_error,
        online,
        datasets,
    }
}

/// Map a seam error to a German, user-facing message (design contract).
pub(crate) fn friendly(e: ProviderError) -> String {
    match e {
        ProviderError::RateLimit { retry_after } => match retry_after {
            Some(secs) => format!("HTTP 429 · Rate-Limit (Retry in {secs} s)"),
            None => "HTTP 429 · Rate-Limit erreicht".to_string(),
        },
        ProviderError::Http { status, body }
            if (status == 400 || status == 403)
                && (body.contains("user_not_found") || body.contains("invalid_secret_key")) =>
        {
            // M2a-Fix (t_2f197d1c): UEX-Auth-Codes der User-scoped Reads —
            // deutscher Hinweis statt Rohfehler-Status/Text.
            "Secret-Key fehlt oder ist falsch — siehe Einstellungen („UEX persönlicher Secret-Key“)."
                .to_string()
        }
        ProviderError::Http { status, .. } => {
            format!("HTTP {status} · UEX nicht erreichbar")
        }
        ProviderError::Timeout => "Zeitüberschreitung · UEX nicht erreichbar".to_string(),
        ProviderError::NotFound(what) => format!("Nicht gefunden: {what}"),
        ProviderError::Cache(msg) => format!("Cache-Fehler: {msg}"),
        ProviderError::Config(msg) => format!("Konfigurationsfehler: {msg}"),
        ProviderError::Json(msg) => format!("Ungültige Antwort: {msg}"),
        ProviderError::Unsupported(msg) => format!("Nicht unterstützt: {msg}"),
        _ => e.to_string(),
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Ist ein UEX-API-Key am Settings-Slot gesetzt? (M2a: ohne Key liefert die
/// UI einen sauberen Settings-Hinweis statt der rohen UEX-Auth-Meldung.)
fn api_key_set() -> bool {
    settings::Settings::load()
        .ok()
        .map(|s| s.uex_api_key().is_some())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    /// Regression (t_925a577a): get_prices nimmt die Row-ID als String ODER
    /// Number — Wiki-Location-IDs (> 2^53) müssen als String durch die
    /// JS-WebView, sonst rundet JS still (…35130 → …35100) und der
    /// Location→Terminal-Resolve bricht mit "not in reference data".
    #[test]
    fn id_arg_accepts_string_and_number() {
        let s: IdArg = serde_json::from_str("\"404707429735935130\"").unwrap();
        let n: IdArg = serde_json::from_str("778").unwrap();
        assert_eq!(s.into_u64(), Some(404707429735935130));
        assert_eq!(n.into_u64(), Some(778));
        let junk: Result<IdArg, _> = serde_json::from_str("\"not-an-id\"");
        assert!(junk.is_ok() && junk.unwrap().into_u64().is_none());
    }
    use super::*;
    use tauri::Manager;
    use tauri::test::mock_app;
    // M2a-Tests (Mein Shop): eigene Offers/Trades nutzen die Seam-Typen.
    use provider::{ListingType, MarketOperation};

    #[test]
    fn friendly_rate_limit_is_german_and_actionable() {
        assert_eq!(
            friendly(ProviderError::RateLimit {
                retry_after: Some(34)
            }),
            "HTTP 429 · Rate-Limit (Retry in 34 s)"
        );
        assert!(friendly(ProviderError::Timeout).contains("UEX"));
        assert!(
            friendly(ProviderError::Http {
                status: 500,
                body: "x".into()
            })
            .contains("500")
        );
    }

    /// M2a-Fix (t_2f197d1c): UEX-Auth-Codes der User-scoped Reads (400
    /// user_not_found / 403 invalid_secret_key) → deutsche Meldung mit
    /// Settings-Verweis statt Rohfehler (kein „HTTP … nicht erreichbar“).
    #[test]
    fn friendly_maps_secret_key_auth_codes_german() {
        let msg_400 = friendly(ProviderError::Http {
            status: 400,
            body: "user_not_found".into(),
        });
        let msg_403 = friendly(ProviderError::Http {
            status: 403,
            body: "invalid_secret_key".into(),
        });
        for m in [&msg_400, &msg_403] {
            assert!(
                m.contains("Secret-Key fehlt oder ist falsch") && m.contains("Einstellungen"),
                "unexpected friendly(): {m}"
            );
            assert!(!m.contains("HTTP"), "raw status leaked: {m}");
        }
        // Andere 400/403 ohne Auth-Code bleiben beim bisherigen Mapping.
        let plain = friendly(ProviderError::Http {
            status: 403,
            body: "forbidden".into(),
        });
        assert!(plain.contains("HTTP 403"));
        // UEX kapselt die Codes auch als envelope.status (body == status).
        let enveloped = friendly(ProviderError::Http {
            status: 400,
            body: "{\"status\":\"user_not_found\"}".into(),
        });
        assert!(enveloped.contains("Secret-Key fehlt oder ist falsch"));
    }

    /// The grouped search contract: three groups in a fixed order, per-group
    /// limit, filter removes groups, and the location group maps onto the
    /// terminal dataset.
    #[test]
    fn group_contract_is_stable() {
        assert_eq!(GROUPS[0].1, "Commodities");
        assert_eq!(GROUPS[1].1, "Items");
        assert_eq!(GROUPS[2].1, "Locations");
        // The location group serves BOTH datasets: wiki locations (rich
        // hierarchy) first, then the UEX terminal dataset.
        assert_eq!(GROUPS[2].2, &[EntityKind::Locations, EntityKind::Terminals]);
        assert_eq!(GROUPS.len(), 3);
    }

    /// Empty queries (incl. whitespace) return an empty response without
    /// touching the cache.
    #[test]
    fn search_empty_query_is_not_an_error() {
        let app = mock_app();
        app.manage(TradeState {
            provider: Arc::new(StubProvider),
            cache: Arc::new(Cache::open_in_memory().unwrap()),
            tracker: Arc::new(SyncTracker::default()),
            wiki_sync: None,
            write_gate: Arc::new(WriteGate::new()),
        });
        let state = app.state::<TradeState>();

        for q in ["", "   ", "\t"] {
            let resp = search(state.clone(), q.into(), None).unwrap();
            assert!(resp.groups.is_empty(), "query {q:?} must yield no groups");
            assert_eq!(resp.total, 0);
        }
        let _ = app; // state borrow keeps app alive for the duration
    }

    /// `search` accepts a set of category filters (from `:c`/`:i`/`:l` prefix
    /// terms) — an empty set means all categories come along.
    #[test]
    fn search_accepts_multiple_category_filters() {
        let app = mock_app();
        app.manage(TradeState {
            provider: Arc::new(StubProvider),
            cache: Arc::new(Cache::open_in_memory().unwrap()),
            tracker: Arc::new(SyncTracker::default()),
            wiki_sync: None,
            write_gate: Arc::new(WriteGate::new()),
        });
        let state = app.state::<TradeState>();

        // None → every group is requested (all three present).
        let resp = search(state.clone(), "gold".into(), None).unwrap();
        assert_eq!(resp.groups.len(), 3, "no filter ⇒ all groups");

        // A single filter keeps one group.
        let resp = search(
            state.clone(),
            "gold".into(),
            Some(vec!["commodity".to_string()]),
        )
        .unwrap();
        assert_eq!(resp.groups.len(), 1);
        assert_eq!(resp.groups[0].kind, "commodity");

        // Two filters keep those two groups, in GROUPS order.
        let resp = search(
            state.clone(),
            "gold".into(),
            Some(vec!["item".to_string(), "location".to_string()]),
        )
        .unwrap();
        assert_eq!(resp.groups.len(), 2);
        assert_eq!(resp.groups[0].kind, "item");
        assert_eq!(resp.groups[1].kind, "location");

        let _ = app;
    }

    /// A bare category prefix (`:c` without free text) must reach the cache's
    /// browse path — the empty-query short-circuit only fires without filters.
    #[test]
    fn empty_query_with_category_filter_browses_not_shortcircuits() {
        let app = mock_app();
        app.manage(TradeState {
            provider: Arc::new(StubProvider),
            cache: Arc::new(Cache::open_in_memory().unwrap()),
            tracker: Arc::new(SyncTracker::default()),
            wiki_sync: None,
            write_gate: Arc::new(WriteGate::new()),
        });
        let state = app.state::<TradeState>();

        // No filter + empty query → nothing (old short-circuit preserved).
        let resp = search(state.clone(), "".into(), None).unwrap();
        assert_eq!(resp.total, 0);
        assert!(resp.groups.is_empty());

        // Filter + empty query → the filtered group is served (browse mode;
        // empty in-memory cache yields 0 hits, but the group must exist).
        let resp = search(
            state.clone(),
            "".into(),
            Some(vec!["commodity".to_string()]),
        )
        .unwrap();
        assert_eq!(resp.groups.len(), 1);
        assert_eq!(resp.groups[0].kind, "commodity");
        assert_eq!(resp.total, 0);

        // Filter + empty query + another category → both groups, GROUPS order.
        let resp = search(
            state.clone(),
            "".into(),
            Some(vec!["item".to_string(), "location".to_string()]),
        )
        .unwrap();
        assert_eq!(resp.groups.len(), 2);
        assert_eq!(resp.groups[0].kind, "item");
        assert_eq!(resp.groups[1].kind, "location");

        let _ = app;
    }

    /// Kriterium 2 (echte Suchkette): `search` synthetisiert für jede
    /// Standort-Kette eine `kind='location'`-Zeile mit der id des
    /// Handels-Terminals — nicht nur `with_location_rows` isoliert.
    #[test]
    fn search_synthesizes_location_row_from_terminal_chain() {
        let chain = "Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station";
        let app = mock_app();
        let cache = Arc::new(Cache::open_in_memory().unwrap());
        cache
            .store_entities(
                EntityKind::Terminals,
                &[
                    provider::Entity {
                        id: 428,
                        kind: EntityKind::Terminals,
                        name: "Landing Services - Checkmate".into(),
                        code: None,
                        subtitle: chain.into(),
                    },
                    provider::Entity {
                        id: 436,
                        kind: EntityKind::Terminals,
                        name: "Commodity Shop - Admin - Checkmate".into(),
                        code: None,
                        subtitle: chain.into(),
                    },
                    provider::Entity {
                        id: 437,
                        kind: EntityKind::Terminals,
                        name: "Guns - Checkmate".into(),
                        code: None,
                        subtitle: chain.into(),
                    },
                ],
            )
            .unwrap();
        app.manage(TradeState {
            provider: Arc::new(StubProvider),
            cache,
            tracker: Arc::new(SyncTracker::default()),
            wiki_sync: None,
            write_gate: Arc::new(WriteGate::new()),
        });
        let state = app.state::<TradeState>();

        // `:l`-Filter → Location-Gruppe, Query trifft die Kette.
        let resp = search(
            state.clone(),
            "checkmate".into(),
            Some(vec!["location".to_string()]),
        )
        .unwrap();
        assert_eq!(resp.groups.len(), 1, "only the location group is requested");
        let loc_group = &resp.groups[0];
        assert_eq!(loc_group.kind, "location");

        // Kriterium 2: mindestens ein Treffer ist die Standort-Entity.
        let location_hits = loc_group
            .hits
            .iter()
            .filter(|h| h.kind == "location")
            .count();
        assert!(
            location_hits >= 1,
            "Kriterium 2 verletzt: keine kind='location' Zeile in der Location-Gruppe"
        );

        // Die Standort-Zeile referenziert das monetäre Terminal (436).
        let loc_row = loc_group
            .hits
            .iter()
            .find(|h| h.kind == "location")
            .expect("location row must be present");
        assert_eq!(
            loc_row.id, 436,
            "location row id must be the money terminal"
        );
        assert_eq!(
            loc_row.name, "Checkmate Station",
            "location name derives from the chain's last segment (N2)"
        );

        // Kriterium 1: die Terminal-Zeilen bleiben kind='terminal' mit echter id.
        let term_hits = loc_group
            .hits
            .iter()
            .filter(|h| h.kind == "terminal")
            .count();
        assert_eq!(term_hits, 3, "all 3 terminals stay kind='terminal'");
        assert!(
            loc_group
                .hits
                .iter()
                .any(|h| h.kind == "terminal" && h.id == 428)
        );
        assert!(
            loc_group
                .hits
                .iter()
                .any(|h| h.kind == "terminal" && h.id == 437)
        );

        let _ = app;
    }

    #[test]
    fn math_eval_inline_calculator() {
        // The UI strips the leading '=' before invoking; the command is
        // tolerant of it anyway.
        assert_eq!(math_eval("2*250+15%".into()).unwrap(), "500.15");
        assert_eq!(math_eval("=2*250+15%".into()).unwrap(), "500.15");
        assert_eq!(math_eval("(2+3)*4".into()).unwrap(), "20");
        assert_eq!(math_eval(" 10/4 ".into()).unwrap(), "2.5");
        // Malformed input → a German error string, never a panic.
        assert!(math_eval("2+3x".into()).is_err());
        assert!(math_eval("".into()).is_err());
        assert!(math_eval("1/0".into()).is_err()); // non-finite
    }

    #[test]
    fn listing_deep_link_builds_verified_url_scheme() {
        // URL scheme is the one verified against the live site (items AND
        // services live under /marketplace/item/info/{slug}/).
        assert_eq!(
            listing_deep_link("stark-compensator-2-UKCqAtSM3p"),
            "https://uexcorp.space/marketplace/item/info/stark-compensator-2-UKCqAtSM3p/"
        );
        // Whitespace-only / empty slug → marketplace home (no broken URL).
        assert_eq!(listing_deep_link(""), "https://uexcorp.space/marketplace/");
        assert_eq!(
            listing_deep_link("   "),
            "https://uexcorp.space/marketplace/"
        );
    }

    /// get_prices routes by `kind`: terminal → own per-terminal prices with a
    /// resolved display name, location → aggregate groups, item → item
    /// price points from `/items_prices`, default → commodity provider
    /// path. Verifies the backend contract the frontend mirrors in
    /// +page.svelte.
    #[tokio::test]
    async fn get_prices_routes_terminal_location_and_item_kinds() {
        use provider::{LocationPrices, TerminalPriceGroup};

        struct LocProvider;
        #[async_trait::async_trait]
        impl DataProvider for LocProvider {
            async fn search(
                &self,
                _e: provider::SearchEntities,
                _q: &str,
            ) -> provider::Result<Vec<SearchHit>> {
                Ok(vec![])
            }
            async fn prices(&self, _c: &str) -> provider::Result<Vec<provider::PricePoint>> {
                Ok(vec![provider::PricePoint {
                    commodity: "Agricium".into(),
                    commodity_id: 1,
                    terminal_id: 12,
                    location: "TDD Area 18".into(),
                    sell_price: 10_000.0,
                    buy_price: 0.0,
                    sell_stock: 301.0,
                    buy_stock: 0.0,
                    sell_status: 1,
                    buy_status: 0,
                    quality: 0,
                    updated: 1_792_341_485,
                }])
            }
            async fn location_prices(&self, _terminal_id: u64) -> provider::Result<LocationPrices> {
                Ok(LocationPrices {
                    location: "Pyro > Monox > Checkmate Station".into(),
                    terminal: "Commodity Shop - Admin - Checkmate".into(),
                    groups: vec![TerminalPriceGroup {
                        terminal_id: 436,
                        terminal_name: "Commodity Shop - Admin - Checkmate".into(),
                        prices: vec![provider::PricePoint {
                            commodity: "Agricultural Supplies".into(),
                            commodity_id: 3,
                            location: "Admin - Checkmate".into(),
                            terminal_id: 436,
                            sell_price: 0.0,
                            buy_price: 1108.0,
                            sell_stock: 0.0,
                            buy_stock: 0.0,
                            sell_status: 0,
                            buy_status: 7,
                            quality: 0,
                            updated: 1_792_355_842,
                        }],
                    }],
                })
            }
            async fn terminal_prices(
                &self,
                terminal_id: u64,
            ) -> provider::Result<Vec<provider::PricePoint>> {
                // Pro Terminal EIGENE Daten: die Menge unterscheidet sich je id.
                let commodity = if terminal_id == 429 {
                    "Medmonitor - Checkmate eigene Ware"
                } else {
                    "Landing Services eigene Ware"
                };
                Ok(vec![provider::PricePoint {
                    commodity: commodity.into(),
                    commodity_id: 99,
                    location: "Checkmate - eigener Stand".into(),
                    terminal_id,
                    sell_price: 0.0,
                    buy_price: 700.0,
                    sell_stock: 0.0,
                    buy_stock: 0.0,
                    sell_status: 0,
                    buy_status: 7,
                    quality: 0,
                    updated: 1_792_355_842,
                }])
            }
            async fn item_prices(
                &self,
                _item_id: u64,
            ) -> provider::Result<Vec<provider::PricePoint>> {
                Ok(vec![provider::PricePoint {
                    commodity: "CSP-68H Backpack".into(),
                    commodity_id: 465,
                    location: String::new(),
                    terminal_id: 107,
                    sell_price: 0.0,
                    buy_price: 295.0,
                    sell_stock: 0.0,
                    buy_stock: 0.0,
                    sell_status: 0,
                    buy_status: 1,
                    quality: 0,
                    updated: 1_792_355_842,
                }])
            }
            async fn marketplace_offers(
                &self,
                _f: provider::MarketplaceFilter,
            ) -> provider::Result<Vec<provider::Offer>> {
                Ok(vec![])
            }
            async fn fetch_entities(
                &self,
                _k: EntityKind,
            ) -> provider::Result<Vec<provider::Entity>> {
                Ok(vec![])
            }
            fn name(&self) -> String {
                "loc".into()
            }
        }

        let app = mock_app();
        let cache = Arc::new(Cache::open_in_memory().unwrap());
        // Seed one terminal so the `terminal`-Route den Anzeigenamen auflöst.
        cache
            .store_entities(
                EntityKind::Terminals,
                &[provider::Entity {
                    id: 429,
                    kind: EntityKind::Terminals,
                    name: "Landing Services - Checkmate".into(),
                    code: None,
                    subtitle: "Pyro > Monox > Checkmate Station".into(),
                }],
            )
            .unwrap();
        app.manage(TradeState {
            provider: Arc::new(LocProvider),
            cache: cache.clone(),
            tracker: Arc::new(SyncTracker::default()),
            wiki_sync: None,
            write_gate: Arc::new(WriteGate::new()),
        });
        let state = app.state::<TradeState>();

        // terminal route → prices of THIS terminal, no location aggregate.
        let d = get_prices(
            state.clone(),
            "Landing Services - Checkmate".into(),
            Some(IdArg::Num(429)),
            Some("terminal".into()),
        )
        .await
        .unwrap();
        assert!(
            d.location.is_none(),
            "terminal route must NOT fill `location`"
        );
        assert_eq!(d.commodity, "Landing Services - Checkmate");
        assert_eq!(d.prices.len(), 1);
        assert_eq!(d.prices[0].terminal_id, 429);
        assert_eq!(d.prices[0].buy_price, 700.0);
        assert_eq!(d.prices[0].commodity, "Medmonitor - Checkmate eigene Ware");

        // terminal route: andere id → andere (eigene) Daten.
        let d = get_prices(
            state.clone(),
            "Guns - Checkmate".into(),
            Some(IdArg::Num(437)),
            Some("terminal".into()),
        )
        .await
        .unwrap();
        assert_eq!(d.prices[0].terminal_id, 437);
        assert_eq!(d.prices[0].commodity, "Landing Services eigene Ware");

        // location route
        let d = get_prices(
            state.clone(),
            "Checkmate".into(),
            Some(IdArg::Num(436)),
            Some("location".into()),
        )
        .await
        .unwrap();
        let loc = d.location.expect("location route must fill `location`");
        assert_eq!(loc.groups.len(), 1);
        assert_eq!(loc.groups[0].prices[0].buy_price, 1108.0);
        assert_eq!(d.prices.len(), 1, "flattened prices mirror the groups");

        // item route → item prices (not empty), no location
        let d = get_prices(
            state.clone(),
            "CSP-68H Backpack".into(),
            Some(IdArg::Num(465)),
            Some("item".into()),
        )
        .await
        .unwrap();
        assert_eq!(d.prices.len(), 1, "item route must now fetch item prices");
        assert_eq!(d.prices[0].commodity_id, 465);
        assert_eq!(d.prices[0].buy_price, 295.0);
        assert!(d.location.is_none());

        // default route → commodity provider path
        let d = get_prices(state.clone(), "agricium".into(), None, None)
            .await
            .unwrap();
        assert_eq!(d.prices.len(), 1);
        assert_eq!(d.prices[0].sell_price, 10_000.0);
        assert!(d.location.is_none());

        let _ = app;
    }

    /// get_marketplace: no snapshot → provider error surfaces as a command
    /// error (UI shows the error pane); with snapshot → offline fallback.
    #[tokio::test]
    async fn get_marketplace_falls_back_to_snapshot_offline() {
        use provider::{ListingType, MarketOperation};

        struct FailingProvider;
        #[async_trait::async_trait]
        impl DataProvider for FailingProvider {
            async fn search(
                &self,
                _e: provider::SearchEntities,
                _q: &str,
            ) -> provider::Result<Vec<SearchHit>> {
                Ok(vec![])
            }
            async fn prices(&self, _c: &str) -> provider::Result<Vec<provider::PricePoint>> {
                Ok(vec![])
            }
            async fn marketplace_offers(
                &self,
                _f: provider::MarketplaceFilter,
            ) -> provider::Result<Vec<provider::Offer>> {
                Err(provider::ProviderError::Timeout)
            }
            async fn fetch_entities(
                &self,
                _k: EntityKind,
            ) -> provider::Result<Vec<provider::Entity>> {
                Ok(vec![])
            }
            fn name(&self) -> String {
                "failing".into()
            }
        }

        let app = mock_app();
        app.manage(TradeState {
            provider: Arc::new(FailingProvider),
            cache: Arc::new(Cache::open_in_memory().unwrap()),
            tracker: Arc::new(SyncTracker::default()),
            wiki_sync: None,
            write_gate: Arc::new(WriteGate::new()),
        });
        let state = app.state::<TradeState>();

        // no snapshot → error surfaces
        let err = get_marketplace(state.clone(), provider::MarketplaceFilter::default())
            .await
            .unwrap_err();
        assert!(err.contains("Zeitüberschreitung"), "got {err}");

        // with a snapshot → offline fallback with note
        state
            .cache
            .store_marketplace(&[provider::Offer {
                id: 1,
                title: "x".into(),
                description: String::new(),
                item_id: 0,
                category_id: 0,
                operation: MarketOperation::Sell,
                listing_type: ListingType::Item,
                price: 1.0,
                currency: "UEC".into(),
                unit: "unit".into(),
                quantity: 1,
                location: "Levski".into(),
                region: "Stanton".into(),
                seller: "s".into(),
                quality: 0,
                updated: 1,
                expires: None,
                slug: "s".into(),
                photos: Vec::new(),
                approved: true,
            }])
            .unwrap();
        let resp = get_marketplace(state.clone(), provider::MarketplaceFilter::default())
            .await
            .unwrap();
        assert_eq!(resp.source, "offline");
        assert_eq!(resp.offers.len(), 1);
        assert!(resp.note.is_some(), "offline fallback must carry the note");
        let _ = app;
    }

    struct StubProvider;

    #[async_trait::async_trait]
    impl DataProvider for StubProvider {
        async fn search(
            &self,
            _e: provider::SearchEntities,
            _q: &str,
        ) -> provider::Result<Vec<SearchHit>> {
            Ok(vec![])
        }
        async fn prices(&self, _c: &str) -> provider::Result<Vec<provider::PricePoint>> {
            Ok(vec![])
        }
        async fn item_prices(&self, _item_id: u64) -> provider::Result<Vec<provider::PricePoint>> {
            Ok(vec![])
        }
        async fn marketplace_offers(
            &self,
            _f: provider::MarketplaceFilter,
        ) -> provider::Result<Vec<provider::Offer>> {
            Ok(vec![])
        }
        async fn fetch_entities(&self, _k: EntityKind) -> provider::Result<Vec<provider::Entity>> {
            Ok(vec![])
        }
        fn name(&self) -> String {
            "stub".into()
        }
    }

    /// Standort-Synthese (Kriterium 2): pro Kette erscheint eine Zeile mit
    /// `kind='location'` und der id des Handels-Terminals (436 = Admin), die
    /// Terminal-Zeilen bleiben `kind='terminal'`.
    #[test]
    fn location_synthesis_emits_location_hit_with_money_terminal() {
        let cache = Arc::new(Cache::open_in_memory().unwrap());
        let chain = "Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station";
        cache
            .store_entities(
                EntityKind::Terminals,
                &[
                    provider::Entity {
                        id: 428,
                        kind: EntityKind::Terminals,
                        name: "Landing Services - Checkmate".into(),
                        code: None,
                        subtitle: chain.into(),
                    },
                    provider::Entity {
                        id: 436,
                        kind: EntityKind::Terminals,
                        name: "Commodity Shop - Admin - Checkmate".into(),
                        code: None,
                        subtitle: chain.into(),
                    },
                    provider::Entity {
                        id: 437,
                        kind: EntityKind::Terminals,
                        name: "Guns - Checkmate".into(),
                        code: None,
                        subtitle: chain.into(),
                    },
                ],
            )
            .unwrap();

        // Nur die Terminal-Hits, die ein (limitierter) FTS-Treffer liefern würde.
        let hits = vec![
            provider::SearchHit {
                id: 437,
                name: "Guns - Checkmate".into(),
                kind: "terminal".into(),
                subtitle: chain.into(),
                code: None,
            },
            provider::SearchHit {
                id: 428,
                name: "Landing Services - Checkmate".into(),
                kind: "terminal".into(),
                subtitle: chain.into(),
                code: None,
            },
        ];
        let out = with_location_rows(&cache, hits).unwrap();

        // Location-Zeile zuerst, id des Handels-Terminals (436) — auch wenn 436
        // nicht selbst unter den FTS-Treffern war (Kette aus dem Cache erschlossen).
        assert_eq!(out[0].kind, "location");
        assert_eq!(out[0].id, 436, "location row id must be the money terminal");
        assert_eq!(
            out[0].name, "Checkmate Station",
            "name is the station aggregate, not the terminal (N2)"
        );
        assert_eq!(out[0].subtitle, chain);
        // code None (kein UEX-Terminal-Code, sondern eine Standort-Entity).
        assert_eq!(out[0].code, None);
        // Terminal-Zeilen unverändert, danach.
        assert_eq!(out[1..].iter().filter(|h| h.kind == "terminal").count(), 2);
        assert!(out[1..].iter().any(|h| h.kind == "terminal" && h.id == 428));
        assert!(out[1..].iter().any(|h| h.kind == "terminal" && h.id == 437));
    }

    #[test]
    fn location_synthesis_empty_hits_is_noop() {
        let cache = Arc::new(Cache::open_in_memory().unwrap());
        assert!(with_location_rows(&cache, vec![]).unwrap().is_empty());
    }

    /// P1.2 (Feedback-Runde 4): ein Wiki-Location-Treffer listet die Shops
    /// seiner Station als eigene Terminal-Zeilen — auch Terminals, die NICHT
    /// selbst im FTS-Treffer waren. Die synthetische Ketten-Location wird
    /// NICHT doppelt erzeugt, wenn die Wiki-Location den Namen schon trägt.
    #[test]
    fn wiki_location_hit_lists_station_shops_and_dedupes_synthesis() {
        let cache = Arc::new(Cache::open_in_memory().unwrap());
        let chain = "Stanton > Nyx > Levski";
        cache
            .store_entities(
                EntityKind::Locations,
                &[
                    provider::Entity {
                        id: 900,
                        kind: EntityKind::Locations,
                        name: "Levski".into(),
                        code: None,
                        subtitle: "Nyx > Levski".into(),
                    },
                    // FALSCH-Positiv-Kandidat: Subtitle enthält Levski,
                    // Name aber nicht (Name-only-Regel — hier nur als
                    // Nicht-Treffer-Demo, der FTS-Treffer wird simuliert).
                    provider::Entity {
                        id: 901,
                        kind: EntityKind::Locations,
                        name: "Mercy Hospital".into(),
                        code: None,
                        subtitle: "Nyx > Levski > Mercy Hospital".into(),
                    },
                ],
            )
            .unwrap();
        cache
            .store_entities(
                EntityKind::Terminals,
                &[
                    provider::Entity {
                        id: 777,
                        kind: EntityKind::Terminals,
                        name: "Pharmacy - Levski".into(),
                        code: None,
                        subtitle: chain.into(),
                    },
                    provider::Entity {
                        id: 778,
                        kind: EntityKind::Terminals,
                        name: "Commodity Shop - Levski".into(),
                        code: None,
                        subtitle: chain.into(),
                    },
                    // Anderer Ort — darf NICHT als Levski-Shop auftauchen.
                    provider::Entity {
                        id: 500,
                        kind: EntityKind::Terminals,
                        name: "Commodity Shop - Admin - Checkmate".into(),
                        code: None,
                        subtitle: "Stanton > Crusader > Checkmate Station".into(),
                    },
                ],
            )
            .unwrap();

        // FTS-Treffer-Simulation: Wiki-Location „Levski" (Name-only-Regel),
        // kein Mercy, kein Terminal selbst getroffen.
        let hits = vec![provider::SearchHit {
            id: 900,
            name: "Levski".into(),
            kind: "location".into(),
            subtitle: "Nyx > Levski".into(),
            code: None,
        }];
        let out = with_location_rows(&cache, hits).unwrap();

        // Location-Zeile bleibt; ihre Shops (777, 778) kommen als
        // Terminal-Zeilen dazu; Checkmate (500) NICHT.
        assert_eq!(out[0].kind, "location");
        assert_eq!(out[0].id, 900);
        let terminal_ids: Vec<u64> = out[1..]
            .iter()
            .filter(|h| h.kind == "terminal")
            .map(|h| h.id)
            .collect();
        assert_eq!(
            terminal_ids,
            vec![777, 778],
            "both station shops must be listed: {terminal_ids:?}"
        );
        assert!(
            !out.iter().any(|h| h.name == "Mercy Hospital"),
            "subtitle-only location match must not leak back in"
        );
        // Keine doppelte synthetische „Levski"-Location.
        assert_eq!(
            out.iter().filter(|h| h.kind == "location").count(),
            1,
            "synthesis must not duplicate the wiki location"
        );
    }

    /// P1.2: Terminal-Treffer OHNE Wiki-Location-Treffer behalten die alte
    /// Synthese (Kette → eine location-Zeile) — Regressions-Anker.
    #[test]
    fn terminal_only_hits_still_synththesize_location_row() {
        let cache = Arc::new(Cache::open_in_memory().unwrap());
        let chain = "Stanton > Crusader > Checkmate Station";
        cache
            .store_entities(
                EntityKind::Terminals,
                &[provider::Entity {
                    id: 436,
                    kind: EntityKind::Terminals,
                    name: "Commodity Shop - Admin - Checkmate".into(),
                    code: None,
                    subtitle: chain.into(),
                }],
            )
            .unwrap();
        let hits = vec![provider::SearchHit {
            id: 436,
            name: "Commodity Shop - Admin - Checkmate".into(),
            kind: "terminal".into(),
            subtitle: chain.into(),
            code: None,
        }];
        let out = with_location_rows(&cache, hits).unwrap();
        assert_eq!(out[0].kind, "location");
        assert_eq!(out[0].id, 436, "money terminal of the chain");
        assert_eq!(out[0].name, "Checkmate Station");
        assert_eq!(out[1].kind, "terminal");
        assert_eq!(out[1].id, 436);
    }

    // ---- M2a (Read-Phase): „Mein Shop" — eigene Offers + Trades --------

    struct ShopProvider {
        fail_offers: bool,
        fail_trades: bool,
    }

    #[async_trait::async_trait]
    impl DataProvider for ShopProvider {
        async fn search(
            &self,
            _e: provider::SearchEntities,
            _q: &str,
        ) -> provider::Result<Vec<SearchHit>> {
            Ok(vec![])
        }
        async fn prices(&self, _c: &str) -> provider::Result<Vec<provider::PricePoint>> {
            Ok(vec![])
        }
        async fn marketplace_offers(
            &self,
            _f: provider::MarketplaceFilter,
        ) -> provider::Result<Vec<provider::Offer>> {
            Ok(vec![])
        }
        async fn my_offers(&self, _u: String) -> provider::Result<Vec<provider::Offer>> {
            if self.fail_offers {
                Err(provider::ProviderError::Http {
                    status: 403,
                    body: "invalid_secret_key".into(),
                })
            } else {
                Ok(vec![provider::Offer {
                    id: 900,
                    title: "Drake Nettle S".into(),
                    description: String::new(),
                    item_id: 0,
                    category_id: 0,
                    operation: MarketOperation::Sell,
                    listing_type: ListingType::Item,
                    price: 4500000.0,
                    currency: "UEC".into(),
                    unit: "unit".into(),
                    quantity: 1,
                    location: "Port Tressler".into(),
                    region: "Stanton".into(),
                    seller: "sebo_one".into(),
                    quality: 72,
                    updated: 1730000000,
                    expires: None,
                    slug: "drake-nettle-s".into(),
                    photos: Vec::new(),
                    approved: true,
                }])
            }
        }
        async fn user_trades(&self) -> provider::Result<Vec<provider::Trade>> {
            if self.fail_trades {
                Err(provider::ProviderError::Http {
                    status: 403,
                    body: "not_allowed".into(),
                })
            } else {
                Ok(vec![provider::Trade {
                    id: 5,
                    terminal_id: 1,
                    commodity_id: 79,
                    operation: MarketOperation::Sell,
                    scu: 250,
                    price: 10000.0,
                    date_added: 1730000100,
                    commodity: "Waste".into(),
                    terminal: "Port Tressler".into(),
                    vehicle: None,
                    fleet_name: None,
                    organization: None,
                }])
            }
        }
        async fn fetch_entities(&self, _k: EntityKind) -> provider::Result<Vec<provider::Entity>> {
            Ok(vec![])
        }
        fn name(&self) -> String {
            "shop".into()
        }
    }

    #[tokio::test]
    async fn my_offers_live_and_offline_fallback() {
        // Offline-Szenario: Live schlägt fehl, aber ein Snapshot existiert →
        // source "offline" + Note, der Snapshot bleibt sichtbar.
        let app = mock_app();
        app.manage(TradeState {
            provider: Arc::new(ShopProvider {
                fail_offers: true,
                fail_trades: true,
            }),
            cache: Arc::new(Cache::open_in_memory().unwrap()),
            tracker: Arc::new(SyncTracker::default()),
            wiki_sync: None,
            write_gate: Arc::new(WriteGate::new()),
        });
        let state = app.state::<TradeState>();
        state
            .cache
            .store_own_offers(
                &[provider::Offer {
                    id: 900,
                    title: "Drake Nettle S".into(),
                    description: String::new(),
                    item_id: 0,
                    category_id: 0,
                    operation: MarketOperation::Sell,
                    listing_type: ListingType::Item,
                    price: 4500000.0,
                    currency: "UEC".into(),
                    unit: "unit".into(),
                    quantity: 1,
                    location: "Port Tressler".into(),
                    region: "Stanton".into(),
                    seller: "sebo_one".into(),
                    quality: 72,
                    updated: 1730000000,
                    expires: None,
                    slug: "drake-nettle-s".into(),
                    photos: Vec::new(),
                    approved: true,
                }],
                "sebo_one",
            )
            .unwrap();
        let resp = get_my_offers(state.clone(), "sebo_one".into())
            .await
            .unwrap();
        assert_eq!(resp.source, "offline");
        assert_eq!(resp.offers.len(), 1, "snapshot survives");
        assert_eq!(resp.offers[0].title, "Drake Nettle S");
        assert!(resp.note.is_some());
        let _ = app;
    }

    #[tokio::test]
    async fn my_offers_offline_fallback_is_username_aware() {
        // Defekt 3 (R2-Probe): Snapshot gehört zu sebo_one, Live-Fetch für
        // automator schlägt fehl → die automator-View darf die sebo_one-
        // Offers NICHT als „offline"-Eigendaten bekommen, sondern muss den
        // Fehlerpfad zeigen (wie der leere Fall). Kontowechsel-Leak.
        let app = mock_app();
        app.manage(TradeState {
            provider: Arc::new(ShopProvider {
                fail_offers: true,
                fail_trades: true,
            }),
            cache: Arc::new(Cache::open_in_memory().unwrap()),
            tracker: Arc::new(SyncTracker::default()),
            wiki_sync: None,
            write_gate: Arc::new(WriteGate::new()),
        });
        let state = app.state::<TradeState>();
        state
            .cache
            .store_own_offers(
                &[provider::Offer {
                    id: 900,
                    title: "Drake Nettle S".into(),
                    description: String::new(),
                    item_id: 0,
                    category_id: 0,
                    operation: MarketOperation::Sell,
                    listing_type: ListingType::Item,
                    price: 4500000.0,
                    currency: "UEC".into(),
                    unit: "unit".into(),
                    quantity: 1,
                    location: "Port Tressler".into(),
                    region: "Stanton".into(),
                    seller: "sebo_one".into(),
                    quality: 72,
                    updated: 1730000000,
                    expires: None,
                    slug: "drake-nettle-s".into(),
                    photos: Vec::new(),
                    approved: true,
                }],
                "sebo_one",
            )
            .unwrap();
        // Gleicher User → offline-Fallback serviert den Snapshot.
        let ok = get_my_offers(state.clone(), "sebo_one".into())
            .await
            .unwrap();
        assert_eq!(ok.source, "offline");
        assert_eq!(ok.offers.len(), 1);
        // Anderer User mit fehlschlagendem Fetch → Err, NIE sebo_one-Offers.
        let err = get_my_offers(state.clone(), "automator".into()).await;
        assert!(
            err.is_err(),
            "fremder Snapshot darf nicht als offline serviert werden"
        );
        let _ = app;
    }

    #[tokio::test]
    async fn my_offers_live_sets_snapshot() {
        // Live-Pfad: der erfolgreiche Fetch füllt den eigenen Snapshot (der
        // spätere Offline-Fallback kann ihn servieren).
        let app = mock_app();
        app.manage(TradeState {
            provider: Arc::new(ShopProvider {
                fail_offers: false,
                fail_trades: true,
            }),
            cache: Arc::new(Cache::open_in_memory().unwrap()),
            tracker: Arc::new(SyncTracker::default()),
            wiki_sync: None,
            write_gate: Arc::new(WriteGate::new()),
        });
        let state = app.state::<TradeState>();
        let resp = get_my_offers(state.clone(), "sebo_one".into())
            .await
            .unwrap();
        assert_eq!(resp.source, "live");
        assert_eq!(resp.offers[0].title, "Drake Nettle S");
        // Hinweis: den Snapshot füllt der echte uex_client-Provider (get_own_-
        // offers_impl) — der IPC-Command delegiert nur. Das ist im uex_client-/
        // cache-Test abgedeckt, nicht an der IPC-Stub-Grenze.
        let _ = app;
    }

    #[tokio::test]
    async fn my_trades_fails_cleanly_without_snapshot_and_serves_offline() {
        let app = mock_app();
        app.manage(TradeState {
            provider: Arc::new(ShopProvider {
                fail_offers: true,
                fail_trades: true,
            }),
            cache: Arc::new(Cache::open_in_memory().unwrap()),
            tracker: Arc::new(SyncTracker::default()),
            wiki_sync: None,
            write_gate: Arc::new(WriteGate::new()),
        });
        let state = app.state::<TradeState>();
        // Kein Snapshot → sauberer Fehler (nicht leere Liste / kein Crash).
        let err = get_my_trades(state.clone()).await.unwrap_err();
        assert!(!err.is_empty(), "clean German error");
        // Snapshot gesetzt → offline-Fallback.
        state
            .cache
            .store_own_trades(&[provider::Trade {
                id: 7,
                terminal_id: 2,
                commodity_id: 79,
                operation: MarketOperation::Buy,
                scu: 100,
                price: 500.0,
                date_added: 1730000200,
                commodity: "Waste".into(),
                terminal: "Lorville".into(),
                vehicle: None,
                fleet_name: None,
                organization: None,
            }])
            .unwrap();
        let resp = get_my_trades(state).await.unwrap();
        assert_eq!(resp.source, "offline");
        assert_eq!(resp.trades[0].commodity, "Waste");
        let _ = app;
    }

    // ---- M2b (Write-Layer): Rate-Limit-Gate + Advertise/Delete-IPC --------

    #[test]
    fn write_gate_limits_to_max_per_window() {
        // Rate-Limit-Sicherheit (Anforderung M2b-5): max ~2 Writes/min.
        let gate = WriteGate::with_window(2, 60);
        let now = 1_000_000u64;
        // Zwei Writes passieren, das Fenster ist nach 2 voll.
        assert!(gate.try_acquire(now).is_ok());
        gate.record_write(now);
        assert!(gate.try_acquire(now + 1).is_ok());
        gate.record_write(now + 1);
        // Dritter Write im selben Fenster → abgelehnt, mit Wartezeit-Meldung.
        let err = gate.try_acquire(now + 5).unwrap_err();
        assert!(
            err.contains("Rate-Limit") && err.contains("warten"),
            "{err}"
        );
        assert!(err.contains("Schreibaktionen"), "{err}");
        // Nach Ablauf des Fensters (ältester Eintrag + window) wieder frei.
        assert!(gate.try_acquire(now + 61).is_ok());
    }

    #[test]
    fn write_gate_failed_writes_do_not_consume_slots() {
        // Fehler dürfen das Fenster NICHT belegen — sonst würde ein 429 den
        // Client für das ganze Fenster sperren (Retry-Spam erzwingen). Nur
        // erfolgreiche Writes zählen.
        let gate = WriteGate::with_window(2, 60);
        let now = 1_000_000u64;
        // Ein Versuch ohne record_write (fehlgeschlagen) erscheint nicht.
        assert!(gate.try_acquire(now).is_ok());
        // (kein record_write → der Versuch ist weg)
        assert!(gate.try_acquire(now + 1).is_ok());
        gate.record_write(now + 1);
        assert!(
            gate.try_acquire(now + 2).is_ok(),
            "nur 1 registriert → frei"
        );
    }

    #[test]
    fn write_gate_message_lists_wait_seconds() {
        let gate = WriteGate::with_window(1, 60);
        let now = 1_000_000u64;
        gate.try_acquire(now).unwrap();
        gate.record_write(now);
        let err = gate.try_acquire(now + 5).unwrap_err();
        // nächster freier Slot = ältester (now) + window (60) - now(5) = 55s.
        assert!(err.contains("55 s"), "{err}");
    }

    /// Write-Provider-Stub: advertise liefert ein festes Offer, delete Ok —
    /// prüft die IPC-Schicht (Gate vor dem Provider-Aufruf).
    struct WriteProvider {
        advertise_calls: std::sync::atomic::AtomicU32,
    }

    #[async_trait::async_trait]
    impl DataProvider for WriteProvider {
        async fn search(
            &self,
            _e: provider::SearchEntities,
            _q: &str,
        ) -> provider::Result<Vec<SearchHit>> {
            Ok(vec![])
        }
        async fn prices(&self, _c: &str) -> provider::Result<Vec<provider::PricePoint>> {
            Ok(vec![])
        }
        async fn marketplace_offers(
            &self,
            _f: provider::MarketplaceFilter,
        ) -> provider::Result<Vec<provider::Offer>> {
            Ok(vec![])
        }
        async fn fetch_entities(&self, _k: EntityKind) -> provider::Result<Vec<provider::Entity>> {
            Ok(vec![])
        }
        fn name(&self) -> String {
            "write".into()
        }
        async fn advertise_offer(
            &self,
            _p: provider::AdvertisePayload,
        ) -> provider::Result<provider::AdvertiseResult> {
            let _ = self
                .advertise_calls
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(provider::AdvertiseResult {
                id: 9001,
                url: "https://uexcorp.space/marketplace/item/info/neu-angelegt".into(),
            })
        }
        async fn delete_offer(&self, _id: u64) -> provider::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn marketplace_advertise_command_gate_blocks_and_permits() {
        // IPC-Anlegen geht durch's WriteGate: 2 OK, 3. blockiert.
        let app = mock_app();
        app.manage(TradeState {
            provider: Arc::new(WriteProvider {
                advertise_calls: Default::default(),
            }),
            cache: Arc::new(Cache::open_in_memory().unwrap()),
            tracker: Arc::new(SyncTracker::default()),
            wiki_sync: None,
            write_gate: Arc::new(WriteGate::new()),
        });
        let state = app.state::<TradeState>();
        let payload = provider::AdvertisePayload {
            id_item: None,
            id_category: None,
            id_star_system: None,
            id_terminal: None,
            id_organization: None,
            operation: "sell".into(),
            language: "en_US".into(),
            r#type: "item".into(),
            unit: "unit".into(),
            price: 120_000,
            currency: "UEC".into(),
            location: Some("Port Tressler".into()),
            title: "DRAKE NETTLE S | Used".into(),
            description: "Minor scuffs on left wing. Fully functional.".into(),
            source: Some("purchased_in_game".into()),
            in_stock: 1,
            availability: Some("ready_pickup".into()),
            durability: Some(85),
            hours_expiration: Some(168),
            is_hidden: None,
            quality: None,
            image_data: None,
            is_production: 1,
        };
        // 2 erfolgreiche Anlegen erlaubt.
        assert!(
            marketplace_advertise(state.clone(), payload.clone())
                .await
                .is_ok()
        );
        assert!(
            marketplace_advertise(state.clone(), payload.clone())
                .await
                .is_ok()
        );
        // 3. → WriteGate greift (das echte Gate hat now=now_unix() als ein- und
        // dieselbe Sekunde → beide Writes im selben 60s-Fenster).
        let third = marketplace_advertise(state.clone(), payload).await;
        assert!(
            third.is_err(),
            "rate-limit-Gate muss den 3. Write blockieren"
        );
        let msg = third.unwrap_err();
        assert!(msg.contains("Rate-Limit"), "{msg}");
        let _ = app;
    }

    #[tokio::test]
    async fn marketplace_delete_command_succeeds_through_gate() {
        let app = mock_app();
        app.manage(TradeState {
            provider: Arc::new(WriteProvider {
                advertise_calls: Default::default(),
            }),
            cache: Arc::new(Cache::open_in_memory().unwrap()),
            tracker: Arc::new(SyncTracker::default()),
            wiki_sync: None,
            write_gate: Arc::new(WriteGate::new()),
        });
        let state = app.state::<TradeState>();
        // DELETE erfolgreich (provider Ok, gate frei).
        assert!(marketplace_delete(state.clone(), 847_291).await.is_ok());
        let _ = app;
    }

    /// item_suggest (MC, t_5faaf946): Typeahead über EntityKind::Items —
    /// nur Items, Ranking starts-with vor contains, max 12, leerer Query leer.
    #[test]
    fn item_suggest_lists_only_items_mc() {
        let app = mock_app();
        let cache = Arc::new(Cache::open_in_memory().unwrap());
        cache
            .store_entities(
                EntityKind::Items,
                &[
                    provider::Entity {
                        id: 2265,
                        kind: EntityKind::Items,
                        name: "Fieldsbury Dark Bear Helmet Guava".into(),
                        code: None,
                        subtitle: "Helmets · Kastak Arms".into(),
                    },
                    provider::Entity {
                        id: 2266,
                        kind: EntityKind::Items,
                        name: "CSP-68H Backpack".into(),
                        code: None,
                        subtitle: "Backpacks · RSI".into(),
                    },
                    provider::Entity {
                        id: 2267,
                        kind: EntityKind::Items,
                        name: "P-6SR Sniper Rifle".into(),
                        code: None,
                        subtitle: "Weapons · Behring".into(),
                    },
                ],
            )
            .unwrap();
        // Terminal/Commodity-Rows dürfen NIEMALS auftauchen (nur Items).
        cache
            .store_entities(
                EntityKind::Terminals,
                &[provider::Entity {
                    id: 436,
                    kind: EntityKind::Terminals,
                    name: "Commodity Shop - Admin - Checkmate".into(),
                    code: None,
                    subtitle: "ArcCorp > Checkmate".into(),
                }],
            )
            .unwrap();
        app.manage(TradeState {
            provider: Arc::new(StubProvider),
            cache,
            tracker: Arc::new(SyncTracker::default()),
            wiki_sync: None,
            write_gate: Arc::new(WriteGate::new()),
        });
        let state = app.state::<TradeState>();

        // starts-with
        let r = item_suggest(state.clone(), "fieldsbury".to_string()).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id, 2265);
        assert_eq!(r[0].name, "Fieldsbury Dark Bear Helmet Guava");
        assert!(r[0].subtitle.contains("Kastak Arms"), "{}", r[0].subtitle);

        // contains
        let r = item_suggest(state.clone(), "rifle".to_string()).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id, 2267);

        // Case-insensitiv + leere Query → leer.
        let r = item_suggest(state.clone(), "FIELDSBURY".to_string()).unwrap();
        assert_eq!(r.len(), 1);
        assert!(
            item_suggest(state.clone(), "".to_string())
                .unwrap()
                .is_empty()
        );

        // Terminal taucht für einen item-Query nicht auf.
        let r = item_suggest(state.clone(), "checkmate".to_string()).unwrap();
        assert!(r.is_empty(), "Terminals gehören nicht ins Item-Typeahead");
        let _ = app;
    }
}

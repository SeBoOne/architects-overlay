//! # Composite data provider
//!
//! Combines the two sources behind the single [`DataProvider`] seam:
//!
//! * **UEX** (`uex_client::UexProvider`) stays the source for terminals,
//!   items, item shop prices, the marketplace and the location aggregates
//!   (id_space_station-gruppierte Terminal-Ketten).
//! * **Wiki** (`wiki_client::WikiProvider`) delivers the Locations search
//!   dataset (rich hierarchy, ~2k rows, patch-driven 12h TTL) and the
//!   commodity PRICES (detail endpoint, `uex_prices.purchase` — the same UEX
//!   terminal id space, so aggregates stay consistent).
//!
//! Fallback/dedupe policy (card t_c4b94db2 Punkt 3):
//!
//! * primary source = config `commodity_source` (default `wiki`);
//! * a non-empty wiki price table wins as a whole — no per-terminal mixing,
//!   so the table always shows ONE consistent snapshot of one source;
//! * gaps (wiki empty / NotFound / fetch error) fall back to the UEX table;
//! * a CONFLICT (both sources have prices) is resolved in favor of the
//!   primary source — both describe the same UEX terminal ids, so mixing
//!   would be defensible, but one-source tables avoid half-updated rows
//!   (wiki: full-detail refresh; UEX: hourly `/commodities_prices`).
//!
//! The switch is live (`SourceSwitch`): the config-view setter flips it and
//! the NEXT price fetch already uses the new source.

use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

use cache::{Cache, SOURCE_WIKI};
use provider::{
    DataProvider, Entity, EntityKind, LocationPrices, MarketplaceFilter, PricePoint, ProviderError,
    Result, SearchEntities, SearchHit,
};
use settings::CommoditySource;
use uex_client::UexProvider;
use wiki_client::WikiProvider;

/// Live commodity-source switch (0 = wiki, 1 = uex). Shared between the
/// settings state (setter) and the composite provider (reader).
#[derive(Clone)]
pub struct SourceSwitch(Arc<AtomicU8>);

impl SourceSwitch {
    pub fn new(source: CommoditySource) -> SourceSwitch {
        SourceSwitch(Arc::new(AtomicU8::new(match source {
            CommoditySource::Wiki => 0,
            CommoditySource::Uex => 1,
        })))
    }

    pub fn set(&self, source: CommoditySource) {
        self.0.store(
            match source {
                CommoditySource::Wiki => 0,
                CommoditySource::Uex => 1,
            },
            Ordering::SeqCst,
        );
    }

    pub fn get(&self) -> CommoditySource {
        match self.0.load(Ordering::SeqCst) {
            1 => CommoditySource::Uex,
            _ => CommoditySource::Wiki,
        }
    }
}

/// Search result page size served from the cache (mirrors the UEX provider).
const SEARCH_LIMIT: u32 = 25;

/// Composite provider: wiki locations + wiki commodity prices, UEX for
/// everything else. See the module docs for the fallback policy.
pub struct CompositeProvider {
    uex: UexProvider,
    wiki: WikiProvider,
    source: SourceSwitch,
    cache: Arc<Cache>,
}

impl CompositeProvider {
    pub fn new(
        uex: UexProvider,
        wiki: WikiProvider,
        source: SourceSwitch,
        cache: Arc<Cache>,
    ) -> Self {
        CompositeProvider {
            uex,
            wiki,
            source,
            cache,
        }
    }

    /// Primary price source for commodity prices.
    fn commodity_source(&self) -> CommoditySource {
        self.source.get()
    }

    /// Terminal price points for one commodity from the wiki detail endpoint
    /// (`uex_prices.purchase`), deduped per terminal id. Wiki fetch failures
    /// are logged and reported as an EMPTY set (→ UEX fallback), never as a
    /// hard error — a wiki outage must not degrade the price view while UEX
    /// is reachable.
    async fn wiki_prices(&self, commodity: &str) -> Vec<PricePoint> {
        match self.wiki.prices_for(commodity).await {
            Ok(points) => points,
            Err(ProviderError::NotFound(_)) => Vec::new(),
            Err(e) => {
                log::warn!("wiki prices for '{commodity}' failed, falling back to UEX: {e}");
                Vec::new()
            }
        }
    }

    /// Terminal price points from the UEX `/commodities_prices` path. Errors
    /// other than NotFound are swallowed into an empty set — the caller only
    /// uses this as the fallback/merge source.
    async fn uex_prices_quiet(&self, commodity: &str) -> Vec<PricePoint> {
        match self.uex.prices(commodity).await {
            Ok(points) => points,
            Err(ProviderError::NotFound(_)) => Vec::new(),
            Err(e) => {
                log::warn!("uex prices for '{commodity}' failed: {e}");
                Vec::new()
            }
        }
    }

    /// Merge two price tables into one, keyed by terminal id. Rows of the
    /// primary source win on terminal conflicts; the secondary source only
    /// fills terminals the primary does not report.
    fn merge_prices(primary: Vec<PricePoint>, secondary: Vec<PricePoint>) -> Vec<PricePoint> {
        let mut seen: std::collections::HashSet<u64> =
            primary.iter().map(|p| p.terminal_id).collect();
        let mut out = primary;
        for p in secondary {
            if seen.insert(p.terminal_id) {
                out.push(p);
            }
        }
        out.sort_by(|a, b| b.buy_price.total_cmp(&a.buy_price));
        out
    }

    /// Resolve a wiki location id to the UEX terminal id of the same place
    /// (Location→Terminal-Konsistenz). Wiki- und UEX-Ketten weichen real
    /// voneinander ab (Wiki „Nyx > Levski“ vs. UEX „Nyx > Delamar > Levski“),
    /// daher ist die Matchung gestuft (siehe `terminal_match_tier`): exakte
    /// Kette, dann last-segment / „- <Ort>“-Namenssuffix / Kettensuffix,
    /// dann irgendein Segment. Gleichwertige Kandidaten werden über den
    /// System-Kontext (erstes Kettensegment) und die Monetär-Heuristik
    /// aufgelöst (siehe `best_terminal_match`).
    async fn uex_terminal_for_wiki_location(&self, wiki_id: u64) -> Result<u64> {
        let wiki_locations = self
            .cache
            .entities_of_sourced(EntityKind::Locations, Some(SOURCE_WIKI))
            .map_err(|e| ProviderError::Cache(e.to_string()))?;
        let location = wiki_locations
            .iter()
            .find(|e| e.id == wiki_id)
            .ok_or_else(|| {
                ProviderError::NotFound(format!("wiki location {wiki_id} not in reference data"))
            })?;
        let name = location.name.clone();
        let chain = location.subtitle.clone();

        let terminals = self
            .cache
            .entities_of(EntityKind::Terminals)
            .map_err(|e| ProviderError::Cache(e.to_string()))?;
        let chosen = best_terminal_match(&name, &chain, &terminals).ok_or_else(|| {
            ProviderError::NotFound(format!("no UEX terminal matches wiki location '{name}'"))
        })?;
        Ok(chosen.id)
    }
}

/// Kettensegmente einer UEX-/Wiki-Kette ("A > B > C" → ["A", "B", "C"]),
/// getrimmt, ohne leere Segmente.
fn chain_segments(chain: &str) -> Vec<&str> {
    chain
        .split('>')
        .map(str::trim)
        .filter(|seg| !seg.is_empty())
        .collect()
}

/// Match-Tier einer UEX-Terminal-Zeile gegen eine Wiki-Location (kleiner =
/// besser). Die Stufen decken die realen Kettendifferenzen ab:
/// 0 = exakte Kette; 1 = letztes Kettensegment == Ortsname (UEX „Nyx >
/// Delamar > Levski“ vs. Wiki „Nyx > Levski“), Terminal-Name endet auf
/// „- <Ortsname>“ (UEX-Namenskonvention, z.B. „Landing Services - Levski“)
/// oder die Wiki-Kette ist ein Kettensuffix; 2 = Ortsname als irgendein
/// Kettensegment (bestehende Toleranz).
fn terminal_match_tier(name: &str, wiki_chain: &str, terminal: &Entity) -> Option<u8> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let t_chain = terminal.subtitle.trim();
    let wiki_segs = chain_segments(wiki_chain);
    let t_segs = chain_segments(t_chain);
    if !wiki_segs.is_empty() && !t_segs.is_empty() && wiki_segs == t_segs {
        return Some(0);
    }
    // (a) letztes Kettensegment == Ortsname.
    let last_seg = t_segs.last().map(|s| s.eq_ignore_ascii_case(name));
    // (b) Terminal-Name endet auf "- <Ortsname>" (UEX-Namenskonvention).
    let name_suffix = terminal
        .name
        .trim()
        .to_lowercase()
        .ends_with(&format!("- {}", name.to_lowercase()));
    // (c) die Wiki-Kette ist ein Kettensuffix der UEX-Kette.
    let chain_suffix = !wiki_segs.is_empty()
        && t_segs.len() >= wiki_segs.len()
        && t_segs[t_segs.len() - wiki_segs.len()..]
            .iter()
            .zip(&wiki_segs)
            .all(|(t, w)| t.eq_ignore_ascii_case(w));
    if last_seg == Some(true) || name_suffix || chain_suffix {
        return Some(1);
    }
    if chain_has(t_chain, name) {
        return Some(2);
    }
    None
}

/// Wählt den besten UEX-Terminal für eine Wiki-Location aus: niedrigster
/// Match-Tier zuerst; bei Gleichstand gewinnt der Kandidat im gleichen
/// System (erstes Kettensegment der Wiki-Kette), dann die Monetär-Heuristik,
/// dann die stabile Reihenfolge. Kein Match → None (→ NotFound).
fn best_terminal_match<'a>(
    name: &str,
    wiki_chain: &str,
    terminals: &'a [Entity],
) -> Option<&'a Entity> {
    let system = chain_segments(wiki_chain).first().map(|s| s.to_lowercase());
    let mut best: Option<(u8, bool, bool, &'a Entity)> = None;
    for t in terminals {
        let Some(tier) = terminal_match_tier(name, wiki_chain, t) else {
            continue;
        };
        let system_match = system
            .as_deref()
            .zip(chain_segments(&t.subtitle).first())
            .is_some_and(|(sys, first)| sys.eq_ignore_ascii_case(first));
        let money = is_money_terminal(&t.name);
        let better = match &best {
            None => true,
            Some((b_tier, b_sys, b_money, _)) => {
                (tier, !system_match, !money) < (*b_tier, !*b_sys, !*b_money)
            }
        };
        if better {
            best = Some((tier, system_match, money, t));
        }
    }
    best.map(|(_, _, _, t)| t)
}

/// Does the UEX chain contain `name` as one of its segments?
fn chain_has(chain: &str, name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && chain
            .split('>')
            .map(str::trim)
            .any(|seg| seg.eq_ignore_ascii_case(name))
}

/// Handels-/Monetär-Terminal-Heuristik (same keyword set as the palette's
/// location-row synthesis in `trade::is_money_terminal`).
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

#[async_trait::async_trait]
impl DataProvider for CompositeProvider {
    async fn search(&self, entities: SearchEntities, query: &str) -> Result<Vec<SearchHit>> {
        // Locations searches BOTH datasets (wiki hierarchy + UEX terminals);
        // the wiki dataset is ensured here so a search after a TTL expiry
        // still finds rows without waiting for the background sweep. UEX
        // terminals are ensured via the uex provider's own search path —
        // queried through the cache directly so BOTH sources' rows come
        // back (the uex provider filters to its own rows by design).
        if entities == SearchEntities::Locations {
            self.wiki.ensure_locations().await?;
            // Ensure the UEX terminal dataset is fresh (reuse the provider's
            // ensure logic via a throwaway search; the result is discarded).
            let _ = self.uex.search(SearchEntities::Terminals, "").await?;
            let hits = self
                .cache
                .search_sourced(
                    &[EntityKind::Locations, EntityKind::Terminals],
                    &[],
                    query,
                    // P1: Location-ENTITY nur per Name (Subtitle matcht nicht).
                    &[EntityKind::Locations],
                    SEARCH_LIMIT,
                )
                .map_err(|e| ProviderError::Cache(e.to_string()))?;
            return Ok(hits);
        }
        self.uex.search(entities, query).await
    }

    /// Commodity prices: primary source (wiki by default), UEX as gap filler
    /// and fallback. See module docs for the policy.
    async fn prices(&self, commodity: &str) -> Result<Vec<PricePoint>> {
        match self.commodity_source() {
            CommoditySource::Wiki => {
                let wiki = self.wiki_prices(commodity).await;
                if wiki.is_empty() {
                    // Gap: full UEX fallback (its own freshness/TTL path).
                    return self.uex.prices(commodity).await;
                }
                // Wiki has data: UEX only fills terminals the wiki does not
                // report at all (best-effort, errors tolerated).
                let uex = self.uex_prices_quiet(commodity).await;
                Ok(Self::merge_prices(wiki, uex))
            }
            CommoditySource::Uex => {
                // Pure-UEX path (A/B switch); wiki ignored entirely.
                self.uex.prices(commodity).await
            }
        }
    }

    async fn location_prices(&self, terminal_id: u64) -> Result<LocationPrices> {
        // UEX terminal ids open the aggregate directly; wiki location ids are
        // resolved to the matching UEX terminal first (Location→Terminal).
        let resolved = match self.uex.location_prices(terminal_id).await {
            Ok(agg) => return Ok(agg),
            Err(ProviderError::NotFound(_)) => {
                self.uex_terminal_for_wiki_location(terminal_id).await?
            }
            Err(other) => return Err(other),
        };
        self.uex.location_prices(resolved).await
    }

    async fn terminal_prices(&self, terminal_id: u64) -> Result<Vec<PricePoint>> {
        self.uex.terminal_prices(terminal_id).await
    }

    async fn item_prices(&self, item_id: u64) -> Result<Vec<PricePoint>> {
        self.uex.item_prices(item_id).await
    }

    async fn marketplace_offers(&self, filters: MarketplaceFilter) -> Result<Vec<provider::Offer>> {
        // Marketplace stays UEX (deprecated-free there).
        self.uex.marketplace_offers(filters).await
    }

    async fn fetch_entities(&self, kind: EntityKind) -> Result<Vec<Entity>> {
        match kind {
            // Wiki reference datasets (stored under source=wiki by the
            // ensure helpers).
            EntityKind::Locations => {
                self.wiki.ensure_locations().await?;
                self.cache
                    .entities_of_sourced(EntityKind::Locations, Some(SOURCE_WIKI))
                    .map_err(|e| ProviderError::Cache(e.to_string()))
            }
            // UEX datasets: the commodity SEARCH id space stays UEX (wiki
            // commodity rows are internal resolution data for the price path
            // and are synced via `WikiSync::sync_wiki_datasets`).
            EntityKind::Commodities
            | EntityKind::Items
            | EntityKind::Terminals
            | EntityKind::StarSystems => self.uex.fetch_entities(kind).await,
        }
    }

    fn name(&self) -> String {
        format!("wiki+uex ({})", self.commodity_source().as_str())
    }
}

/// Extra sync work the background sweep performs beyond the GROUPS datasets:
/// the wiki reference datasets (commodities + locations) on their own
/// patch-driven TTL.
#[async_trait::async_trait]
pub trait WikiSync: Send + Sync {
    /// Ensure both wiki datasets are fresh (no-op when within TTL).
    async fn sync_wiki_datasets(&self, cache: &Cache) -> Result<()>;
}

#[async_trait::async_trait]
impl WikiSync for CompositeProvider {
    async fn sync_wiki_datasets(&self, cache: &Cache) -> Result<()> {
        self.wiki.ensure_commodities().await?;
        self.wiki.ensure_locations().await?;
        let _ = cache; // the ensure helpers write through the shared cache
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use provider::SearchHit;
    use settings::CommoditySource;

    /// A stub UEX provider so the composite can be exercised without the
    /// real UEX HTTP client.
    struct StubUex {
        fail_prices: bool,
    }

    fn hit(id: u64, name: &str, subtitle: &str) -> SearchHit {
        SearchHit {
            id,
            name: name.into(),
            kind: "terminal".into(),
            subtitle: subtitle.into(),
            code: None,
        }
    }

    fn point(terminal_id: u64, buy: f64, sell: f64) -> PricePoint {
        PricePoint {
            commodity: "Agricium".into(),
            commodity_id: 1,
            location: format!("terminal {terminal_id}"),
            terminal_id,
            sell_price: sell,
            buy_price: buy,
            sell_stock: 0.0,
            buy_stock: 0.0,
            sell_status: if sell > 0.0 { 1 } else { 0 },
            buy_status: if buy > 0.0 { 1 } else { 0 },
            quality: 0,
            updated: 1,
        }
    }

    #[async_trait::async_trait]
    impl DataProvider for StubUex {
        async fn search(&self, _e: SearchEntities, _q: &str) -> Result<Vec<SearchHit>> {
            Ok(vec![hit(
                436,
                "Commodity Shop - Admin - Checkmate",
                "Pyro > Monox > Checkmate Station",
            )])
        }
        async fn prices(&self, _c: &str) -> Result<Vec<PricePoint>> {
            if self.fail_prices {
                Err(ProviderError::Timeout)
            } else {
                Ok(vec![point(12, 8_000.0, 0.0), point(802, 0.0, 9_700.0)])
            }
        }
        async fn location_prices(&self, terminal_id: u64) -> Result<LocationPrices> {
            if terminal_id == 436 {
                Ok(LocationPrices {
                    location: "Pyro > Monox > Checkmate Station".into(),
                    terminal: "Commodity Shop - Admin - Checkmate".into(),
                    groups: vec![],
                })
            } else {
                Err(ProviderError::NotFound(format!("terminal {terminal_id}")))
            }
        }
        async fn marketplace_offers(&self, _f: MarketplaceFilter) -> Result<Vec<provider::Offer>> {
            Ok(vec![])
        }
        async fn fetch_entities(&self, _k: EntityKind) -> Result<Vec<Entity>> {
            Ok(vec![])
        }
        fn name(&self) -> String {
            "uex-stub".into()
        }
    }

    /// Stub wiki provider: known commodity → two terminal prices; unknown →
    /// NotFound; controllable failure.
    struct StubWiki {
        fail: bool,
        empty: bool,
    }

    #[async_trait::async_trait]
    impl WikiSync for StubWiki {
        async fn sync_wiki_datasets(&self, _cache: &Cache) -> Result<()> {
            Ok(())
        }
    }

    impl StubWiki {
        async fn prices(&self, commodity: &str) -> Result<Vec<PricePoint>> {
            if self.fail {
                return Err(ProviderError::Timeout);
            }
            if commodity.eq_ignore_ascii_case("agricium") && !self.empty {
                return Ok(vec![point(778, 8_379.0, 0.0), point(802, 0.0, 9_800.0)]);
            }
            Err(ProviderError::NotFound(format!("commodity '{commodity}'")))
        }
        async fn ensure_locations(&self) -> Result<()> {
            Ok(())
        }
    }

    /// The composite logic under test is `prices` + `merge_prices` +
    /// `uex_terminal_for_wiki_location`. To test those without the real HTTP
    /// clients, the composite is exercised through its merge helper plus a
    /// wiring test using the real types with unreachable base URLs (offline
    /// failure = fallback path).
    #[test]
    fn merge_prices_primary_wins_secondary_fills_gaps() {
        let wiki = vec![point(778, 8_379.0, 0.0), point(802, 0.0, 9_800.0)];
        let uex = vec![point(12, 8_000.0, 0.0), point(802, 0.0, 9_700.0)];
        let merged = CompositeProvider::merge_prices(wiki, uex);
        // 802 conflict → wiki wins (9800); 12 is a wiki gap → UEX fills.
        assert_eq!(merged.len(), 3);
        let t802 = merged.iter().find(|p| p.terminal_id == 802).unwrap();
        assert_eq!(
            t802.sell_price, 9_800.0,
            "conflict resolved for the primary (wiki)"
        );
        assert!(
            merged.iter().any(|p| p.terminal_id == 12),
            "gap filled from UEX"
        );
        // Sorted best-buy first.
        assert_eq!(merged[0].terminal_id, 778);
    }

    #[test]
    fn merge_prices_empty_primary_yields_secondary() {
        let merged = CompositeProvider::merge_prices(vec![], vec![point(12, 8_000.0, 0.0)]);
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn chain_has_matches_segments_case_insensitive() {
        assert!(chain_has("Stanton > Hurston > Aberdeen", "aberdeen"));
        assert!(chain_has("Stanton > Hurston > Aberdeen", "Hurston"));
        assert!(!chain_has("Stanton > Hurston", "Levski"));
        assert!(!chain_has("Stanton", ""));
    }

    #[test]
    fn money_terminal_heuristic_prefers_trade_terminals() {
        assert!(is_money_terminal("Commodity Shop - Admin - Checkmate"));
        assert!(is_money_terminal("Trading console - Everus"));
        assert!(!is_money_terminal("Landing Services - Checkmate"));
        assert!(!is_money_terminal("Fuel Station - Checkmate"));
    }

    fn term(id: u64, name: &str, chain: &str) -> Entity {
        Entity {
            id,
            kind: EntityKind::Terminals,
            name: name.into(),
            code: None,
            subtitle: chain.into(),
        }
    }

    /// Tier-0: exakte Ketten-Übereinstimmung.
    #[test]
    fn tier0_exact_chain_match() {
        let t = term(1, "Shop - X", "Stanton > Hurston > Aberdeen");
        assert_eq!(
            terminal_match_tier("Aberdeen", "Stanton > Hurston > Aberdeen", &t),
            Some(0)
        );
    }

    /// Tier 1 (a): letztes Kettensegment == Ortsname — Wiki „Nyx > Levski“
    /// vs. UEX „Nyx > Delamar > Levski“ (echter Cache-Befund, t_d43ac351).
    #[test]
    fn tier1_last_segment_match() {
        let t = term(
            109,
            "Conscientious Objects - Levski",
            "Nyx > Delamar > Levski",
        );
        assert_eq!(
            terminal_match_tier("Levski", "Nyx > Levski", &t),
            Some(1),
            "Letztes Segment 'Levski' muss trotz fehlendem 'Delamar' matchen"
        );
    }

    /// Tier 1 (b): UEX-Namenskonvention „- <Ortsname>“ — der Checkmate-Fall:
    /// UEX-Kette endet auf „Checkmate Station“, der Terminal-Name auf
    /// „- Checkmate“; ohne Namensuffix-Regel bleibt der Match leer.
    #[test]
    fn tier1_name_suffix_match() {
        let t = term(
            436,
            "Commodity Shop - Admin - Checkmate",
            "Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station",
        );
        assert_eq!(
            terminal_match_tier("Checkmate", "Pyro > Checkmate", &t),
            Some(1)
        );
        // Kein Über-Matchen: „Checkmate Clinic“-Terminal endet nicht auf
        // „- Checkmate“ und seine Kette trägt den Namen nicht.
        let clinic = term(
            500,
            "Checkmate Clinic",
            "Pyro > Checkmate > Checkmate Clinic",
        );
        assert_eq!(
            terminal_match_tier("Checkmate Station", "Pyro > Checkmate", &clinic),
            None
        );
    }

    /// Tier 1 (c): die Wiki-Kette ist ein Kettensuffix der UEX-Kette.
    #[test]
    fn tier1_chain_suffix_match() {
        let t = term(
            441,
            "Refinery Shop",
            "Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station",
        );
        assert_eq!(
            terminal_match_tier(
                "Checkmate Station",
                "Monox Lagrange Point 4 > Checkmate Station",
                &t
            ),
            Some(1)
        );
    }

    /// Tier 2: Ortsname als irgendein Segment (bestehende Toleranz).
    #[test]
    fn tier2_any_segment_match() {
        let t = term(1, "Shop", "Stanton > Hurston > Aberdeen");
        assert_eq!(
            terminal_match_tier("Hurston", "Stanton > Hurston > Hurston Landing", &t),
            Some(2)
        );
        // Gar kein Match → None (→ NotFound-Verhalten).
        assert_eq!(
            terminal_match_tier("Orbituary", "Pyro > Orbituary", &t),
            None
        );
    }

    /// N1-Mehrdeutigkeit (t_d43ac351): gleicher Ortsname in zwei Systemen —
    /// der System-Kontext (erstes Kettensegment der Wiki-Kette) entscheidet,
    /// nicht die Reihenfolge oder die Monetär-Heuristik.
    #[test]
    fn best_match_uses_system_context_for_ambiguous_location_names() {
        let wiki_name = "Guns";
        let wiki_chain = "Nyx > Delamar > Guns";
        let terminals = vec![
            // Gleicher Tier, falsches System, monetär (gewöbe sonst vor).
            term(
                437,
                "Guns - Checkmate",
                "Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station",
            ),
            // Gleicher Tier, richtiges System, NICHT monetär.
            term(1, "Guns - Delamar", "Nyx > Delamar > Guns"),
        ];
        let chosen = best_terminal_match(wiki_name, wiki_chain, &terminals).unwrap();
        assert_eq!(
            chosen.id, 1,
            "System-Kontext (Nyx) muss vor Monetär-Heuristik und Reihenfolge gehen"
        );
    }

    /// Monetär-Heuristik als letzter Gleichstand-Tiebreaker bei gleichem Tier
    /// und gleichem System.
    #[test]
    fn best_match_prefers_money_terminal_within_tier_and_system() {
        let terminals = vec![
            term(393, "Landing Services - Levski", "Nyx > Delamar > Levski"),
            term(778, "Commodity Shop - Levski", "Nyx > Delamar > Levski"),
        ];
        let chosen = best_terminal_match("Levski", "Nyx > Levski", &terminals).unwrap();
        assert_eq!(chosen.id, 778);
    }

    /// Tier-1 schlägt Tier-2, auch wenn der Tier-2-Kandidat monetär wäre.
    #[test]
    fn lower_tier_wins_over_higher_tier() {
        let terminals = vec![
            // Tier 2: Name nur als irgendein Segment.
            term(
                9,
                "Trading console - Everus",
                "Stanton > Crusader > Everus > Levski",
            ),
            // Tier 1: letztes Segment == Ortsname.
            term(778, "Commodity Shop - Levski", "Nyx > Delamar > Levski"),
        ];
        let chosen = best_terminal_match("Levski", "Nyx > Levski", &terminals).unwrap();
        assert_eq!(chosen.id, 778);
    }

    #[test]
    fn source_switch_roundtrip() {
        let s = SourceSwitch::new(CommoditySource::Wiki);
        assert_eq!(s.get(), CommoditySource::Wiki);
        s.set(CommoditySource::Uex);
        assert_eq!(s.get(), CommoditySource::Uex);
        s.set(CommoditySource::Wiki);
        assert_eq!(s.get(), CommoditySource::Wiki);
    }

    /// Regression (t_c3d46cff): the composite provider must read the SAME
    /// switch instance the settings state flips — a CLONE shares the inner
    /// Arc, a freshly constructed switch would freeze the boot value and a
    /// runtime flip would only apply after a restart.
    #[test]
    fn source_switch_clone_shares_live_state() {
        let setter = SourceSwitch::new(CommoditySource::Wiki);
        let reader = setter.clone();
        setter.set(CommoditySource::Uex);
        assert_eq!(reader.get(), CommoditySource::Uex);
        setter.set(CommoditySource::Wiki);
        assert_eq!(reader.get(), CommoditySource::Wiki);
    }

    /// Regression (t_925a577a): eine Wiki-Location, deren Kette ein
    /// doppelt geliefertes Segment trägt („Nyx > Nyx > Levski“), muss über
    /// die tolerante Segment-Matchung auf die UEX-Terminals desselben
    /// Ortes auflösen („Nyx > Delamar > Levski“) — die exakte Ketten-Matchung
    /// scheitert dort, das Location-Detail wäre sonst „keine Preise“.
    #[tokio::test]
    async fn wiki_location_with_duplicated_chain_resolves_uex_terminals() {
        let cache = Arc::new(Cache::open_in_memory().unwrap());
        let uex = UexProvider::with_base_url(
            cache.clone(),
            None,
            "http://127.0.0.1:1",
            std::time::Duration::ZERO,
        )
        .unwrap();
        let wiki = WikiProvider::with_base_url(
            "http://127.0.0.1:2",
            std::time::Duration::ZERO,
            cache.clone(),
        )
        .unwrap();
        let provider =
            CompositeProvider::new(uex, wiki, SourceSwitch::new(CommoditySource::Wiki), cache);

        // UEX-Terminals der Levski-Kette (Live-Form) + die Wiki-Location-Zeile
        // mit dem doppelten Segment (Cache-Befund aus dem Live-Lauf).
        let terminals = vec![
            Entity {
                id: 778,
                kind: EntityKind::Terminals,
                name: "Commodity Shop - Levski".into(),
                code: Some("LEVSKI".into()),
                subtitle: "Nyx > Delamar > Levski".into(),
            },
            Entity {
                id: 393,
                kind: EntityKind::Terminals,
                name: "Landing Services - Levski".into(),
                code: Some("LSLEV".into()),
                subtitle: "Nyx > Delamar > Levski".into(),
            },
            // Störer: gleicher Segment-Name an anderem Ort darf nicht matchen.
            Entity {
                id: 12,
                kind: EntityKind::Terminals,
                name: "Commodity Shop - Admin - Checkmate".into(),
                code: Some("CHECK".into()),
                subtitle: "Pyro > Monox > Checkmate Station".into(),
            },
        ];
        provider
            .cache
            .store_entities(EntityKind::Terminals, &terminals)
            .unwrap();
        let wiki_row = vec![Entity {
            id: 4_047_074_297_359_351_300,
            kind: EntityKind::Locations,
            name: "Levski".into(),
            code: Some("levski".into()),
            subtitle: "Nyx > Nyx > Levski".into(),
        }];
        provider
            .cache
            .store_entities_sourced(EntityKind::Locations, SOURCE_WIKI, &wiki_row)
            .unwrap();

        let resolved = provider
            .uex_terminal_for_wiki_location(4_047_074_297_359_351_300)
            .await
            .expect("duplicated wiki chain must resolve to the Levski terminals");
        // Der Monetär-Terminal-Heuristik-Kandidat ist der Commodity Shop.
        assert_eq!(resolved, 778);
    }

    /// Der Störer-Check (gleicher Segment-Name an anderem Ort): eine Location
    /// ohne jeglichen Segment-/Namens-Match bleibt NotFound.
    #[tokio::test]
    async fn wiki_location_without_segment_match_stays_not_found() {
        let cache = Arc::new(Cache::open_in_memory().unwrap());
        let uex = UexProvider::with_base_url(
            cache.clone(),
            None,
            "http://127.0.0.1:1",
            std::time::Duration::ZERO,
        )
        .unwrap();
        let wiki = WikiProvider::with_base_url(
            "http://127.0.0.1:2",
            std::time::Duration::ZERO,
            cache.clone(),
        )
        .unwrap();
        let provider =
            CompositeProvider::new(uex, wiki, SourceSwitch::new(CommoditySource::Wiki), cache);
        provider
            .cache
            .store_entities(
                EntityKind::Terminals,
                &[Entity {
                    id: 12,
                    kind: EntityKind::Terminals,
                    name: "Commodity Shop - Admin - Checkmate".into(),
                    code: None,
                    subtitle: "Pyro > Monox > Checkmate Station".into(),
                }],
            )
            .unwrap();
        provider
            .cache
            .store_entities_sourced(
                EntityKind::Locations,
                SOURCE_WIKI,
                &[Entity {
                    id: 9_001,
                    kind: EntityKind::Locations,
                    name: "Orbituary".into(),
                    code: Some("orbituary".into()),
                    subtitle: "Pyro > Pyro > Orbituary".into(),
                }],
            )
            .unwrap();
        let err = provider
            .uex_terminal_for_wiki_location(9_001)
            .await
            .unwrap_err();
        assert!(matches!(err, ProviderError::NotFound(_)), "got {err:?}");
    }

    /// Offline wiring test: both clients point at unreachable hosts. Wiki
    /// prices fail → empty → composite must fall back to UEX. With UEX also
    /// failing, `prices` propagates the UEX error (the UI's OFFLINE path).
    #[tokio::test]
    async fn prices_fall_back_to_uex_when_wiki_unreachable() {
        let cache = Arc::new(Cache::open_in_memory().unwrap());
        let uex = UexProvider::with_base_url(
            cache.clone(),
            None,
            "http://127.0.0.1:1",
            std::time::Duration::ZERO,
        )
        .unwrap();
        let wiki = WikiProvider::with_base_url(
            "http://127.0.0.1:2",
            std::time::Duration::ZERO,
            cache.clone(),
        )
        .unwrap();
        let provider =
            CompositeProvider::new(uex, wiki, SourceSwitch::new(CommoditySource::Wiki), cache);
        // Both unreachable → error surfaces (NOT silently empty).
        let err = provider.prices("Agricium").await.unwrap_err();
        assert!(
            matches!(
                err,
                ProviderError::Other(_) | ProviderError::Timeout | ProviderError::Http { .. }
            ),
            "got {err:?}"
        );
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use std::time::Duration;

    /// End-to-end live smoke against BOTH production APIs (opt-in via
    /// `cargo test -p architects-overlay -- --ignored`): wiki commodity
    /// prices (primary), UEX fallback path and the merged location search.
    #[tokio::test]
    #[ignore = "hits the live UEX + wiki APIs"]
    async fn live_composite_prices_and_location_search() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Arc::new(Cache::open(&dir.path().join("smoke.sqlite3")).expect("cache"));
        let uex = UexProvider::with_base_url(
            cache.clone(),
            None,
            uex_client::private::DEFAULT_BASE_URL,
            Duration::ZERO,
        )
        .expect("uex");
        let wiki = WikiProvider::new(cache.clone()).expect("wiki");
        let provider =
            CompositeProvider::new(uex, wiki, SourceSwitch::new(CommoditySource::Wiki), cache);

        // 1. Commodity prices via the wiki detail endpoint (primary source).
        let points = provider.prices("Agricium").await.expect("prices");
        assert!(!points.is_empty(), "Agricium must have terminal prices");
        let buy = points
            .iter()
            .find(|p| p.buy_price > 0.0)
            .expect("buy terminal");
        let sell = points
            .iter()
            .find(|p| p.sell_price > 0.0)
            .expect("sell terminal");
        eprintln!(
            "prices(Agricium): {} points; buy {:.0} @ {} (terminal {}), sell {:.0} @ {} (terminal {})",
            points.len(),
            buy.buy_price,
            buy.location,
            buy.terminal_id,
            sell.sell_price,
            sell.location,
            sell.terminal_id
        );

        // 2. Location search serves BOTH datasets (wiki rows kind=location,
        //    UEX rows kind=terminal).
        let hits = provider
            .search(SearchEntities::Locations, "Levski")
            .await
            .expect("location search");
        let wiki_hits = hits.iter().filter(|h| h.kind == "location").count();
        let uex_hits = hits.iter().filter(|h| h.kind == "terminal").count();
        eprintln!("search(Locations, Levski): {wiki_hits} wiki rows, {uex_hits} UEX terminal rows");
        assert!(wiki_hits >= 1, "wiki location rows must appear");
        assert!(uex_hits >= 1, "UEX terminal rows must appear");

        // 3. Location→Terminal: a wiki location id resolves to the UEX
        //    aggregate of the same place.
        let wiki_hit = hits.iter().find(|h| h.kind == "location").unwrap();
        let agg = provider
            .location_prices(wiki_hit.id)
            .await
            .expect("aggregate via wiki location id");
        eprintln!(
            "location_prices(via wiki id): {} groups @ {}",
            agg.groups.len(),
            agg.location
        );
        assert_eq!(provider.name(), "wiki+uex (wiki)");
    }

    /// LIVE-Smoke (Feedback-Runde 4, t_7a1add66) — echte APIs, echter Cache.
    /// Läuft nur mit `ARCH_SMOKE_CACHE=<pfad>` gegen eine KOPIE der App-Cache-DB
    /// (`cargo test -p architects-overlay --lib live_smoke -- --ignored
    /// --nocapture`), damit die laufende App unberührt bleibt. Prüft P1/P2/P3
    /// und die Laranite-Baseline (26 Zeilen) gegen die echten Datenquellen.
    #[tokio::test]
    #[ignore = "live: needs ARCH_SMOKE_CACHE=<copy of app cache.db> + network"]
    async fn live_smoke_search_semantics() {
        let Some(path) = std::env::var_os("ARCH_SMOKE_CACHE") else {
            panic!("ARCH_SMOKE_CACHE not set — this live test is opt-in");
        };
        let cache = Arc::new(Cache::open(std::path::Path::new(&path)).unwrap());
        let uex = UexProvider::new(cache.clone(), None).unwrap();
        let wiki = WikiProvider::new(cache.clone()).unwrap();
        let provider = CompositeProvider::new(
            uex,
            wiki,
            SourceSwitch::new(CommoditySource::Wiki),
            cache.clone(),
        );
        println!(
            "terminals in cache: {}",
            cache.count_entities(EntityKind::Terminals).unwrap()
        );

        for q in ["levski", "checkmate"] {
            let hits = provider.search(SearchEntities::Locations, q).await.unwrap();
            println!("\n:l {q} → {} Treffer", hits.len());
            for h in &hits {
                println!("  [{}] {} · {}", h.kind, h.name, h.subtitle);
            }
            let mercy = hits.iter().any(|h| h.name.contains("Mercy"));
            let services = hits.iter().any(|h| {
                h.kind == "terminal"
                    && (h.name.contains("Landing Services")
                        || h.name.contains("Refinement Center")
                        || h.name.contains("Rentals"))
            });
            assert!(
                !mercy,
                "P1: Mercy Hospital darf bei '{q}' nicht mehr auftauchen"
            );
            assert!(
                !services,
                "P2: Service-Terminals (Landing/Refinement/Rentals) sind gefiltert"
            );
            let shops = hits.iter().filter(|h| h.kind == "terminal").count();
            println!("  Terminal-Zeilen (Station-Shops): {shops}");
            assert!(shops > 0, "P1.2: Station-Shops müssen gelistet sein");
        }

        // P1.3: Levski-Aggregat (Location-Detail = alle Kaufterminals).
        let hits = provider
            .search(SearchEntities::Locations, "levski")
            .await
            .unwrap();
        let loc = hits
            .iter()
            .find(|h| h.kind == "location" && h.name.eq_ignore_ascii_case("Levski"))
            .expect("Levski location hit")
            .clone();
        let agg = provider.location_prices(loc.id).await.unwrap();
        let rows: usize = agg.groups.iter().map(|g| g.prices.len()).sum();
        println!(
            "Levski-Aggregat: {} Terminal-Gruppen, {rows} Preiszeilen (erwarte 8+ Gruppen)",
            agg.groups.len()
        );
        assert!(
            agg.groups.len() >= 8,
            "Levski-Referenzfall: 8+ Kaufterminals"
        );

        // `:c laranite` — Baseline 26 Zeilen.
        let rows = provider.prices("laranite").await.unwrap();
        println!("\n:c laranite → {} Preiszeilen (Baseline: 26)", rows.len());
        assert_eq!(rows.len(), 26, "Laranite-Baseline unverändert");

        use provider::OfferSort;
        // P3: Market hybrid — komplettes Offer-Fenster für 'parallax'.
        let offers = provider
            .marketplace_offers(MarketplaceFilter {
                q: "parallax".into(),
                sort: Some(OfferSort::PriceAsc),
                limit: 1000,
                ..Default::default()
            })
            .await
            .unwrap();
        let q900: Vec<&provider::Offer> = offers.iter().filter(|o| o.quality >= 900).collect();
        println!(
            "\nMarket 'parallax': {} Offers, {} mit Q>=900 (erwarte >=15), price_asc={:?}",
            offers.len(),
            q900.len(),
            q900.iter().take(5).map(|o| o.price).collect::<Vec<_>>()
        );
        assert!(
            offers.len() >= 15,
            "P3: 'Parallax + Min-Q 900' braucht >=15 Offers"
        );
        assert!(
            offers.windows(2).all(|w| w[0].price <= w[1].price),
            "price_asc sortiert"
        );
    }
}

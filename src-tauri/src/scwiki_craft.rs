//! SC-Wiki Craft-Daten (M8-K5, t_03eafbfb) — Command-Exposure der
//! M8-K4-Client-Endpunkte (`craftable_items` / `blueprint`) für den
//! Crafting-Modus.
//!
//! * **Cache-first:** beide Reads laufen über `route_meta` (eigener
//!   `scwiki:`-Namespace, TTL [`TTL_SCWIKI_CRAFT`] = 7 Tage —
//!   Patch-zyklische Daten, die Liste ist EIN Datensatz). Frisch → aus dem
//!   Cache; sonst Live-Fetch + Upsert; Fetch-Fehler mit vorhandenem
//!   (stale) Cache → stale degradiert zurück, OHNE Fehler — die Craft-
//!   Details sind Bereicherung, kein Pfad des Kern-Workflows.
//! * **Kreuzreferenz (Spec M8-K4):** `scwiki_craft_detail(item_name)`
//!   matcht den Strata-Item-Namen über [`scwiki_client::StrataIndex`]
//!   (trim + lowercase, exakt — fehlender Match = `None`, kein Hard-Fail)
//!   auf ein SC-Wiki-Craft-Item und löst den ersten `blueprint_uuid`
//!   zum Blueprint-Detail auf.
//! * **Fehler:** `error.scwiki.*`-i18n-Keys (JSON-Contract T2) — das
//!   Frontend resolviert über die Locale-Fallback-Kette.
//! * **Attribution:** [`ATTRIBUTION_SCWIKI`] reist in JEDER Response mit
//!   (ToS-Pflicht) und wird von der UI zusätzlich zur Strata-Angabe
//!   gezeigt.

use std::sync::Arc;

use cache::{Cache, TTL_SCWIKI_CRAFT};
use scwiki_client::{
    ATTRIBUTION_SCWIKI, Blueprint, CraftableItem, MiningIndex, SCMDB_DEFAULT_BASE_URL,
    ScWikiClient, ScmdbClient, StrataIndex, normalize_name,
};
use serde::Serialize;
use serde_json::json;

use crate::i18n_err::{err_key, err_keyp};

/// Route-Meta-Key der gecachten craftable-Items-Liste (EIN Datensatz —
/// Suchfilter greift client-seitig, kein zweiter Cache pro Query).
const KEY_ITEMS: &str = "scwiki:craftable_items";
/// Prefix der Blueprint-Cache-Keys (`scwiki:blueprint:{uuid}`).
const KEY_BLUEPRINT_PREFIX: &str = "scwiki:blueprint:";
/// Route-Meta-Key des gecachten SCMDB-Mining-Index (EIN Datensatz —
/// Name→Band-Map, Patch-zyklisch, TTL [`TTL_SCWIKI_CRAFT`], M8-R2).
const KEY_SCMDB_MINING: &str = "scmdb:mining_index";

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `ScWikiError` → `error.scwiki.*`-Key (JSON-Contract T2). Netzwerkfehler
/// (reqwest) laufen unter `timeout` (die UI zeigt „SC-Wiki nicht erreichbar
/// / später erneut" — Rate-Limit-Etikette ist client-seitig durch Pacing
/// abgedeckt, es gibt keinen eigenen rate_limit-Fall).
fn scwiki_error_key(e: &scwiki_client::ScWikiError) -> String {
    use scwiki_client::ScWikiError as E;
    match e {
        E::Status(status) => err_keyp("error.scwiki.http", json!({ "status": status })),
        E::Json(msg) => err_keyp("error.scwiki.bad_response", json!({ "msg": msg })),
        E::TooManyPages(pages) => err_keyp(
            "error.scwiki.bad_response",
            json!({ "msg": format!("pagination stopped after {pages} pages") }),
        ),
        E::Http(_) => err_key("error.scwiki.unreachable"),
    }
}

/// Geteilter Zustand für die SC-Wiki-Craft-Commands — nur der Cache
/// (denselben Handle wie StrataState; kein zweiter Zustands-Typ).
pub struct ScWikiCraftState {
    pub cache: Arc<Cache>,
}

/// Alle craftbaren Items (`GET /api/items?filter[is_craftable]=1`) —
/// cache-first, TTL [`TTL_SCWIKI_CRAFT`]. `search` filtert lokal
/// (case-insensitiv im Namen); kein eigener Live-Fetch pro Query
/// (Rate-Limit-Etikette + EIN Datensatz im Cache).
#[tauri::command]
pub async fn scwiki_craftable_items(
    state: tauri::State<'_, ScWikiCraftState>,
    search: Option<String>,
) -> Result<Vec<CraftableItem>, String> {
    let items = cached_craftable_items(&state.cache).await?;
    Ok(match search.as_deref().map(str::trim) {
        Some(q) if !q.is_empty() => {
            let needle = q.to_lowercase();
            items
                .into_iter()
                .filter(|it| it.name.to_lowercase().contains(&needle))
                .collect()
        }
        _ => items,
    })
}

/// Blueprint-Detail per UUID (`GET /api/blueprints/{uuid}`) — cache-first,
/// TTL [`TTL_SCWIKI_CRAFT`], Cache-Key enthält die UUID.
#[tauri::command]
pub async fn scwiki_blueprint(
    state: tauri::State<'_, ScWikiCraftState>,
    uuid: String,
) -> Result<Blueprint, String> {
    cached_blueprint(&state.cache, uuid.trim()).await
}

/// Kreuzreferenz Strata → SC-Wiki: Item-Detail + Blueprint für einen
/// Strata-Item-Namen (Craft-Detail-Kopf, Stat-Block, Missionen). Kein
/// Match → `None` (UI degradiert: Abschnitte bleiben weg, kein Hard-Fail).
#[tauri::command]
pub async fn scwiki_craft_detail(
    state: tauri::State<'_, ScWikiCraftState>,
    item_name: String,
) -> Result<Option<CraftDetailResponse>, String> {
    let name = item_name.trim();
    if name.is_empty() {
        return Ok(None);
    }
    let items = cached_craftable_items(&state.cache).await?;
    let index = StrataIndex::build(items.iter().map(|it| it.name.as_str()));
    // StrataIndex.find mappt SC-Wiki → Strata-Namen; hier suchen wir die
    // Gegenrichtung (Strata-Name → SC-Wiki-Item), gleiche Exaktheit.
    let matched = items.into_iter().find(|it| {
        index.find(&it.name).is_some_and(|strata| {
            strata.eq_ignore_ascii_case(name) || normalize_name(&it.name) == normalize_name(name)
        })
    });
    let Some(item) = matched else {
        return Ok(None);
    };
    // Erster Blueprint-UUID genügt (Live-Beleg: eins pro craftbarem Item).
    let blueprint = match item.blueprint_uuids.first() {
        Some(uuid) => match cached_blueprint(&state.cache, uuid).await {
            Ok(bp) => Some(bp),
            // Blueprint-Fetch-Fehler = Bereicherung degradiert (Item-Kopf
            // bleibt; Missionen/Stats-aspects entfallen) — KEIN Hard-Fail.
            Err(e) => {
                log::warn!("scwiki-craft: blueprint {uuid}: {e}");
                None
            }
        },
        None => None,
    };
    Ok(Some(CraftDetailResponse {
        item,
        blueprint,
        attribution: ATTRIBUTION_SCWIKI.to_owned(),
    }))
}

/// Response von [`scwiki_craft_detail`] — Item-Kopf + Blueprint-Detail.
#[derive(Debug, Clone, Serialize)]
pub struct CraftDetailResponse {
    pub item: CraftableItem,
    pub blueprint: Option<Blueprint>,
    pub attribution: String,
}

/// Snap-Punkte eines Materials (M8-R2) — leere Liste = kein Match im
/// SCMDB-Mining-Index → UI rastet auf 500-only (500 ist der Always-Punkt).
#[derive(Debug, Clone, Serialize)]
pub struct QualitySnap {
    pub uuid: String,
    pub values: Vec<u32>,
}

/// Qualitäts-Snap-Punkte für mehrere Materialien (M8-R2) — Quelle ist der
/// SCMDB-Mining-Index (`data/latest.json` → `mining_data-<version>.json`,
/// KrovaxCode/SCMDB_DATA), cache-first mit TTL [`TTL_SCWIKI_CRAFT`].
///
/// Auflösung rein über den Materialnamen: normalize (lowercase/trim +
/// `(Ore)`/`(Raw)`/`(Pure)`-Suffix STRIPpen) → Match gegen materialName
/// ODER name-Form („Taranite (Raw)" trägt materialName Taranite). Kein
/// Match → leere Liste (die UI rastet dann auf dem 500-Always-Punkt) —
/// NICHT mehr frei (das 0.5.88-Verhalten ist zurückgenommen). Der
/// SC-Wiki-quantized-Codepfad (Commodity-Endpoint + (Ore)-Fallback,
/// t_1dd162e3) ist entfernt: SCMDB deckt alle ab.
#[tauri::command]
pub async fn scmdb_quality_snaps(
    state: tauri::State<'_, ScWikiCraftState>,
    uuids: Vec<String>,
    names: Option<Vec<String>>,
) -> Result<Vec<QualitySnap>, String> {
    let names = names.unwrap_or_default();
    let requests: Vec<(String, Option<String>)> = uuids
        .iter()
        .enumerate()
        .map(|(i, u)| (u.clone(), names.get(i).cloned()))
        .collect();
    quality_snaps_inner(&state.cache, &requests, SCMDB_DEFAULT_BASE_URL).await
}

/// (uuid, Materialname)-Paar → Band-Snap-Punkte; `names[i]` gehört zu
/// `uuids[i]` (Frontend baut beide aus derselben Aspekt-Liste). Der
/// Index-Fetch degradiert: Fehler → ALLE Zeilen ohne Punkte (Bereicherung,
/// kein Hard-Fail — die Regler rasten auf 500-only).
async fn quality_snaps_inner(
    cache: &Cache,
    requests: &[(String, Option<String>)],
    base_url: &str,
) -> Result<Vec<QualitySnap>, String> {
    let index = match cached_mining_index(cache, base_url).await {
        Ok(idx) => idx,
        Err(e) => {
            log::warn!("scwiki-craft: SCMDB-Mining-Index: {e} — Snap-Punkte entfallen");
            MiningIndex::default()
        }
    };
    let mut out = Vec::new();
    for (uuid, name) in requests {
        let uuid = uuid.trim();
        if uuid.is_empty() {
            continue;
        }
        let trimmed = name.as_deref().map(str::trim).unwrap_or_default();
        let values = if trimmed.is_empty() {
            Vec::new()
        } else {
            let bands = index.bands_for(trimmed);
            if bands.is_empty() {
                log::info!("scmdb: {trimmed}: kein Band-Match — Regler rastet auf 500-only");
            } else {
                log::info!("scmdb: {trimmed}: {} Band-Snap-Punkte", bands.len());
            }
            bands
        };
        out.push(QualitySnap {
            uuid: uuid.to_owned(),
            values,
        });
    }
    Ok(out)
}

/// SCMDB-Mining-Index: frisch aus dem Cache, sonst Live-Fetch (latest.json
/// → mining_data-<version>.json, EIN latest-Fetch + EIN Daten-Fetch) +
/// Upsert. Fetch-Fehler mit vorhandenem stale Cache → stale zurück.
async fn cached_mining_index(cache: &Cache, base_url: &str) -> Result<MiningIndex, String> {
    if let Some((fetched_at, payload)) = cache.route_meta_get(KEY_SCMDB_MINING).ok().flatten()
        && now_unix().saturating_sub(fetched_at) < TTL_SCWIKI_CRAFT.as_secs()
    {
        return serde_json::from_str(&payload)
            .map_err(|e| err_keyp("error.scwiki.bad_response", json!({ "msg": e.to_string() })));
    }
    let client = ScmdbClient::new(base_url).map_err(|e| e.to_string())?;
    match client.mining_index().await {
        Ok(idx) => {
            if let Ok(payload) = serde_json::to_string(&idx)
                && let Err(e) = cache.route_meta_set(KEY_SCMDB_MINING, now_unix(), &payload)
            {
                log::warn!("scmdb: cache write {KEY_SCMDB_MINING}: {e}");
            }
            Ok(idx)
        }
        Err(e) => {
            if let Ok(Some((_, payload))) = cache.route_meta_get(KEY_SCMDB_MINING)
                && let Ok(idx) = serde_json::from_str::<MiningIndex>(&payload)
            {
                log::warn!("scmdb: live fehlgeschlagen ({e}) — stale Mining-Index");
                return Ok(idx);
            }
            Err(err_keyp(
                "error.scwiki.unreachable",
                json!({ "msg": e.to_string() }),
            ))
        }
    }
}

// ---------- Cache-First-Helfer (nicht-Command, testbar) -------------------

/// Craftable-Items: frisch aus dem Cache, sonst Live-Fetch (Upsert).
/// Fetch-Fehler mit vorhandenem stale Cache → stale zurück (degradiert).
async fn cached_craftable_items(cache: &Cache) -> Result<Vec<CraftableItem>, String> {
    cached_craftable_items_at(cache, scwiki_client::DEFAULT_BASE_URL).await
}

/// Testbare Variante mit überschriebener Basis-URL (Hermetik: Unit-Tests
/// zeigen auf eine unreachbare Adresse statt auf die Live-API).
async fn cached_craftable_items_at(
    cache: &Cache,
    base_url: &str,
) -> Result<Vec<CraftableItem>, String> {
    if let Some((fetched_at, payload)) = cache.route_meta_get(KEY_ITEMS).ok().flatten()
        && now_unix().saturating_sub(fetched_at) < TTL_SCWIKI_CRAFT.as_secs()
    {
        return serde_json::from_str(&payload)
            .map_err(|e| err_keyp("error.scwiki.bad_response", json!({ "msg": e.to_string() })));
    }
    let client = scwiki_client_at(base_url)
        .map_err(|e| err_keyp("error.scwiki.bad_response", json!({ "msg": e })))?;
    match client.craftable_items(None).await {
        Ok(items) => {
            if let Ok(payload) = serde_json::to_string(&items)
                && let Err(e) = cache.route_meta_set(KEY_ITEMS, now_unix(), &payload)
            {
                log::warn!("scwiki-craft: cache write {KEY_ITEMS}: {e}");
            }
            Ok(items)
        }
        Err(e) => {
            // Stale Cache retten (Offline-Toleranz), sonst Fehler nach oben.
            if let Ok(Some((_, payload))) = cache.route_meta_get(KEY_ITEMS)
                && let Ok(items) = serde_json::from_str::<Vec<CraftableItem>>(&payload)
            {
                log::warn!("scwiki-craft: live fetch fehlgeschlagen ({e}) — stale Cache");
                return Ok(items);
            }
            Err(scwiki_error_key(&e))
        }
    }
}

/// Blueprint-Detail: frisch aus dem Cache, sonst Live-Fetch (Upsert).
async fn cached_blueprint(cache: &Cache, uuid: &str) -> Result<Blueprint, String> {
    cached_blueprint_at(cache, scwiki_client::DEFAULT_BASE_URL, uuid).await
}

async fn cached_blueprint_at(
    cache: &Cache,
    base_url: &str,
    uuid: &str,
) -> Result<Blueprint, String> {
    let uuid = uuid.trim();
    if uuid.is_empty() {
        return Err(err_keyp(
            "error.scwiki.bad_response",
            json!({ "msg": "blueprint: uuid leer" }),
        ));
    }
    let key = format!("{KEY_BLUEPRINT_PREFIX}{uuid}");
    if let Some((fetched_at, payload)) = cache.route_meta_get(&key).ok().flatten()
        && now_unix().saturating_sub(fetched_at) < TTL_SCWIKI_CRAFT.as_secs()
    {
        return serde_json::from_str(&payload)
            .map_err(|e| err_keyp("error.scwiki.bad_response", json!({ "msg": e.to_string() })));
    }
    let client = scwiki_client_at(base_url)
        .map_err(|e| err_keyp("error.scwiki.bad_response", json!({ "msg": e })))?;
    match client.blueprint(uuid).await {
        Ok(bp) => {
            if let Ok(payload) = serde_json::to_string(&bp)
                && let Err(e) = cache.route_meta_set(&key, now_unix(), &payload)
            {
                log::warn!("scwiki-craft: cache write {key}: {e}");
            }
            Ok(bp)
        }
        Err(e) => {
            if let Ok(Some((_, payload))) = cache.route_meta_get(&key)
                && let Ok(bp) = serde_json::from_str::<Blueprint>(&payload)
            {
                log::warn!("scwiki-craft: blueprint {uuid} live fehlgeschlagen ({e}) — stale");
                return Ok(bp);
            }
            Err(scwiki_error_key(&e))
        }
    }
}

fn scwiki_client_at(base_url: &str) -> Result<ScWikiClient, String> {
    scwiki_client::ScWikiClient::new(base_url).map_err(|e| e.to_string())
}

/// `true`, wenn die craftable-Items-Liste im Cache liegt (frisch ODER
/// stale — Hauptsache vorhanden): StrataStatus-Integration „scwiki_ready".
pub fn scwiki_ready(cache: &Cache) -> bool {
    cache
        .route_meta_fetched_at(KEY_ITEMS)
        .map(|v| v.is_some())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use scwiki_client::MineableElement;

    fn cache() -> Cache {
        Cache::open_in_memory().unwrap()
    }

    fn item(name: &str) -> CraftableItem {
        CraftableItem {
            name: name.to_owned(),
            item_type: "WeaponGun".to_owned(),
            size: Some(2),
            grade: Some("A".to_owned()),
            class: None,
            mass: Some(192.0),
            description_de: Some("de".to_owned()),
            description_en: Some("en".to_owned()),
            blueprint_uuids: vec!["uuid-1".to_owned()],
            vehicle_weapon: None,
            ammunition: None,
        }
    }

    fn seed(cache: &Cache, items: &[CraftableItem], fresh: bool) {
        let fetched_at = if fresh { now_unix() } else { 0 };
        cache
            .route_meta_set(
                KEY_ITEMS,
                fetched_at,
                &serde_json::to_string(items).unwrap(),
            )
            .unwrap();
    }

    /// Unreachbare Test-Adresse: sofortiger Http-Fehler ohne Netz.
    const TEST_URL: &str = "http://127.0.0.1:1";

    #[tokio::test]
    async fn craftable_items_reads_fresh_cache_without_live_fetch() {
        let cache = cache();
        seed(&cache, &[item("10-Series Greatsword Cannon")], true);
        let out = cached_craftable_items_at(&cache, TEST_URL).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "10-Series Greatsword Cannon");
    }

    #[tokio::test]
    async fn search_filters_case_insensitive_on_cached_list() {
        let cache = cache();
        seed(
            &cache,
            &[item("Alpha Cannon"), item("beta gun"), item("Gamma")],
            true,
        );
        let all = cached_craftable_items_at(&cache, TEST_URL).await.unwrap();
        let filtered: Vec<_> = all
            .into_iter()
            .filter(|it| it.name.to_lowercase().contains("can"))
            .collect();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "Alpha Cannon");
    }

    /// Spec M8-K4: fehlender Match = None (kein Hard-Fail), Match exakt
    /// nach trim+lowercase („  10-series greatsword cannon  " → Greatsword).
    #[tokio::test]
    async fn craft_detail_matches_exactly_and_degrades_on_missing() {
        let cache = cache();
        let items = vec![item("10-Series Greatsword Cannon"), item("Titanium")];
        seed(&cache, &items, true);
        // Blueprint-UUID zeigt ins Leere → Blueprint degradiert auf None,
        // die Response (Item-Kopf) bleibt trotzdem vorhanden.
        let r = cached_craft_detail_inner(&cache, TEST_URL, "  10-series greatsword cannon ")
            .await
            .unwrap()
            .expect("Match erwartet");
        assert_eq!(r.item.name, "10-Series Greatsword Cannon");
        assert_eq!(r.attribution, ATTRIBUTION_SCWIKI);
        assert!(r.blueprint.is_none(), "Fetch-Fehler degradiert auf None");
        let none = cached_craft_detail_inner(&cache, TEST_URL, "gibt es nicht")
            .await
            .unwrap();
        assert!(none.is_none());
    }

    /// Offlineroute: leerer Cache + Live-Fetch unmöglich →
    /// `error.scwiki.unreachable`-Key (kein Panic, kein Klartext).
    #[tokio::test]
    async fn offline_without_cache_maps_to_i18n_key() {
        let cache = cache(); // nichts geseedet → Live-Fetch nötig
        let err = cached_craftable_items_at(&cache, TEST_URL)
            .await
            .unwrap_err();
        assert!(err.contains("error.scwiki.unreachable"), "{err}");
    }

    /// Stale-Cache-Rettung: kaputter Live-Fetch mit vorhandenem stale
    /// Datensatz → stale Items statt Fehler (Offline-Toleranz).
    #[tokio::test]
    async fn stale_cache_survives_failed_live_fetch() {
        let cache = cache();
        seed(&cache, &[item("Legacy Item")], false); // fetched_at = 0 → stale
        let out = cached_craftable_items_at(&cache, TEST_URL).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "Legacy Item");
    }

    #[test]
    fn scwiki_ready_tracks_cache_presence() {
        let cache = cache();
        assert!(!scwiki_ready(&cache));
        seed(&cache, &[item("x")], false); // stale zählt als vorhanden
        assert!(scwiki_ready(&cache));
    }

    // ---------- M8-R2: SCMDB-Quality-Snaps ----------

    fn mineable(
        name: &str,
        material: Option<&str>,
        bands: &[u32],
    ) -> scwiki_client::MineableElement {
        scwiki_client::MineableElement {
            name: name.to_owned(),
            material_name: material.map(str::to_owned),
            quality_bands: bands.to_vec(),
        }
    }

    fn seed_mining(cache: &Cache, els: &[MineableElement], fresh: bool) {
        let fetched_at = if fresh { now_unix() } else { 0 };
        cache
            .route_meta_set(
                KEY_SCMDB_MINING,
                fetched_at,
                &serde_json::to_string(&MiningIndex::build(els)).unwrap(),
            )
            .unwrap();
    }

    fn req(uuid: &str, name: Option<&str>) -> (String, Option<String>) {
        (uuid.to_owned(), name.map(str::to_owned))
    }

    /// Frischer Cache → Live-Fetch unnötig; Spec-Beleg Taranite (Raw).
    #[tokio::test]
    async fn quality_snaps_read_fresh_cache_without_live_fetch() {
        let cache = cache();
        seed_mining(
            &cache,
            &[mineable(
                "Taranite (Raw)",
                Some("Taranite"),
                &[310, 525, 646, 718, 853, 925, 957, 1000],
            )],
            true,
        );
        let out = quality_snaps_inner(
            &cache,
            &[req("uuid-taranite", Some("Taranite (Raw)"))],
            TEST_URL,
        )
        .await
        .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].uuid, "uuid-taranite");
        assert_eq!(out[0].values, vec![310, 525, 646, 718, 853, 925, 957, 1000]);
    }

    /// Normalize-Matches: Case/Space egal, (Ore)-Mapping über materialName.
    #[tokio::test]
    async fn quality_snaps_match_material_name_form() {
        let cache = cache();
        seed_mining(
            &cache,
            &[mineable("Borase (Ore)", Some("Borase"), &[274, 526, 1000])],
            true,
        );
        let out = quality_snaps_inner(
            &cache,
            &[
                req("u1", Some("borase (ore)")),
                req("u2", Some("BORASE")),
                req("u3", Some("  Borase  ")),
            ],
            TEST_URL,
        )
        .await
        .unwrap();
        for snap in &out {
            assert_eq!(snap.values, vec![274, 526, 1000], "{}", snap.uuid);
        }
    }

    /// Kein Match → LEERE Liste (UI rastet auf 500-only, 0.5.88-Verhalten
    /// „frei bei leer" zurückgenommen).
    #[tokio::test]
    async fn quality_snaps_empty_without_match() {
        let cache = cache();
        seed_mining(
            &cache,
            &[mineable("Hadanite", None, &[274, 526, 1000])],
            true,
        );
        let out = quality_snaps_inner(
            &cache,
            &[req("u1", Some("Unobtainium")), req("u2", Some(""))],
            TEST_URL,
        )
        .await
        .unwrap();
        assert!(out[0].values.is_empty());
        assert!(out[1].values.is_empty(), "leerer Name → leer");
    }

    /// Fetch-Fehler (unreachbare SCMDB-URL) → alle Zeilen ohne Punkte
    /// (Bereicherung degradiert, kein Hard-Fail).
    #[tokio::test]
    async fn quality_snaps_degrade_to_empty_on_fetch_error() {
        let cache = cache();
        let out = quality_snaps_inner(
            &cache,
            &[req("u1", Some("Taranite")), req("", None)],
            TEST_URL,
        )
        .await
        .unwrap();
        assert_eq!(out.len(), 1, "leere UUID wird übersprungen");
        assert!(out[0].values.is_empty());
    }

    /// Stale Mining-Index rettet den Fetch-Fehler (Offline-Toleranz).
    #[tokio::test]
    async fn quality_snaps_use_stale_index_when_live_fails() {
        let cache = cache();
        seed_mining(
            &cache,
            &[mineable(
                "Agricium (Ore)",
                Some("Agricium"),
                &[346, 588, 1000],
            )],
            false,
        );
        let out = quality_snaps_inner(&cache, &[req("u1", Some("Agricium"))], TEST_URL)
            .await
            .unwrap();
        assert_eq!(out[0].values, vec![346, 588, 1000]);
    }

    /// Carinite-Dup-Case: identische Bänder kollabieren, gemischte union.
    #[tokio::test]
    async fn quality_snaps_union_duplicate_keys() {
        let cache = cache();
        seed_mining(
            &cache,
            &[
                mineable("Carinite", None, &[273, 554, 1000]),
                mineable("Carinite (Pure)", None, &[273, 647, 1000]),
            ],
            true,
        );
        let out = quality_snaps_inner(&cache, &[req("u1", Some("Carinite"))], TEST_URL)
            .await
            .unwrap();
        assert_eq!(out[0].values, vec![273, 554, 647, 1000]);
    }

    /// Testbare Kernroute von `scwiki_craft_detail` (ohne Tauri-State).
    async fn cached_craft_detail_inner(
        cache: &Cache,
        base_url: &str,
        name: &str,
    ) -> Result<Option<CraftDetailResponse>, String> {
        let items = cached_craftable_items_at(cache, base_url).await?;
        let index = StrataIndex::build(items.iter().map(|it| it.name.as_str()));
        let matched = items.into_iter().find(|it| {
            index
                .find(&it.name)
                .is_some_and(|strata| strata.eq_ignore_ascii_case(name))
                || normalize_name(&it.name) == normalize_name(name)
        });
        let Some(item) = matched else {
            return Ok(None);
        };
        let blueprint = match item.blueprint_uuids.first() {
            Some(uuid) => cached_blueprint_at(cache, base_url, uuid).await.ok(),
            None => None,
        };
        Ok(Some(CraftDetailResponse {
            item,
            blueprint,
            attribution: ATTRIBUTION_SCWIKI.to_owned(),
        }))
    }
}

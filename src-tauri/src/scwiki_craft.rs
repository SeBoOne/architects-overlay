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
    ATTRIBUTION_SCWIKI, Blueprint, CommodityQuality, CommodityRef, CraftableItem, ScWikiClient,
    StrataIndex, normalize_name,
};
use serde::Serialize;
use serde_json::json;

use crate::i18n_err::{err_key, err_keyp};

/// Route-Meta-Key der gecachten craftable-Items-Liste (EIN Datensatz —
/// Suchfilter greift client-seitig, kein zweiter Cache pro Query).
const KEY_ITEMS: &str = "scwiki:craftable_items";
/// Prefix der Blueprint-Cache-Keys (`scwiki:blueprint:{uuid}`).
const KEY_BLUEPRINT_PREFIX: &str = "scwiki:blueprint:";
/// Prefix der Commodity-Snap-Keys (`scwiki:commodity:{uuid}`, M8-R1-A).
const KEY_COMMODITY_PREFIX: &str = "scwiki:commodity:";
/// Route-Meta-Key der gecachten Commodities-Liste (EIN Datensatz, 206
/// Einträge / 7 Seiten — LIVE-Beleg 2026-11-19; Basis des `(Ore)`-
/// Name-Mappings im Snap-Fallback, t_1dd162e3).
const KEY_COMMODITIES: &str = "scwiki:commodities";

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

/// Snap-Punkte eines Materials (M8-R1-A) — leere Liste = keine Daten
/// (unbekannte UUID, Fetch-Fehler) → UI degradiert (Toggle ohne Punkte).
#[derive(Debug, Clone, Serialize)]
pub struct QualitySnap {
    pub uuid: String,
    pub values: Vec<u32>,
}

/// Qualitäts-Snap-Punkte für mehrere Commodities (`/api/commodities/{uuid}`,
/// M8-R1-A) — cache-first pro UUID, TTL [`TTL_SCWIKI_CRAFT`]. Fehler pro
/// UUID degradieren auf eine leere Liste (Bereicherung, kein Hard-Fail);
/// die UI zeigt den Regler dann ohne Einrastpunkte weiter an.
///
/// Snap-Fallback Raffinat → Erz (t_1dd162e3): Blueprint-Inputs tragen die
/// Raffinat-Commodity-UUID (`resource_type_uuid`, z. B. Borase 33bff393 —
/// 0 locations → 0 quantisierte Werte → Snap tot). Die Werte liegen an den
/// `(Ore)`-Commodities (z. B. „Borase (Ore)" 728a3d7e, 39 locations).
/// Leere Werte am Input-UUID → Fallback:
///   1. Name-Mapping `<name>` → `<name> (Ore)` über die gecachten
///      Commodities-Liste (primär),
///   2. `dismantle_returns[].resource_type_uuid` des Blueprints, per
///      Materialname gematcht (sekundär).
/// Kein Raten: bleibt auch der (Ore)-Match leer → keine Snap-Punkte.
#[tauri::command]
pub async fn scwiki_quality_snaps(
    state: tauri::State<'_, ScWikiCraftState>,
    uuids: Vec<String>,
    names: Option<Vec<String>>,
    blueprint_uuid: Option<String>,
) -> Result<Vec<QualitySnap>, String> {
    let names = names.unwrap_or_default();
    let requests: Vec<(String, Option<String>)> = uuids
        .iter()
        .enumerate()
        .map(|(i, u)| (u.clone(), names.get(i).cloned()))
        .collect();
    quality_snaps_inner(
        &state.cache,
        &requests,
        blueprint_uuid.as_deref(),
        scwiki_client::DEFAULT_BASE_URL,
    )
    .await
}

/// (uuid, Materialname)-Paar → Snap-Punkte; `names[i]` gehört zu
/// `uuids[i]` (Frontend baut beide aus derselben Aspekt-Liste).
async fn quality_snaps_inner(
    cache: &Cache,
    requests: &[(String, Option<String>)],
    blueprint_uuid: Option<&str>,
    base_url: &str,
) -> Result<Vec<QualitySnap>, String> {
    let mut out = Vec::new();
    for (uuid, name) in requests {
        let uuid = uuid.trim();
        if uuid.is_empty() {
            continue;
        }
        let mut values = cached_commodity_quality_at(cache, base_url, uuid)
            .await
            .map(|c| c.quality_quantized_values)
            .unwrap_or_else(|e| {
                log::warn!("scwiki-craft: commodity {uuid}: {e} — keine Snap-Punkte");
                Vec::new()
            });
        if values.is_empty() {
            values =
                snap_fallback_ore(cache, base_url, uuid, name.as_deref(), blueprint_uuid).await;
        }
        out.push(QualitySnap {
            uuid: uuid.to_owned(),
            values,
        });
    }
    Ok(out)
}

/// Snap-Fallback Raffinat → `(Ore)`-Commodity (t_1dd162e3): erst
/// Name-Mapping (`<name> (Ore)`) über die gecachte Commodities-Liste,
/// dann — falls der (Ore)-Match leer bleibt — der Blueprint: per
/// Materialname gematcht zuerst `ingredients[].link`-UUID (SC-Wiki-eigener
/// Zeiger aufs Vorkommens-Commodity, LIVE-Beleg), sonst
/// `dismantle_returns[].resource_type_uuid`. Gibt NUR belegte Werte
/// zurück; bleibt alles leer → leere Liste (kein Fake-Raster).
async fn snap_fallback_ore(
    cache: &Cache,
    base_url: &str,
    uuid: &str,
    name: Option<&str>,
    blueprint_uuid: Option<&str>,
) -> Vec<u32> {
    let name = match name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => n,
        None => {
            log::warn!(
                "scwiki-craft: commodity {uuid}: ohne Werte, aber ohne Materialnamen — kein Fallback"
            );
            return Vec::new();
        }
    };
    // 1) Name-Mapping: `<name> (Ore)` in der gecachten Commodities-Liste.
    let needle = format!("{} (ore)", normalize_name(name));
    let list = match cached_commodities(cache, base_url).await {
        Ok(list) => Some(list),
        Err(e) => {
            log::warn!(
                "scwiki-craft: commodity {uuid}: Commodities-Liste: {e} — kein Name-Mapping"
            );
            None
        }
    };
    if let Some(ore) = list
        .as_ref()
        .and_then(|l| l.iter().find(|c| normalize_name(&c.name) == needle))
    {
        match cached_commodity_quality_at(cache, base_url, &ore.uuid).await {
            Ok(cq) if !cq.quality_quantized_values.is_empty() => {
                log::info!(
                    "scwiki-craft: commodity {uuid}: Fallback auf {} ({}) — {} Snap-Punkte",
                    cq.name,
                    cq.uuid,
                    cq.quality_quantized_values.len()
                );
                return cq.quality_quantized_values;
            }
            Ok(_) => {
                log::warn!(
                    "scwiki-craft: commodity {uuid}: (Ore)-Match {} ohne Werte — weiter",
                    ore.uuid
                );
            }
            Err(e) => {
                log::warn!(
                    "scwiki-craft: commodity {uuid}: (Ore)-Fetch {}: {e}",
                    ore.uuid
                );
            }
        }
    }
    // 2) Blueprint-Anker: SC-Wiki-eigener Zeiger — ingredients[].link
    //    zeigt LIVE auf das Vorkommens-Commodity („Borase (Ore)",
    //    Blueprint SW16BR1 „Buzzsaw", t_1dd162e3). Per Materialname
    //    gematcht; dismantle_returns[].resource_type_uuid (zeigt live auf
    //    das Raffinat, damit wirkungslos) als letzter Versuch.
    if let Some(bp_uuid) = blueprint_uuid.map(str::trim).filter(|u| !u.is_empty())
        && let Ok(bp) = cached_blueprint(cache, bp_uuid).await
        && let Some(anchor) = bp
            .ingredients
            .iter()
            .find(|i| normalize_name(&i.name) == normalize_name(name))
            .and_then(|i| i.commodity_uuid.as_deref())
            .filter(|u| !u.trim().is_empty())
            .map(str::to_owned)
            .or_else(|| {
                bp.dismantle_returns
                    .iter()
                    .find(|d| normalize_name(&d.name) == normalize_name(name))
                    .and_then(|d| d.resource_type_uuid.as_deref())
                    .filter(|u| !u.trim().is_empty())
                    .map(str::to_owned)
            })
    {
        match cached_commodity_quality_at(cache, base_url, &anchor).await {
            Ok(cq) if !cq.quality_quantized_values.is_empty() => {
                log::info!(
                    "scwiki-craft: commodity {uuid}: Blueprint-Fallback auf {} ({}) — {} Snap-Punkte",
                    cq.name,
                    cq.uuid,
                    cq.quality_quantized_values.len()
                );
                return cq.quality_quantized_values;
            }
            Ok(_) => {
                log::warn!(
                    "scwiki-craft: commodity {uuid}: Blueprint-Fallback {anchor} ohne Werte"
                );
            }
            Err(e) => {
                log::warn!("scwiki-craft: commodity {uuid}: Blueprint-Fetch {anchor}: {e}");
            }
        }
    }
    log::warn!(
        "scwiki-craft: commodity {uuid}: kein (Ore)-Match ({name}) — keine Snap-Punkte (kein Raten)"
    );
    Vec::new()
}

/// Commodities-Liste: frisch aus dem Cache, sonst Live-Fetch (Upsert,
/// EIN Datensatz). Fetch-Fehler mit vorhandenem stale Cache → stale.
async fn cached_commodities(cache: &Cache, base_url: &str) -> Result<Vec<CommodityRef>, String> {
    if let Some((fetched_at, payload)) = cache.route_meta_get(KEY_COMMODITIES).ok().flatten()
        && now_unix().saturating_sub(fetched_at) < TTL_SCWIKI_CRAFT.as_secs()
    {
        return serde_json::from_str(&payload)
            .map_err(|e| err_keyp("error.scwiki.bad_response", json!({ "msg": e.to_string() })));
    }
    let client = scwiki_client_at(base_url)
        .map_err(|e| err_keyp("error.scwiki.bad_response", json!({ "msg": e })))?;
    match client.commodities().await {
        Ok(list) => {
            if let Ok(payload) = serde_json::to_string(&list)
                && let Err(e) = cache.route_meta_set(KEY_COMMODITIES, now_unix(), &payload)
            {
                log::warn!("scwiki-craft: cache write {KEY_COMMODITIES}: {e}");
            }
            Ok(list)
        }
        Err(e) => {
            if let Ok(Some((_, payload))) = cache.route_meta_get(KEY_COMMODITIES)
                && let Ok(list) = serde_json::from_str::<Vec<CommodityRef>>(&payload)
            {
                log::warn!("scwiki-craft: commodities live fehlgeschlagen ({e}) — stale Cache");
                return Ok(list);
            }
            Err(scwiki_error_key(&e))
        }
    }
}

/// Commodity-Snap-Punkte: frisch aus dem Cache, sonst Live-Fetch (Upsert).
async fn cached_commodity_quality_at(
    cache: &Cache,
    base_url: &str,
    uuid: &str,
) -> Result<CommodityQuality, String> {
    let key = format!("{KEY_COMMODITY_PREFIX}{uuid}");
    if let Some((fetched_at, payload)) = cache.route_meta_get(&key).ok().flatten()
        && now_unix().saturating_sub(fetched_at) < TTL_SCWIKI_CRAFT.as_secs()
    {
        return serde_json::from_str(&payload)
            .map_err(|e| err_keyp("error.scwiki.bad_response", json!({ "msg": e.to_string() })));
    }
    let client = scwiki_client_at(base_url)
        .map_err(|e| err_keyp("error.scwiki.bad_response", json!({ "msg": e })))?;
    match client.commodity_quality(uuid).await {
        Ok(cq) => {
            if let Ok(payload) = serde_json::to_string(&cq)
                && let Err(e) = cache.route_meta_set(&key, now_unix(), &payload)
            {
                log::warn!("scwiki-craft: cache write {key}: {e}");
            }
            Ok(cq)
        }
        Err(e) => {
            if let Ok(Some((_, payload))) = cache.route_meta_get(&key)
                && let Ok(cq) = serde_json::from_str::<CommodityQuality>(&payload)
            {
                log::warn!("scwiki-craft: commodity {uuid} live fehlgeschlagen ({e}) — stale");
                return Ok(cq);
            }
            Err(scwiki_error_key(&e))
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
    use scwiki_client::{DismantleReturn, Ingredient};

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

    // ---------- M8-R1-A: Quality-Snaps ----------

    fn commodity(uuid: &str, values: &[u32]) -> CommodityQuality {
        CommodityQuality {
            uuid: uuid.to_owned(),
            name: "Iron".to_owned(),
            quality_quantized_values: values.to_vec(),
            occurrences: vec![values.to_vec()],
        }
    }

    fn seed_commodity(cache: &Cache, cq: &CommodityQuality, fresh: bool) {
        let fetched_at = if fresh { now_unix() } else { 0 };
        cache
            .route_meta_set(
                &format!("{KEY_COMMODITY_PREFIX}{}", cq.uuid),
                fetched_at,
                &serde_json::to_string(cq).unwrap(),
            )
            .unwrap();
    }

    fn seed_commodities_list(cache: &Cache, list: &[CommodityRef], fresh: bool) {
        let fetched_at = if fresh { now_unix() } else { 0 };
        cache
            .route_meta_set(
                KEY_COMMODITIES,
                fetched_at,
                &serde_json::to_string(list).unwrap(),
            )
            .unwrap();
    }

    fn req(uuid: &str, name: Option<&str>) -> (String, Option<String>) {
        (uuid.to_owned(), name.map(str::to_owned))
    }

    #[tokio::test]
    async fn quality_snaps_read_fresh_cache_without_live_fetch() {
        let cache = cache();
        seed_commodity(&cache, &commodity("uuid-iron", &[325, 521, 1000]), true);
        let out = quality_snaps_inner(&cache, &[req("uuid-iron", Some("Iron"))], None, TEST_URL)
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].uuid, "uuid-iron");
        assert_eq!(out[0].values, vec![325, 521, 1000]);
    }

    /// Unbekannte UUID (nur Live-Fetch möglich, der ins Leere läuft) →
    /// leere Snap-Liste statt Fehler (Bereicherung degradiert).
    #[tokio::test]
    async fn quality_snaps_degrade_to_empty_on_fetch_error() {
        let cache = cache(); // nichts geseedet → Live-Fetch nötig
        let out = quality_snaps_inner(&cache, &[req("fehlt", None), req("", None)], None, TEST_URL)
            .await
            .unwrap();
        assert_eq!(out.len(), 1, "leere UUID wird übersprungen");
        assert!(out[0].values.is_empty());
    }

    /// Stale Commodity-Cache rettet den Fetch-Fehler (Offline-Toleranz).
    #[tokio::test]
    async fn quality_snaps_use_stale_cache_when_live_fails() {
        let cache = cache();
        seed_commodity(&cache, &commodity("uuid-iron", &[346, 588]), false);
        let out = quality_snaps_inner(&cache, &[req("uuid-iron", None)], None, TEST_URL)
            .await
            .unwrap();
        assert_eq!(out[0].values, vec![346, 588]);
    }

    /// Direkter Treffer schlägt den Fallback: uuid-iron liefert eigene
    /// Werte, selbst wenn Commodities-Liste + Blueprint einen (Ore)-Anker
    /// mit ANDEREN Werten anbieten würden (t_1dd162e3).
    #[tokio::test]
    async fn direct_commodity_hit_beats_ore_fallback() {
        let cache = cache();
        seed_commodity(&cache, &commodity("uuid-iron", &[100]), true);
        seed_commodities_list(
            &cache,
            &[CommodityRef {
                uuid: "uuid-iron-ore".to_owned(),
                name: "Iron (Ore)".to_owned(),
            }],
            true,
        );
        seed_commodity(&cache, &commodity("uuid-iron-ore", &[999]), true);
        let out = quality_snaps_inner(&cache, &[req("uuid-iron", Some("Iron"))], None, TEST_URL)
            .await
            .unwrap();
        assert_eq!(
            out[0].values,
            vec![100],
            "kein Fallback bei direktem Treffer"
        );
    }

    // ---------- t_1dd162e3: Snap-Fallback Raffinat → (Ore) ----------

    /// LIVE-Beleg Omar (t_1dd162e3): Borase 33bff393 (Raffinat) hat 0
    /// locations → 0 Werte; die Union liegt an „Borase (Ore)" 728a3d7e.
    /// Probehafte Wiederholung mit Fixtures: Name-Mapping findet den
    /// (Ore)-Commodity in der gecachten Liste und übernimmt seine Union.
    const BORASE_UNION: [u32; 21] = [
        264, 302, 359, 553, 584, 597, 632, 644, 676, 743, 756, 786, 864, 875, 892, 903, 916, 941,
        959, 976, 1000,
    ];

    #[tokio::test]
    async fn snap_fallback_resolves_ore_via_commodity_name_mapping() {
        let cache = cache();
        // Raffinat-UUID bewusst NICHT geseedet → Live-Fetch (TEST_URL)
        // scheitert → Werte leer → Fallback greift.
        seed_commodities_list(
            &cache,
            &[
                CommodityRef {
                    uuid: "33bff393-raffinat".to_owned(),
                    name: "Borase".to_owned(),
                },
                CommodityRef {
                    uuid: "uuid-ore-borase".to_owned(),
                    name: "Borase (Ore)".to_owned(),
                },
            ],
            true,
        );
        seed_commodity(&cache, &commodity("uuid-ore-borase", &BORASE_UNION), true);
        let out = quality_snaps_inner(
            &cache,
            &[req("33bff393-raffinat", Some("Borase"))],
            None,
            TEST_URL,
        )
        .await
        .unwrap();
        assert_eq!(out[0].uuid, "33bff393-raffinat");
        assert_eq!(
            out[0].values,
            BORASE_UNION.to_vec(),
            "Snap auf Borase-(Ore)-Union gerastet"
        );
    }

    /// Kein Raten: Material ohne `(Ore)`-Gegenstück (Commodities-Liste
    /// ohne Match, kein Blueprint) → leere Snap-Liste, kein Fake-Raster.
    #[tokio::test]
    async fn snap_fallback_stays_empty_without_ore_match() {
        let cache = cache();
        seed_commodities_list(
            &cache,
            &[CommodityRef {
                uuid: "uuid-x".to_owned(),
                name: "Something Else".to_owned(),
            }],
            true,
        );
        let out = quality_snaps_inner(
            &cache,
            &[req("uuid-raffinat", Some("Quxdust"))],
            None,
            TEST_URL,
        )
        .await
        .unwrap();
        assert!(out[0].values.is_empty());
    }

    /// (Ore)-Match vorhanden, aber ohne Werte (z. B. Ore_Jaclium, 0
    /// locations) → KEIN Fake-Raster; ohne Blueprint-Anker bleibt es leer.
    #[tokio::test]
    async fn snap_fallback_does_not_fake_values_on_empty_ore() {
        let cache = cache();
        seed_commodities_list(
            &cache,
            &[CommodityRef {
                uuid: "uuid-ore-empty".to_owned(),
                name: "Jaclium (Ore)".to_owned(),
            }],
            true,
        );
        seed_commodity(&cache, &commodity("uuid-ore-empty", &[]), true);
        let out = quality_snaps_inner(
            &cache,
            &[req("uuid-raffinat", Some("Jaclium"))],
            None,
            TEST_URL,
        )
        .await
        .unwrap();
        assert!(out[0].values.is_empty());
    }

    /// Sekundärer Anker: Name-Mapping ohne (Ore)-Eintrag, aber Blueprint
    /// mit `ingredients[].commodity_uuid` (aus link geparsed — LIVE-Beleg
    /// SW16BR1 „Buzzsaw": Borase-Ingredient link → 728a3d7e = „Borase
    /// (Ore)") → dessen Union als Snap-Punkte.
    #[tokio::test]
    async fn snap_fallback_via_blueprint_ingredient_link() {
        let cache = cache();
        seed_commodities_list(
            &cache,
            &[CommodityRef {
                uuid: "uuid-x".to_owned(),
                name: "Something Else".to_owned(),
            }],
            true,
        );
        let bp = Blueprint {
            uuid: "bp-1".to_owned(),
            output_name: "Qux Device".to_owned(),
            craft_time_seconds: None,
            craft_time_label: None,
            is_available_by_default: false,
            game_version: None,
            ingredients: vec![Ingredient {
                name: "Quxdust".to_owned(),
                kind: "resource".to_owned(),
                quantity_scu: Some(0.36),
                commodity_uuid: Some("uuid-ore-quxdust".to_owned()),
            }],
            dismantle_returns: Vec::new(),
            tiers: Vec::new(),
            summary_properties: Vec::new(),
            aspects: Vec::new(),
            unlocking_missions_grouped: Vec::new(),
            web_url: None,
        };
        cache
            .route_meta_set(
                &format!("{KEY_BLUEPRINT_PREFIX}bp-1"),
                now_unix(),
                &serde_json::to_string(&bp).unwrap(),
            )
            .unwrap();
        seed_commodity(&cache, &commodity("uuid-ore-quxdust", &[500, 1000]), true);
        let out = quality_snaps_inner(
            &cache,
            &[req("uuid-raffinat", Some("Quxdust"))],
            Some("bp-1"),
            TEST_URL,
        )
        .await
        .unwrap();
        assert_eq!(out[0].values, vec![500, 1000]);
    }

    /// Tertiärer Anker: weder (Ore)-Eintrag noch Ingredient-Link, aber
    /// `dismantle_returns[].resource_type_uuid` → dessen Werte (LIVE-
    /// Beleg zeigt:UUIDs aufs Raffinat — dann degradiert das korrekt leer).
    #[tokio::test]
    async fn snap_fallback_via_blueprint_dismantle_return() {
        let cache = cache();
        seed_commodities_list(
            &cache,
            &[CommodityRef {
                uuid: "uuid-x".to_owned(),
                name: "Something Else".to_owned(),
            }],
            true,
        );
        let bp = Blueprint {
            uuid: "bp-1".to_owned(),
            output_name: "Qux Device".to_owned(),
            craft_time_seconds: None,
            craft_time_label: None,
            is_available_by_default: false,
            game_version: None,
            ingredients: vec![Ingredient {
                name: "Quxdust".to_owned(),
                kind: "resource".to_owned(),
                quantity_scu: Some(0.36),
                commodity_uuid: None,
            }],
            dismantle_returns: vec![DismantleReturn {
                name: "Quxdust".to_owned(),
                quantity_scu: Some(0.2),
                resource_type_uuid: Some("uuid-ore-quxdust".to_owned()),
            }],
            tiers: Vec::new(),
            summary_properties: Vec::new(),
            aspects: Vec::new(),
            unlocking_missions_grouped: Vec::new(),
            web_url: None,
        };
        cache
            .route_meta_set(
                &format!("{KEY_BLUEPRINT_PREFIX}bp-1"),
                now_unix(),
                &serde_json::to_string(&bp).unwrap(),
            )
            .unwrap();
        seed_commodity(&cache, &commodity("uuid-ore-quxdust", &[500, 1000]), true);
        let out = quality_snaps_inner(
            &cache,
            &[req("uuid-raffinat", Some("Quxdust"))],
            Some("bp-1"),
            TEST_URL,
        )
        .await
        .unwrap();
        assert_eq!(out[0].values, vec![500, 1000]);
    }

    /// Kein Raten II: Blueprint-Anker (dismantle_returns) zeigt aufs
    /// RAFFINAT (Live-Befund) mit 0 Werten → leere Liste, kein Fake-Raster.
    #[tokio::test]
    async fn snap_fallback_does_not_fake_values_on_refined_dismantle_anchor() {
        let cache = cache();
        seed_commodities_list(
            &cache,
            &[CommodityRef {
                uuid: "uuid-x".to_owned(),
                name: "Something Else".to_owned(),
            }],
            true,
        );
        let bp = Blueprint {
            uuid: "bp-1".to_owned(),
            output_name: "Qux Device".to_owned(),
            craft_time_seconds: None,
            craft_time_label: None,
            is_available_by_default: false,
            game_version: None,
            ingredients: vec![Ingredient {
                name: "Quxdust".to_owned(),
                kind: "resource".to_owned(),
                quantity_scu: Some(0.36),
                commodity_uuid: Some("uuid-raffinat".to_owned()), // = angefragte UUID
            }],
            dismantle_returns: vec![DismantleReturn {
                name: "Quxdust".to_owned(),
                quantity_scu: Some(0.2),
                resource_type_uuid: Some("uuid-raffinat".to_owned()),
            }],
            tiers: Vec::new(),
            summary_properties: Vec::new(),
            aspects: Vec::new(),
            unlocking_missions_grouped: Vec::new(),
            web_url: None,
        };
        cache
            .route_meta_set(
                &format!("{KEY_BLUEPRINT_PREFIX}bp-1"),
                now_unix(),
                &serde_json::to_string(&bp).unwrap(),
            )
            .unwrap();
        let out = quality_snaps_inner(
            &cache,
            &[req("uuid-raffinat", Some("Quxdust"))],
            Some("bp-1"),
            TEST_URL,
        )
        .await
        .unwrap();
        assert!(out[0].values.is_empty());
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

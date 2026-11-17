//! DTOs der Strata Public API — CONTRACT-REALITÄT: die Live-Fixtures
//! (`tests/fixtures-live/`, echte celd.space-Responses 2026-11-17) sind der
//! Contract, NICHT das OpenAPI-Spec-Dokument (strata.celd.space/strata-api.md
//! — STALE, Beispiel-Werte weichen live ab, siehe Feld-Doku).
//! Kein Envelope: die Strata-API antwortet direkt mit dem Objekt (anders als
//! UEX).
//!
//! Null-Disziplin (Spec, verbindlich): `tier`, `scanSignature`,
//! `instability`, `resistance` und `category` sind `null`-fähig → `Option`.
//! **NIEMALS truthiness** — `0.0` (instability) / `0` (scanSignature) sind
//! gültige Werte, kein „fehlend".
//!
//! Live-Abweichungen vom (stalen) Spec — M7-F1, am echten Key verifiziert:
//! - `ores[].tier` ist String-Rarity (`common`/`uncommon`/`rare`/`epic`/
//!   `legendary`, null-fähig) — der Spec zeigt eine i64-Zahl, die live NIE
//!   vorkommt. (Die Tier-Zahl 1–6 existiert nur in `ore-locations`/
//!   `location-ores` — dort ist der Spec korrekt.)
//! - `ores[].instability`/`resistance` sind unbeschränkte f64 (live: 350!
//!   bzw. negativ) — die Spec-Range 0.0–1.0 ist falsch. KEINE Clamp-Logik:
//!   die Werte werden unverändert durchgereicht.
//! - `locations[].system` kann `null` sein (Dungeons ohne System-Zuordnung)
//!   und `type` umfasst mehr Werte als die Spec-Beispiele (asteroid_field,
//!   asteroid_cluster, station, lagrange_point, moon, planet, dungeon, …) —
//!   system → `Option<String>`, type bleibt freier String; die UI leitet
//!   ihre Filter dynamisch aus den Daten ab.

use std::collections::BTreeMap;

/// `GET /api/public/ores` — alle Erze, alphabetisch sortiert.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ore {
    /// Kanonische, exakt matchende ID (z. B. `beryl_raw`) — für Lookups IMMER
    /// die ID, nicht den Slug (Slugs kollidieren).
    pub id: String,
    pub name: String,
    /// Best-effort-Slug — Anzeige/Suche, kein kanonischer Key.
    pub slug: String,
    /// `ship` / `fps` / `ground_vehicle` — null-fähig laut Spec.
    #[serde(default)]
    pub category: Option<String>,
    /// String-Rarity aus den Game-Daten — live: `common`/`uncommon`/`rare`/
    /// `epic`/`legendary`, null-fähig. **ACHTUNG: das OpenAPI-Spec zeigt hier
    /// eine i64-Zahl — das ist STALE** (M7-F1: live kommt NIE eine Zahl,
    /// s. Fixtures-live). Die Tier-Zahl 1–6 existiert nur in `ore-locations`
    /// / `location-ores` (`LocationRow.tier`/`OreRow.tier`).
    #[serde(default)]
    pub tier: Option<String>,
    /// Scan-Signatur (höher = früher entdeckt). 0 ist gültige Daten.
    #[serde(default)]
    pub scan_signature: Option<i64>,
    /// UNBESCHRÄNKTES f64 (live: 350! — die Spec-Range 0.0–1.0 ist STALE,
    /// M7-F1). 0.0 ist gültige Daten — null-check, nie truthiness. KEIN
    /// Clamp: die Werte werden unverändert durchgereicht.
    #[serde(default)]
    pub instability: Option<f64>,
    /// UNBESCHRÄNKTES f64 (live auch negativ — Spec-Range STALE, M7-F1).
    /// 0.0 ist gültige Daten, kein Clamp.
    #[serde(default)]
    pub resistance: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OresResponse {
    pub ores: Vec<Ore>,
    pub count: u64,
    /// Pflichtanzeige in der UI bei JEGLICHER öffentlichen Anzeige.
    pub attribution: String,
}

/// `GET /api/public/locations` — alle Orte mit mindestens einem Ore-Spawn.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Location {
    pub id: String,
    pub name: String,
    pub slug: String,
    /// System-String — live KANN der Wert `null` sein (Dungeon-Spawns ohne
    /// System-Zuordnung; M7-F1, Fixtures-live). `Option` + UI-Filter dynamisch
    /// aus den Daten (kein hartkodiertes Spec-Vokabular).
    #[serde(default)]
    pub system: Option<String>,
    /// Game-Daten-Typ (`belt`, `moon`, `dungeon`, `exploration_region`,
    /// `asteroid_field`, `asteroid_cluster`, `station`, `lagrange_point`,
    /// `planet`, … — die Spec-Beispielliste ist unvollständig). Freier
    /// String; UI-Filter dynamisch aus den Daten. JSON-Feld heißt `type`
    /// (Rust-Keyword → `kind`).
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LocationsResponse {
    pub locations: Vec<Location>,
    pub count: u64,
    pub attribution: String,
}

/// `{ore: {id, name}}`-Kopf von `ore-locations/{ore}`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OreRef {
    pub id: String,
    pub name: String,
}

/// `{location: {id, name, system, type}}`-Kopf von `location-ores/{location}`.
/// system `Option` wie `Location.system` (live null-fähig, M7-F1).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LocationRef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub system: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
}

/// Eine Ergebniszeile in `ore-locations/{ore}` — je Gruppe sortiert.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationRow {
    pub location_id: String,
    pub location: String,
    pub system: String,
    /// 0–100, NUR innerhalb der eigenen Gruppe vergleichbar.
    pub rank_pct: i64,
    /// Wettbewerbs-Rang innerhalb der Gruppe (1 = best, Ties gleicher Rang).
    pub rank: i64,
    /// Orte in dieser Gruppe mit irgendeinem Spawn des Erzes.
    pub n: i64,
    /// Anteil des Erzes an der Mining-Präsenz des Ortes, 0.0–1.0.
    pub concentration: f64,
    /// Absolute Konzentrations-Band 1–6 (Negligible→Pure), 0 (None) taucht
    /// nur in der Legende auf.
    pub tier: i64,
    pub tier_name: String,
}

/// Die vier Ergebnis-Gruppen von `ore-locations/{ore}`. Fehlende Gruppe →
/// leeres Vec (`serde(default)` — die API kann Gruppen weglassen).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OreLocationGroups {
    #[serde(default)]
    pub space: Vec<LocationRow>,
    #[serde(default)]
    pub surface: Vec<LocationRow>,
    #[serde(default)]
    pub cave: Vec<LocationRow>,
    #[serde(default)]
    pub exploration: Vec<LocationRow>,
}

/// `GET /api/public/ore-locations/{ore}?sort=rank|concentration`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OreLocationsResponse {
    pub ore: OreRef,
    /// Echo des Request-Params (`rank` oder `concentration`).
    pub sort: String,
    /// Legende `"0"`.."6"` → Tier-Name (Option: Spec zeigt sie immer, aber
    /// defensiv — fehlt sie, rendert die UI die eingebaute Legende).
    #[serde(default)]
    pub tier_legend: Option<BTreeMap<String, String>>,
    pub results: OreLocationGroups,
    pub attribution: String,
}

/// Eine Erz-Zeile in `location-ores/{location}` — nach Konzentration absteigend.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OreRow {
    pub ore_id: String,
    pub name: String,
    /// 0.0–1.0, 4 Dezimalstellen, über Ortstypen hinweg vergleichbar.
    pub concentration: f64,
    pub tier: i64,
    pub tier_name: String,
    /// null-fähig laut Spec (0 ist gültige Daten).
    #[serde(default)]
    pub scan_signature: Option<i64>,
}

/// `GET /api/public/location-ores/{location}`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LocationOresResponse {
    pub location: LocationRef,
    pub ores: Vec<OreRow>,
    pub count: u64,
    pub attribution: String,
}

//! DTOs strikt nach der Strata-Public-API-Spec (OpenAPI v1.0.0, Stand
//! 2026-11-17 — docs/MINING-MODE-PLAN.md). Kein Envelope: die Strata-API
//! antwortet direkt mit dem Objekt (anders als UEX).
//!
//! Null-Disziplin (Spec, verbindlich): `tier`, `scanSignature`,
//! `instability`, `resistance` und `category` sind `null`-fähig → `Option`.
//! **NIEMALS truthiness** — `0.0` (instability) / `0` (scanSignature) sind
//! gültige Werte, kein „fehlend".

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
    /// Erz-Gruppierung aus den Game-Daten — null-fähig laut Spec.
    #[serde(default)]
    pub tier: Option<i64>,
    /// Scan-Signatur (höher = früher entdeckt). 0 ist gültige Daten.
    #[serde(default)]
    pub scan_signature: Option<i64>,
    /// 0.0–1.0. **0.0 ist gültige Daten** — null-check, nie truthiness.
    #[serde(default)]
    pub instability: Option<f64>,
    /// 0.0–1.0, höher = härteres Gestein. 0.0 ist gültige Daten.
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
    pub system: String,
    /// Game-Daten-Typ (`belt`, `moon`, `dungeon`, `exploration_region`, …).
    /// JSON-Feld heißt `type` (Rust-Keyword → `kind`).
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
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LocationRef {
    pub id: String,
    pub name: String,
    pub system: String,
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

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
    /// Live-Drift (2026-11-18, t_c3367dfb-Smoke): FPS-Cave-Zeilen kommen mit
    /// `system: null` — wie locations[].system null-fähig (M7-F1-Präzedenz).
    #[serde(default)]
    pub system: Option<String>,
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

// ---- Crafting (M8-K1, t_a35b6344) -------------------------------------------
//
// CONTRACT-REALITÄT: Live-Fixtures 2026-11-19 (`tests/fixtures-live/items.*`,
// `craft-cost.*.live.json`, echter Omar-Key). Live-Stand: 735 craftbare Items,
// Kategorien freie Strings (Ship Weapon / FPS Weapon / Shield / Mining Laser /
// Mission Item / …) — KEIN hartkodiertes Vokabular; die UI leitet die Filter
// dynamisch aus den Daten ab (M7-Präzedenz: locations[].type).
//
// **Einheiten-Regel (verbindlich, NIE mischen/vergleichen):**
// Schiff-Erze laufen über `CraftUnit::Scu` (Volumen, SCU),
// FPS/Ground-Vehicle-Erze über `CraftUnit::Gems` (Stück; **1000 gems =
// 1 SCU** — siehe [`CRAFT_GEMS_PER_SCU`]), Nicht-Erze über
// `CraftUnit::Items` (Stück). Eine Menge ist NUR innerhalb ihrer eigenen
// Einheit interpretierbar — Summen über `costs[]` dürfen nie die Einheit
// überschreiten und Vergleichs-UI (Balken/Rankings) niemals SCU gegen gems
// addieren.

/// Einheitenumrechnung gems ↔ SCU: 1000 gems = 1 SCU (Plan M8, API-Realität
/// 2026-11-19). Konstante statt Magie — die Craft-UI nutzt sie für den
/// (nur-planerischen) Einheitenausgleich; im Mining-Modus gilt sie NICHT.
pub const CRAFT_GEMS_PER_SCU: u64 = 1000;

/// Kosteneinheit einer `craft-cost`-Zeile — `SCU` | `gems` | `items`
/// (Wire-Strings exakt so; kein anderes Vokabular in den Live-Fixtures).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CraftUnit {
    /// Volumen-Erze von Schiff-Rezepten.
    #[serde(rename = "SCU")]
    Scu,
    /// FPS/Ground-Vehicle-Erze in Stück (1000 gems = 1 SCU).
    #[serde(rename = "gems")]
    Gems,
    /// Nicht-Erz-Bestandteile in Stück.
    #[serde(rename = "items")]
    Items,
}

impl CraftUnit {
    pub fn as_str(self) -> &'static str {
        match self {
            CraftUnit::Scu => "SCU",
            CraftUnit::Gems => "gems",
            CraftUnit::Items => "items",
        }
    }

    /// true = Volumen-Erz (SCU); gems sind Stück, items sind Stück — nur
    /// innerhalb von `Scu` dürfen Dezimalmengen als Volumen gelesen werden.
    pub fn is_volume(self) -> bool {
        matches!(self, CraftUnit::Scu)
    }
}

/// `GET /api/public/items` — ein craftbares Item. Live (2026-11-19): alle
/// vier Felder IMMER gesetzt (String) — `category` ist trotzdem `Option`:
/// Null-Disziplin wie `Ore.category` (M7-Präzedenz), die UI filtert dynamisch.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Item {
    /// UUID — kanonischer Key für `craft-cost/{item}` (**id vor Slug**:
    /// IDs kollidieren nie; Slugs sind Best-effort-Anzeige-Handle).
    pub id: String,
    pub name: String,
    /// Best-effort-Slug — Anzeige/Suche, kein kanonischer Key (M7-Präzedenz).
    pub slug: String,
    /// Freie Kategorie — `Option` defensiv (siehe oben), UI-Filter dynamisch.
    #[serde(default)]
    pub category: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ItemsResponse {
    pub items: Vec<Item>,
    pub count: u64,
    /// Pflichtanzeige in der UI bei JEGLICHER öffentlichen Anzeige.
    pub attribution: String,
}

/// Item-Kopf von `craft-cost/{item}` — live OHNE `slug` (nur id/name/
/// category, Fixtures 2026-11-19). Absichtlich KEIN Slug-Feld: ein Feld mit
/// `serde(default)` würde einen nicht-vorhandenen Vertrag suggerieren.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CraftItemRef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub category: Option<String>,
}

/// Eine Kostenzeile. `oreId` ist NULL für `unit: gems`-Zeilen ohne
/// Ore-Bezug (live: Hadanite, Fixtures a03-sniper-rifle) — `Option`, nie
/// truthiness. `minQuality` ist Zahl-oder-null laut Spec (live: ganze Zahlen
/// 0/1) → `Option<f64>`-tolerant; `0` ist gültige Daten.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CraftCostEntry {
    /// Anzeige-Name des Materials (live z. B. `"Titanium"`, `"Hadanite"`).
    pub material: String,
    /// Ore-ID für den Mining-Modus-Sprung (M8-K3) — null-fähig (s. oben).
    #[serde(default)]
    pub ore_id: Option<String>,
    /// Menge in `unit` — nie über Einheiten hinweg aufsummieren.
    pub quantity: f64,
    /// `SCU` | `gems` | `items` — Einheiten-Regel oben.
    pub unit: CraftUnit,
    /// Geforderte Mindest-Qualität des Inputs, 0–1000 — null = keine
    /// Anforderung. Zahl-oder-null (Spec); 0 ist gültige Daten.
    #[serde(default)]
    pub min_quality: Option<f64>,
}

/// `GET /api/public/craft-cost/{item}` — `{item}` ist die UUID (id vor Slug;
/// die API nimmt auch den Slug, aber IDs sind kanonisch). Kein Envelope.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CraftCostResponse {
    pub item: CraftItemRef,
    /// Craft-Zeit in Sekunden (live: 960 / 180 — Ganzzahl-Wire-Wert).
    pub craft_time: i64,
    pub costs: Vec<CraftCostEntry>,
    /// Pflichtanzeige in der UI bei JEGLICHER öffentlichen Anzeige.
    pub attribution: String,
}

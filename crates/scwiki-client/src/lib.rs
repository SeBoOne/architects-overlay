//! Schlanker SC-Wiki-API-Client (S3, Karte t_988e132f) — ausschließlich für
//! die Schiffs-Modul-Kompatibilität im Hangar.
//!
//! Datenquellen (Research `docs/research/scwiki-modules.md`):
//! * `GET /api/game-versions/default` — aktuelle Game-Version (Sync-Trigger).
//! * `GET /api/vehicles?include=ports` — paginiert (Hardcap 50/Seite); je
//!   Fahrzeug die Port-Hardpoints mit `compatible_types` + Size-Range. Das ist
//!   die Kompatibilitäts-Quelle: Die API bietet KEINE Gegenrichtung
//!   „Modul X passt in Schiff Y" — die Zuordnung wird clientseitig
//!   cross-referenziert (Port-Typ/Size × Item-Katalog).
//! * `GET /api/items?filter[type]=T` — paginiert; Module pro Kategorie.
//!
//! Etikette (Punkt 6): geringe Rate (Standard 700 ms Abstand), nur ~30
//! Requests pro Game-Version, sonst kein Traffic.
//!
//! Cross-Referenz: [`build_compat`] ordnet jedem Item die Fahrzeuge zu, deren
//! Ports den Item-Typ (+ Sub-Typ) und die Item-Size akzeptieren.
//!
//! M8-K4 (t_72b27d99) erweitert den Client um Craft-Daten: [`ScWikiClient::
//! craftable_items`] (is_craftable-Filter + `name_cont`-Suche) und
//! [`ScWikiClient::blueprint`] (Detail mit tiers/modifiers/missions).
//! Kreuzreferenz zu Strata über [`StrataIndex`] (trim+lowercase, exakt —
//! fehlender Match degradiert, kein Hard-Fail); Strata bleibt die
//! Material-/Mengen-/MIN-QUALITY-Basis, SC-Wiki liefert Details/Stats/
//! Missionen. Attribution: [`ATTRIBUTION_SCWIKI`]. Cache-TTL für die
//! Sync-Schicht: `cache::TTL_SCWIKI_CRAFT` (7 Tage, source `scwiki`).

use std::time::{Duration, Instant};

use serde::Deserialize;

pub mod scmdb;
pub use scmdb::{
    MineableElement, MiningIndex, SCMDB_DEFAULT_BASE_URL, ScmdbClient, normalize_material,
};

/// SC-Wiki-API-Basis-URL (Research `docs/research/scwiki-modules.md`).
pub const DEFAULT_BASE_URL: &str = "https://api.star-citizen.wiki/api";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Rate-Limit-Etikette: SC-Wiki limitiert auf 60 req/min — 700 ms Abstand
/// (~85/min theoretisch, praktisch deutlich weniger Requests) bleibt safe.
pub const MIN_INTERVAL: Duration = Duration::from_millis(700);
/// Sicherheitsgrenze gegen Endlos-Pagination bei API-Änderungen.
/// Realität (Live-Beleg 2026-11-19): craftable items = 33 Seiten bei
/// 1630 Items (per_page=50, meta.last_page=33) — 30 kapppte hart mit
/// `TooManyPages`. 50 = Realität + Wachstumspuffer; der hart-bounded
/// Guard bleibt (Endlos-Schleife bei API-Bug weiterhin ausgeschlossen).
const MAX_PAGES: u32 = 50;
const PAGE_SIZE: u32 = 50;

/// Modul-relevante Item-Typen, die im Sync abgerufen werden. Bewusst
/// Whitelist (kein Vollabzug von 12 617 Items): Interieur-/Deko-Typen
/// (Paints, Door, Usable, FlightController, …) sind keine Hangar-Module.
pub const MODULE_TYPES: &[&str] = &[
    "Shield",
    "PowerPlant",
    "Cooler",
    "QuantumDrive",
    "Radar",
    "LifeSupportGenerator",
    "EMP",
    "QuantumInterdictionGenerator",
    "WeaponGun",
    "WeaponDefensive",
    "MissileLauncher",
    "BombLauncher",
    "Turret",
    "Module",
    "Container",
    "FuelTank",
    "MiningLaser",
    "TractorBeam",
];

/// Kategorie-Labels für die Autocomplete-Anzeige (Type → i18n-KEY;
/// T2 t_c5904bcb: das Frontend übersetzt via $t(label), Fallback en).
pub fn category_label(item_type: &str) -> String {
    match item_type {
        "Shield" => "moduletype.shield",
        "PowerPlant" => "moduletype.powerplant",
        "Cooler" => "moduletype.cooler",
        "QuantumDrive" => "moduletype.quantumdrive",
        "Radar" => "moduletype.radar",
        "LifeSupportGenerator" => "moduletype.lifesupport",
        "EMP" => "moduletype.emp",
        "QuantumInterdictionGenerator" => "moduletype.qinterdiction",
        "WeaponGun" => "moduletype.weapongun",
        "WeaponDefensive" => "moduletype.weapongdef",
        "MissileLauncher" => "moduletype.missile",
        "BombLauncher" => "moduletype.bomb",
        "Turret" => "moduletype.turret",
        "Module" => "moduletype.module",
        "Container" => "moduletype.container",
        "FuelTank" => "moduletype.fueltank",
        "MiningLaser" => "moduletype.mininglaser",
        "TractorBeam" => "moduletype.tractorbeam",
        other => other,
    }
    .to_string()
}

#[derive(Debug, thiserror::Error)]
pub enum ScWikiError {
    #[error("HTTP-Fehler: {0}")]
    Http(#[from] reqwest::Error),
    #[error("SC-Wiki HTTP-Status {0}")]
    Status(u16),
    #[error("Antwort-Format unerwartet: {0}")]
    Json(String),
    #[error("Pagination-Endlosschleife abgebrochen ({0} Seiten)")]
    TooManyPages(u32),
}

pub type Result<T> = std::result::Result<T, ScWikiError>;

/// Aktuelle Default-Game-Version (`/api/game-versions/default`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameVersion {
    pub code: String,
    pub channel: String,
    pub released_at: String,
}

/// Ein Port-Hardpoint eines Fahrzeugs: welcher Modul-Typ (mit Sub-Typen)
/// passt, und welche Größe.
#[derive(Debug, Clone)]
pub struct PortSlot {
    pub name: String,
    /// `compatible_types`-Einträge: (Type, Sub-Types).
    pub compatible: Vec<(String, Vec<String>)>,
    pub min_size: Option<u32>,
    pub max_size: Option<u32>,
}

impl PortSlot {
    /// Akzeptiert der Port ein Item mit `item_type`/`sub_type`/`size`?
    ///
    /// Sub-Typ-Check: leerer `sub_types`-Vektor = egal; `UNDEFINED`-Sub-Typen
    /// auf Item-Seite matchen nur, wenn der Port keine Sub-Typen einschränkt.
    pub fn accepts(&self, item_type: &str, sub_type: &str, size: Option<u32>) -> bool {
        let type_ok = self.compatible.iter().any(|(t, subs)| {
            t.eq_ignore_ascii_case(item_type)
                && (subs.is_empty()
                    || sub_type.eq_ignore_ascii_case("UNDEFINED")
                    || subs.iter().any(|s| s.eq_ignore_ascii_case(sub_type)))
        });
        if !type_ok {
            return false;
        }
        let (Some(min), Some(max)) = (self.min_size, self.max_size) else {
            // Port ohne Size-Angabe: Typ-Match genügt.
            return true;
        };
        match size {
            Some(s) => s >= min && s <= max,
            None => false,
        }
    }
}

/// Fahrzeug mit seinen Port-Hardpoints (nur Fahrzeuge MIT Ports landen hier).
#[derive(Debug, Clone)]
pub struct VehiclePorts {
    pub name: String,
    pub ports: Vec<PortSlot>,
}

/// Ein Modul-Item aus dem Katalog.
#[derive(Debug, Clone)]
pub struct ModuleItem {
    pub name: String,
    pub item_type: String,
    pub sub_type: String,
    pub size: Option<u32>,
}

/// Ergebnis der Cross-Referenz: Modul → Schiffe, die es akzeptieren.
#[derive(Debug, Clone, PartialEq)]
pub struct CompatRow {
    pub module_name: String,
    pub category: String,
    pub ships: Vec<String>,
}

/// SC-Wiki-Quirk (live belegt): Felder kommen teils EXPLIZIT als `null`
/// statt abwesend — `#[serde(default)]` deckt nur FEHLENDE Felder ab, nicht
/// `null`. Diese Helfer mappen null → leer/None (vgl. Serde-Regel UEX).
fn null_to_vec<'de, D, T>(d: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    let opt: Option<Vec<T>> = Deserialize::deserialize(d)?;
    Ok(opt.unwrap_or_default())
}

fn null_to_string<'de, D>(d: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<String> = Deserialize::deserialize(d)?;
    Ok(opt.unwrap_or_default())
}

#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(default)]
    data: serde_json::Value,
    #[serde(default)]
    meta: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct VersionDto {
    #[serde(deserialize_with = "null_to_string")]
    code: String,
    #[serde(default)]
    channel: String,
    #[serde(rename = "releasedAt", alias = "released_at", default)]
    released_at: String,
}

#[derive(Debug, Deserialize)]
struct VehicleDto {
    #[serde(deserialize_with = "null_to_string")]
    name: String,
    #[serde(default, deserialize_with = "null_to_opt_vec")]
    ports: Option<Vec<PortDto>>,
}

/// null → None (nicht default-Vec), damit „ohne Ports" vom Caller getrennt
/// werden kann.
fn null_to_opt_vec<'de, D, T>(d: D) -> std::result::Result<Option<Vec<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Deserialize::deserialize(d)
}

#[derive(Debug, Deserialize)]
struct PortDto {
    #[serde(default, deserialize_with = "null_to_string")]
    name: String,
    #[serde(
        rename = "compatibleTypes",
        alias = "compatible_types",
        default,
        deserialize_with = "null_to_vec"
    )]
    compatible_types: Vec<CompatibleTypeDto>,
    #[serde(default)]
    sizes: Option<SizesDto>,
}

#[derive(Debug, Deserialize)]
struct CompatibleTypeDto {
    #[serde(rename = "type", deserialize_with = "null_to_string")]
    item_type: String,
    #[serde(
        rename = "subTypes",
        alias = "sub_types",
        default,
        deserialize_with = "null_to_vec"
    )]
    sub_types: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SizesDto {
    #[serde(default)]
    min: Option<u32>,
    #[serde(default)]
    max: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct ItemDto {
    #[serde(deserialize_with = "null_to_string")]
    name: String,
    #[serde(rename = "type", deserialize_with = "null_to_string")]
    item_type: String,
    #[serde(
        rename = "subType",
        alias = "sub_type",
        default,
        deserialize_with = "null_to_string"
    )]
    sub_type: String,
    #[serde(default)]
    size: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct PageMeta {
    #[serde(rename = "lastPage", alias = "last_page", default)]
    last_page: u32,
}

/// Async HTTP-Client mit Pacing gegen die öffentliche SC-Wiki-API.
pub struct ScWikiClient {
    client: reqwest::Client,
    base_url: String,
    last_request: std::sync::Mutex<Instant>,
    min_interval: Duration,
}

impl ScWikiClient {
    pub fn new(base_url: &str) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent(concat!("architects-overlay/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(ScWikiError::Http)?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            last_request: std::sync::Mutex::new(Instant::now() - MIN_INTERVAL),
            min_interval: MIN_INTERVAL,
        })
    }

    /// Client mit überschriebener Basis-URL für Tests (wiremock).
    pub fn with_min_interval(mut self, interval: Duration) -> Self {
        self.min_interval = interval;
        self
    }

    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Envelope> {
        // Pacing: mindestens `min_interval` zwischen zwei Requests.
        let wait = {
            let last = self.last_request.lock().expect("pacing lock");
            self.min_interval
                .checked_sub(last.elapsed())
                .unwrap_or(Duration::ZERO)
        };
        if wait > Duration::ZERO {
            tokio::time::sleep(wait).await;
        }
        *self.last_request.lock().expect("pacing lock") = Instant::now();

        let url = format!("{base}{path}", base = self.base_url, path = path);
        let resp = self
            .client
            .get(&url)
            .query(query)
            .send()
            .await
            .map_err(ScWikiError::Http)?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(ScWikiError::Status(status));
        }
        resp.json::<Envelope>()
            .await
            .map_err(|e| ScWikiError::Json(format!("{path}: {e}")))
    }

    /// Aktuelle Default-Game-Version.
    pub async fn game_version_default(&self) -> Result<GameVersion> {
        let env = self.get("/game-versions/default", &[]).await?;
        let dto: VersionDto = serde_json::from_value(env.data)
            .map_err(|e| ScWikiError::Json(format!("game-versions/default: {e}")))?;
        Ok(GameVersion {
            code: dto.code,
            channel: dto.channel,
            released_at: dto.released_at,
        })
    }

    /// Einzelnes Fahrzeug per Slug (`/api/vehicles/{slug}`) — für Schiffe,
    /// die in der paginierten Liste fehlen (z. B. Hull E, Review D1).
    pub async fn vehicle_by_slug(&self, slug: &str) -> Result<VehiclePorts> {
        let env = self.get(&format!("/vehicles/{slug}"), &[]).await?;
        let dto: VehicleDto = serde_json::from_value(env.data)
            .map_err(|e| ScWikiError::Json(format!("vehicles/{slug}: {e}")))?;
        Ok(VehiclePorts {
            name: dto.name,
            ports: port_slots(dto.ports.unwrap_or_default()),
        })
    }

    /// Alle Fahrzeuge mit ihren Port-Hardpoints (paginiert, Hardcap 50/Seite).
    /// Fahrzeuge OHNE Ports kommen mit leerem `ports`-Vektor zurück — der
    /// Caller trennt damit „Schiffe mit Ports" vs. „ohne Ports" (S3 Punkt 4).
    pub async fn vehicles(&self) -> Result<Vec<VehiclePorts>> {
        let mut out = Vec::new();
        let mut page: u32 = 1;
        loop {
            let env = self
                .get(
                    "/vehicles",
                    &[
                        ("include", "ports".to_string()),
                        ("page[size]", PAGE_SIZE.to_string()),
                        ("page[number]", page.to_string()),
                    ],
                )
                .await?;
            let meta: PageMeta = serde_json::from_value(env.meta.clone())
                .map_err(|e| ScWikiError::Json(format!("vehicles meta: {e}")))?;
            let vehicles: Vec<VehicleDto> = serde_json::from_value(env.data)
                .map_err(|e| ScWikiError::Json(format!("vehicles data: {e}")))?;
            for v in vehicles {
                let ports = port_slots(v.ports.unwrap_or_default());
                out.push(VehiclePorts {
                    name: v.name,
                    ports,
                });
            }
            if page >= meta.last_page {
                break;
            }
            page += 1;
            if page > MAX_PAGES {
                return Err(ScWikiError::TooManyPages(MAX_PAGES));
            }
        }
        Ok(out)
    }

    /// Modul-Items für eine Liste von Typen (paginiert je Typ).
    pub async fn items_by_types(&self, types: &[&str]) -> Result<Vec<ModuleItem>> {
        let mut out = Vec::new();
        for ty in types {
            let mut page: u32 = 1;
            loop {
                let env = self
                    .get(
                        "/items",
                        &[
                            ("filter[type]", (*ty).to_string()),
                            ("page[size]", PAGE_SIZE.to_string()),
                            ("page[number]", page.to_string()),
                        ],
                    )
                    .await?;
                let meta: PageMeta = serde_json::from_value(env.meta.clone())
                    .map_err(|e| ScWikiError::Json(format!("items meta: {e}")))?;
                let items: Vec<ItemDto> = serde_json::from_value(env.data)
                    .map_err(|e| ScWikiError::Json(format!("items[{ty}]: {e}")))?;
                for it in items {
                    if it.name.trim().is_empty() {
                        continue;
                    }
                    out.push(ModuleItem {
                        name: it.name,
                        item_type: it.item_type,
                        sub_type: it.sub_type,
                        size: it.size,
                    });
                }
                if page >= meta.last_page {
                    break;
                }
                page += 1;
                if page > MAX_PAGES {
                    return Err(ScWikiError::TooManyPages(MAX_PAGES));
                }
            }
        }
        Ok(out)
    }
}

/// PortDto-Liste → PortSlot-Liste (dekorative Slots ohne Kompatibilität
/// fliegen raus — gleiche Filterung wie in `vehicles()`).
fn port_slots(dtos: Vec<PortDto>) -> Vec<PortSlot> {
    dtos.into_iter()
        .filter_map(|p| {
            if p.compatible_types.is_empty() {
                return None; // dekorativer Slot ohne Kompatibilität
            }
            Some(PortSlot {
                name: p.name,
                compatible: p
                    .compatible_types
                    .into_iter()
                    .map(|c| (c.item_type, c.sub_types))
                    .collect(),
                min_size: p.sizes.as_ref().and_then(|s| s.min),
                max_size: p.sizes.as_ref().and_then(|s| s.max),
            })
        })
        .collect()
}

/// Cross-Referenz Port × Item-Katalog: pro Modul die akzeptierenden Schiffe.
///
/// Dedupliziert Module nach Name (Katalog enthält Varianten mehrfach); Schiffe
/// nach Fahrzeug-`name`. Kategorien via [`category_label`].
pub fn build_compat(vehicles: &[VehiclePorts], items: &[ModuleItem]) -> Vec<CompatRow> {
    // Item-Dedup: gleicher Name → ein Row (Schiffe vereinigt).
    let mut by_name: std::collections::BTreeMap<String, CompatRow> =
        std::collections::BTreeMap::new();
    for item in items {
        let row = by_name
            .entry(item.name.clone())
            .or_insert_with(|| CompatRow {
                module_name: item.name.clone(),
                category: category_label(&item.item_type),
                ships: Vec::new(),
            });
        for v in vehicles {
            if v.ports
                .iter()
                .any(|p| p.accepts(&item.item_type, &item.sub_type, item.size))
                && !row.ships.iter().any(|s| s == &v.name)
            {
                row.ships.push(v.name.clone());
            }
        }
    }
    by_name.into_values().collect()
}

// =========================== Craft-Blueprints (M8-K4, t_72b27d99) ==========
//
// Quellen-Beleg (Live-Verifikation 2026-11-19, Fixtures in
// `tests/fixtures/` — ECHTE Responses, nicht erfunden):
// * `GET /api/items?filter[is_craftable]=1` (+ optional `filter[name_cont]`)
//   — paginiert; craft-relevante Felder: size, grade, class, mass,
//   ammunition, vehicle_weapon{capacity,range,damage_per_shot,rpm,
//   damages[]}, description.{de_DE,en_EN}, blueprint[{uuid,link}].
// * `GET /api/blueprints/{uuid}` — craft_time_seconds,
//   ingredients[{name,kind,quantity_scu}], dismantle_returns,
//   tiers[].requirements (nested GROUPS FRAME/CYCLER/… → resources mit
//   min_quality + modifiers[] mit quality/modifier-Range),
//   unlocking_missions_grouped[{label,chance,missions[...]}],
//   summary_properties, is_available_by_default, game_version — sowie
//   `aspects` (Envelope `{aspects[], aspect_groups, has_interactive_…}`,
//   Live-Beleg): die flache Aspekt-Liste mit `input` + `modifiers[]`
//   (property_key/label/better_when/quality_range/modifier_range/
//   value_range_type) — GENAU der Shape, den der Qualitäts-Stat-Rechner
//   (M8-K6, `src/lib/quality-calc.ts`) erwartet. M8-K5 (t_03eafbfb)
//   exposiert sie als `blueprintAspects` im Craft-Detail-Response.
//
// Kreuzreferenz-Strategie (Spec): SC-Wiki-Item.name ↔ Strata-Item.name,
// exakt nach trim+lowercase-Normalisierung; Blueprint via
// item.blueprint[0].uuid. Fehlender Match ist KEIN Fehler — der Caller
// degradiert (Abschnitt „Daten fehlen"). Strata bleibt die Material-/
// Mengen-/MIN-QUALITY-Basis, SC-Wiki liefert Details/Stats/Missionen.

// ---------- Aspekte (M8-K5, t_03eafbfb) — K6-Stat-Rechner-Contract ----------

/// Quality-Range eines Aspekt-Modifiers (`{min, max}`, Qualität 0–1000).
/// Eigenes Struct (nicht Tuple), damit das JSON exakt dem K6-Contract
/// (`src/lib/quality-calc.ts`) entspricht.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QualityRange {
    pub min: Option<f64>,
    pub max: Option<f64>,
}

/// Modifier-Range (`{at_min_quality, at_max_quality}`) — linearer Faktor
/// an den Range-Enden (z. B. 0.9 bei Q0, 1.1 bei Q1000).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModifierRange {
    #[serde(rename = "at_min_quality")]
    pub at_min_quality: Option<f64>,
    #[serde(rename = "at_max_quality")]
    pub at_max_quality: Option<f64>,
}

/// Aspekt-Modifier — Serialize-Shape = SC-Wiki `aspects[].modifiers[]` =
/// K6 `AspectModifier` (snake_case, bewusst KEIN camelCase-Rename: das
/// Frontend rechnet mit `property_key`/`quality_range`/`value_range_type`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AspectModifier {
    pub property_key: String,
    pub label: String,
    pub better_when: Option<String>,
    pub quality_range: Option<QualityRange>,
    pub modifier_range: Option<ModifierRange>,
    pub value_range_type: Option<String>,
}

/// Input eines Aspekts (Resource/Item) — K6 `AspectInput` (snake_case).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AspectInput {
    pub name: String,
    /// Commodity-UUID des Materials (Live-Beleg M8-R1-A, Blueprint
    /// Greatsword: `input.uuid`) — Schlüssel für die Snap-Punkte-Abfrage
    /// am Commodity-Endpoint; null degradiert (keine Snap-Punkte).
    #[serde(default)]
    pub uuid: Option<String>,
    pub min_quality: Option<f64>,
    pub quantity: Option<f64>,
    pub quantity_scu: Option<f64>,
}

/// Ein Aspekt des Blueprints (FRAME/CYCLER/BARREL/…) — K6 `Aspect`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BlueprintAspect {
    pub key: String,
    pub name: String,
    pub required_count: Option<u32>,
    pub input: Option<AspectInput>,
    pub modifiers: Vec<AspectModifier>,
}

#[derive(Debug, Deserialize)]
struct AspectsEnvelopeDto {
    #[serde(default, deserialize_with = "null_to_vec")]
    aspects: Vec<AspectDto>,
}

#[derive(Debug, Deserialize)]
struct AspectDto {
    #[serde(default, deserialize_with = "null_to_string")]
    key: String,
    #[serde(default, deserialize_with = "null_to_string")]
    name: String,
    #[serde(default)]
    required_count: Option<u32>,
    #[serde(default)]
    input: Option<AspectInputDto>,
    #[serde(default, deserialize_with = "null_to_vec")]
    modifiers: Vec<ModifierDto>,
}

#[derive(Debug, Deserialize)]
struct AspectInputDto {
    #[serde(default, deserialize_with = "null_to_string")]
    name: String,
    #[serde(default, deserialize_with = "null_to_opt_string")]
    uuid: Option<String>,
    #[serde(default)]
    min_quality: Option<f64>,
    #[serde(default)]
    quantity: Option<f64>,
    #[serde(default)]
    quantity_scu: Option<f64>,
}

fn aspect_from(dto: AspectDto) -> Option<BlueprintAspect> {
    if dto.key.trim().is_empty() && dto.name.trim().is_empty() {
        return None; // dekorativer Eintrag ohne Identität
    }
    Some(BlueprintAspect {
        key: dto.key,
        name: dto.name,
        required_count: dto.required_count,
        input: dto.input.map(|i| AspectInput {
            name: i.name,
            uuid: i.uuid,
            min_quality: i.min_quality,
            quantity: i.quantity,
            quantity_scu: i.quantity_scu,
        }),
        modifiers: dto.modifiers.iter().map(aspect_modifier_from).collect(),
    })
}

fn aspect_modifier_from(m: &ModifierDto) -> AspectModifier {
    AspectModifier {
        property_key: m.property_key.clone(),
        label: m.name.clone(),
        better_when: m.better_when.clone(),
        quality_range: m.quality_range.as_ref().map(|r| QualityRange {
            min: r.min,
            max: r.max,
        }),
        modifier_range: m.modifier_range.as_ref().map(|r| ModifierRange {
            at_min_quality: r.at_min_quality,
            at_max_quality: r.at_max_quality,
        }),
        value_range_type: m.value_range_type.clone(),
    }
}

/// Pflicht-Attribution (ToS-Muster wie Strata/UEX) — reist in jedem
/// Craft-Response mit und wird zusätzlich zur Strata-Angabe gezeigt.
pub const ATTRIBUTION_SCWIKI: &str = "Data from star-citizen.wiki";

/// Namens-Normalisierung für die Kreuzreferenz (Spec: exakt, trim +
/// lowercase — keine Fuzzy-Heuristik).
pub fn normalize_name(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Index über Strata-Item-Namen: normalisierter Name → Original-Name.
#[derive(Debug, Clone, Default)]
pub struct StrataIndex {
    by_normalized: std::collections::HashMap<String, String>,
}

impl StrataIndex {
    pub fn build<'a, I: IntoIterator<Item = &'a str>>(names: I) -> Self {
        let mut by_normalized = std::collections::HashMap::new();
        for n in names {
            let n = n.trim();
            if n.is_empty() {
                continue;
            }
            by_normalized
                .entry(normalize_name(n))
                .or_insert_with(|| n.to_string());
        }
        Self { by_normalized }
    }

    /// SC-Wiki-Item-Name → Strata-Original-Name. `None` = kein Match
    /// (Feature degradiert, kein Hard-Fail — Spec M8-K4).
    pub fn find(&self, scwiki_name: &str) -> Option<&str> {
        self.by_normalized
            .get(&normalize_name(scwiki_name))
            .map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.by_normalized.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_normalized.is_empty()
    }
}

/// Craftbares Item aus `/api/items?filter[is_craftable]=1`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CraftableItem {
    pub name: String,
    pub item_type: String,
    pub size: Option<u32>,
    pub grade: Option<String>,
    pub class: Option<String>,
    pub mass: Option<f64>,
    pub description_de: Option<String>,
    pub description_en: Option<String>,
    /// Blueprint-UUIDs (`blueprint[]`) — für die `/api/blueprints/{uuid}`
    /// Auflösung; leer = keine Craft-Details verfügbar (degradiert).
    pub blueprint_uuids: Vec<String>,
    pub vehicle_weapon: Option<VehicleWeapon>,
    pub ammunition: Option<Ammunition>,
}

/// Fahrzeug-Waffen-Stats (`vehicle_weapon`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VehicleWeapon {
    pub capacity: Option<f64>,
    pub range: Option<f64>,
    pub damage_per_shot: Option<f64>,
    pub rpm: Option<f64>,
    pub damages: Vec<WeaponDamage>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WeaponDamage {
    pub damage_type: String,
    pub damage: Option<f64>,
}

/// Munitions-Stats (`ammunition`) — nur die Anzeige-relevanten Felder.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Ammunition {
    pub size: Option<u32>,
    pub capacity: Option<u32>,
    pub speed: Option<f64>,
    pub range: Option<f64>,
    pub lifetime: Option<f64>,
}

/// Blueprint-Detail aus `/api/blueprints/{uuid}`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Blueprint {
    pub uuid: String,
    pub output_name: String,
    pub craft_time_seconds: Option<u64>,
    pub craft_time_label: Option<String>,
    pub is_available_by_default: bool,
    pub game_version: Option<String>,
    pub ingredients: Vec<Ingredient>,
    pub dismantle_returns: Vec<DismantleReturn>,
    /// Tiers mit verschachtelten Requirement-Gruppen (FRAME/CYCLER/…) —
    /// pro Gruppe Resources mit min_quality + Quality-Modifiers.
    pub tiers: Vec<BlueprintTier>,
    pub summary_properties: Vec<SummaryProperty>,
    /// Flache Aspekt-Liste (`aspects.aspects[]`, M8-K5) — Basis des
    /// `blueprintAspects`-Felds im Craft-Detail-Response (K6-Contract).
    pub aspects: Vec<BlueprintAspect>,
    /// Missions-Unlock-Gruppen (`Guaranteed`/Chance-Labels).
    pub unlocking_missions_grouped: Vec<MissionGroup>,
    pub web_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Ingredient {
    pub name: String,
    /// `resource` oder `item` (Spec).
    pub kind: String,
    pub quantity_scu: Option<f64>,
    /// Commodity-UUID aus `ingredients[].link` (LIVE-Beleg t_1dd162e3,
    /// Blueprint SW16BR1 „Buzzsaw": ingredient Borase → resource_type_uuid
    /// 33bff393 = Raffinat, link 728a3d7e = „Borase (Ore)") — SC-Wiki-
    /// eigener Zeiger auf das Vorkommens-Commodity; sekundärer Anker des
    /// Snap-Fallbacks Raffinat → Erz. `None` = kein parsbarer Link.
    #[serde(default)]
    pub commodity_uuid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DismantleReturn {
    pub name: String,
    pub quantity_scu: Option<f64>,
    /// Commodity-UUID hinter dem Return (Live-Beleg Blueprint-Endpoint,
    /// `dismantle_returns[].resource_type_uuid`, t_1dd162e3) — sekundärer
    /// Anker des Snap-Fallbacks (Raffinat → Erz); null degradiert.
    #[serde(default)]
    pub resource_type_uuid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BlueprintTier {
    pub tier_index: u32,
    pub requirements: RequirementGroup,
    pub craft_time_seconds: Option<u64>,
}

/// Requirement-Gruppe (kind=group): KEY (FRAME/CYCLER/…), Modifier und
/// Kinder (Resources). `children` sind Resources ODER weitere Gruppen —
/// die API nestet aktuell nur eine Ebene; leere Kinder sind erlaubt.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RequirementGroup {
    pub key: Option<String>,
    pub name: Option<String>,
    pub kind: Option<String>,
    pub required_count: Option<u32>,
    pub modifiers: Vec<QualityModifier>,
    pub children: Vec<RequirementChild>,
}

/// Quality-Modifier an einer Gruppe/Resource: Qualität 0..1000 mappt
/// linear auf modifier_range (at_min_quality..at_max_quality).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QualityModifier {
    /// `property_key` (requirement_groups) bzw. `key` (tiers) — beides
    /// derselbe Schlüssel (Live-Beleg), via Alias vereint.
    pub property_key: String,
    pub label: String,
    pub better_when: Option<String>,
    pub quality_min: Option<f64>,
    pub quality_max: Option<f64>,
    pub at_min_quality: Option<f64>,
    pub at_max_quality: Option<f64>,
    /// `linear` oder `segments` (value_segments non-null).
    pub value_range_type: Option<String>,
}

/// Kind-Eintrag in einer Requirement-Gruppe: eine Resource (mit
/// min_quality, Spec) — Items-Kinder kämen analog als kind=item.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RequirementChild {
    pub name: String,
    pub kind: Option<String>,
    pub uuid: Option<String>,
    pub min_quality: Option<f64>,
    pub quantity_scu: Option<f64>,
    pub required_count: Option<u32>,
    /// Verschachtelte Gruppe (kind=group) — aktuell max. 1 Ebene.
    pub group: Option<Box<RequirementGroup>>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SummaryProperty {
    pub property_key: String,
    pub label: String,
    pub better_when: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MissionGroup {
    pub label: String,
    pub chance: Option<f64>,
    pub missions: Vec<UnlockingMission>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UnlockingMission {
    pub title: String,
    /// Interne Missions-ID (Live-Beleg `debug_name` in
    /// `unlocking_missions`) — kompakte Anzeige unter dem Titel; null
    /// degradiert (die grouped-Liste trägt sie nicht immer).
    pub debug_name: Option<String>,
    pub reward_scope: Option<String>,
    pub count: Option<u32>,
    pub web_url: Option<String>,
}

// ---------- DTOs (serde) — null-tolerant wie der Rest des Crates ----------

#[derive(Debug, Deserialize)]
struct CraftItemDto {
    #[serde(deserialize_with = "null_to_string")]
    name: String,
    #[serde(rename = "type", default, deserialize_with = "null_to_string")]
    item_type: String,
    #[serde(default)]
    size: Option<u32>,
    #[serde(default, deserialize_with = "null_to_opt_string")]
    grade: Option<String>,
    #[serde(default, deserialize_with = "null_to_opt_string")]
    class: Option<String>,
    #[serde(default)]
    mass: Option<f64>,
    #[serde(default)]
    description: Option<DescriptionDto>,
    #[serde(default, deserialize_with = "null_to_opt_vec")]
    blueprint: Option<Vec<BlueprintRefDto>>,
    #[serde(default)]
    vehicle_weapon: Option<VehicleWeaponDto>,
    #[serde(default)]
    ammunition: Option<AmmunitionDto>,
}

fn null_to_opt_string<'de, D>(d: D) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Deserialize::deserialize(d)
}

#[derive(Debug, Deserialize)]
struct DescriptionDto {
    #[serde(
        rename = "de_DE",
        alias = "de_de",
        alias = "deDE",
        default,
        deserialize_with = "null_to_opt_string"
    )]
    de_de: Option<String>,
    #[serde(
        rename = "en_EN",
        alias = "en_en",
        alias = "enEN",
        default,
        deserialize_with = "null_to_opt_string"
    )]
    en_en: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BlueprintRefDto {
    #[serde(default, deserialize_with = "null_to_string")]
    uuid: String,
}

#[derive(Debug, Deserialize)]
struct VehicleWeaponDto {
    #[serde(default)]
    capacity: Option<f64>,
    #[serde(default)]
    range: Option<f64>,
    #[serde(default)]
    damage_per_shot: Option<f64>,
    #[serde(default)]
    rpm: Option<f64>,
    #[serde(default, deserialize_with = "null_to_vec")]
    damages: Vec<WeaponDamageDto>,
}

#[derive(Debug, Deserialize)]
struct WeaponDamageDto {
    #[serde(rename = "type", default, deserialize_with = "null_to_string")]
    damage_type: String,
    #[serde(default)]
    damage: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct AmmunitionDto {
    #[serde(default)]
    size: Option<u32>,
    #[serde(default)]
    capacity: Option<u32>,
    #[serde(default)]
    speed: Option<f64>,
    #[serde(default)]
    range: Option<f64>,
    #[serde(default)]
    lifetime: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct BlueprintDto {
    #[serde(default, deserialize_with = "null_to_string")]
    uuid: String,
    #[serde(default, deserialize_with = "null_to_string")]
    output_name: String,
    #[serde(default)]
    craft_time_seconds: Option<u64>,
    #[serde(default, deserialize_with = "null_to_opt_string")]
    craft_time_label: Option<String>,
    #[serde(default, deserialize_with = "null_to_bool")]
    is_available_by_default: bool,
    #[serde(default, deserialize_with = "null_to_opt_string")]
    game_version: Option<String>,
    #[serde(default, deserialize_with = "null_to_vec")]
    ingredients: Vec<IngredientDto>,
    #[serde(default, deserialize_with = "null_to_vec")]
    dismantle_returns: Vec<DismantleReturnDto>,
    #[serde(default, deserialize_with = "null_to_vec")]
    tiers: Vec<TierDto>,
    #[serde(default, deserialize_with = "null_to_vec")]
    summary_properties: Vec<SummaryPropertyDto>,
    /// `aspects`-Envelope (`{aspects[], …}`) — fehlt/null → leer.
    #[serde(default)]
    aspects: Option<AspectsEnvelopeDto>,
    #[serde(
        rename = "unlocking_missions_grouped",
        alias = "unlockingMissionsGrouped",
        default,
        deserialize_with = "null_to_vec"
    )]
    unlocking_missions_grouped: Vec<MissionGroupDto>,
    #[serde(default, deserialize_with = "null_to_opt_string")]
    web_url: Option<String>,
}

fn null_to_bool<'de, D>(d: D) -> std::result::Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<bool> = Deserialize::deserialize(d)?;
    Ok(opt.unwrap_or(false))
}

/// Letztes Pfad-Segment einer API-Link-URL als UUID (z. B.
/// `https://api.star-citizen.wiki/api/commodities/728a3d7e-…` → die UUID);
/// `None` ohne UUID-förmiges Segment (36 Zeichen, 4 Bindestriche, Hex).
fn link_uuid(link: &str) -> Option<String> {
    let seg = link.rsplit('/').next()?.trim();
    if seg.len() != 36 || seg.matches('-').count() != 4 {
        return None;
    }
    let hex = |c: char| c.is_ascii_hexdigit();
    if seg.chars().all(|c| hex(c) || c == '-') {
        Some(seg.to_string())
    } else {
        None
    }
}

#[derive(Debug, Deserialize)]
struct IngredientDto {
    #[serde(default, deserialize_with = "null_to_string")]
    name: String,
    #[serde(default, deserialize_with = "null_to_string")]
    kind: String,
    #[serde(default)]
    quantity_scu: Option<f64>,
    #[serde(default, deserialize_with = "null_to_opt_string")]
    link: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DismantleReturnDto {
    #[serde(default, deserialize_with = "null_to_string")]
    name: String,
    #[serde(default, deserialize_with = "null_to_opt_string")]
    resource_type_uuid: Option<String>,
    #[serde(default)]
    quantity_scu: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct TierDto {
    #[serde(default)]
    tier_index: u32,
    #[serde(default)]
    requirements: Option<ReqNodeDto>,
    #[serde(default)]
    craft_time_seconds: Option<u64>,
}

/// Requirement-Baum-Knoten: group (key/name/children/modifiers) oder
/// resource-Kind. Tiers nutzen `key`/`name`, requirement_groups oben
/// `property_key`/`label` — Aliase decken beide Shapes ab (Live-Beleg).
#[derive(Debug, Deserialize)]
struct ReqNodeDto {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default, deserialize_with = "null_to_opt_string")]
    key: Option<String>,
    #[serde(default, deserialize_with = "null_to_opt_string")]
    label: Option<String>,
    #[serde(default)]
    required_count: Option<u32>,
    #[serde(default, deserialize_with = "null_to_vec")]
    modifiers: Vec<ModifierDto>,
    #[serde(default, deserialize_with = "null_to_vec")]
    children: Vec<ReqNodeDto>,
    // Resource-Kind-Felder:
    #[serde(default, deserialize_with = "null_to_opt_string")]
    name: Option<String>,
    #[serde(default, deserialize_with = "null_to_opt_string")]
    uuid: Option<String>,
    #[serde(default)]
    min_quality: Option<f64>,
    #[serde(default)]
    quantity_scu: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct ModifierDto {
    #[serde(
        rename = "property_key",
        alias = "key",
        default,
        deserialize_with = "null_to_string"
    )]
    property_key: String,
    #[serde(alias = "label", default, deserialize_with = "null_to_string")]
    name: String,
    #[serde(default, deserialize_with = "null_to_opt_string")]
    better_when: Option<String>,
    #[serde(default)]
    quality_range: Option<RangeDto>,
    #[serde(default)]
    modifier_range: Option<RangeDto>,
    #[serde(default, deserialize_with = "null_to_opt_string")]
    value_range_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RangeDto {
    #[serde(default)]
    min: Option<f64>,
    #[serde(default)]
    max: Option<f64>,
    #[serde(rename = "at_min_quality", alias = "atMinQuality", default)]
    at_min_quality: Option<f64>,
    #[serde(rename = "at_max_quality", alias = "atMaxQuality", default)]
    at_max_quality: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct SummaryPropertyDto {
    #[serde(
        rename = "property_key",
        alias = "key",
        default,
        deserialize_with = "null_to_string"
    )]
    property_key: String,
    #[serde(alias = "label", default, deserialize_with = "null_to_string")]
    name: String,
    #[serde(default, deserialize_with = "null_to_opt_string")]
    better_when: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MissionGroupDto {
    #[serde(default, deserialize_with = "null_to_string")]
    label: String,
    #[serde(default)]
    chance: Option<f64>,
    #[serde(default, deserialize_with = "null_to_vec")]
    missions: Vec<MissionDto>,
}

#[derive(Debug, Deserialize)]
struct MissionDto {
    #[serde(default, deserialize_with = "null_to_string")]
    title: String,
    #[serde(default, deserialize_with = "null_to_opt_string")]
    debug_name: Option<String>,
    #[serde(default, deserialize_with = "null_to_opt_string")]
    reward_scope: Option<String>,
    #[serde(default)]
    count: Option<u32>,
    #[serde(default, deserialize_with = "null_to_opt_string")]
    web_url: Option<String>,
}

// ---------- Mapping DTO → Domänen-Structs ----------

fn craft_item_from(dto: CraftItemDto) -> CraftableItem {
    CraftableItem {
        name: dto.name,
        item_type: dto.item_type,
        size: dto.size,
        grade: dto.grade,
        class: dto.class,
        mass: dto.mass,
        description_de: dto.description.as_ref().and_then(|d| d.de_de.clone()),
        description_en: dto.description.as_ref().and_then(|d| d.en_en.clone()),
        blueprint_uuids: dto
            .blueprint
            .unwrap_or_default()
            .into_iter()
            .filter(|b| !b.uuid.is_empty())
            .map(|b| b.uuid)
            .collect(),
        vehicle_weapon: dto.vehicle_weapon.map(|v| VehicleWeapon {
            capacity: v.capacity,
            range: v.range,
            damage_per_shot: v.damage_per_shot,
            rpm: v.rpm,
            damages: v
                .damages
                .into_iter()
                .map(|d| WeaponDamage {
                    damage_type: d.damage_type,
                    damage: d.damage,
                })
                .collect(),
        }),
        ammunition: dto.ammunition.map(|a| Ammunition {
            size: a.size,
            capacity: a.capacity,
            speed: a.speed,
            range: a.range,
            lifetime: a.lifetime,
        }),
    }
}

fn req_node_to_group(node: ReqNodeDto) -> RequirementGroup {
    // Gruppen heißen `label` ODER `name` (tier-Groups: name; keine
    // top-level Groups ohne beide — Anzeige-Fallback ist beides erlaubt).
    let display = node.label.clone().or_else(|| node.name.clone());
    if node.children.is_empty() && node.kind.as_deref() != Some("group") && node.name.is_some() {
        // Ressourcen-Kind ohne Kinder → als Gruppe mit einem Child rendern,
        // damit requirement_groups + tiers denselben Typ haben.
        return RequirementGroup {
            key: node.key,
            name: display,
            kind: node.kind.clone(),
            required_count: node.required_count,
            modifiers: node.modifiers.iter().map(modifier_from).collect(),
            children: vec![RequirementChild {
                name: node.name.unwrap_or_default(),
                kind: node.kind,
                uuid: node.uuid,
                min_quality: node.min_quality,
                quantity_scu: node.quantity_scu,
                required_count: node.required_count,
                group: None,
            }],
        };
    }
    RequirementGroup {
        key: node.key,
        name: display,
        kind: node.kind,
        required_count: node.required_count,
        modifiers: node.modifiers.iter().map(modifier_from).collect(),
        children: node
            .children
            .into_iter()
            .map(|c| {
                if c.kind.as_deref() == Some("group") || !c.children.is_empty() {
                    RequirementChild {
                        name: c.name.clone().unwrap_or_default(),
                        kind: c.kind.clone(),
                        uuid: c.uuid.clone(),
                        min_quality: c.min_quality,
                        quantity_scu: c.quantity_scu,
                        required_count: c.required_count,
                        group: Some(Box::new(req_node_to_group(c))),
                    }
                } else {
                    RequirementChild {
                        name: c.name.unwrap_or_default(),
                        kind: c.kind,
                        uuid: c.uuid,
                        min_quality: c.min_quality,
                        quantity_scu: c.quantity_scu,
                        required_count: c.required_count,
                        group: None,
                    }
                }
            })
            .collect(),
    }
}

fn modifier_from(m: &ModifierDto) -> QualityModifier {
    QualityModifier {
        property_key: m.property_key.clone(),
        label: m.name.clone(),
        better_when: m.better_when.clone(),
        quality_min: m.quality_range.as_ref().and_then(|r| r.min),
        quality_max: m.quality_range.as_ref().and_then(|r| r.max),
        at_min_quality: m.modifier_range.as_ref().and_then(|r| r.at_min_quality),
        at_max_quality: m.modifier_range.as_ref().and_then(|r| r.at_max_quality),
        value_range_type: m.value_range_type.clone(),
    }
}

fn blueprint_from(dto: BlueprintDto) -> Blueprint {
    Blueprint {
        uuid: dto.uuid,
        output_name: dto.output_name,
        craft_time_seconds: dto.craft_time_seconds,
        craft_time_label: dto.craft_time_label,
        is_available_by_default: dto.is_available_by_default,
        game_version: dto.game_version,
        ingredients: dto
            .ingredients
            .into_iter()
            .filter(|i| !i.name.is_empty())
            .map(|i| Ingredient {
                name: i.name,
                kind: i.kind,
                quantity_scu: i.quantity_scu,
                commodity_uuid: i.link.as_deref().and_then(link_uuid),
            })
            .collect(),
        dismantle_returns: dto
            .dismantle_returns
            .into_iter()
            .filter(|d| !d.name.is_empty())
            .map(|d| DismantleReturn {
                name: d.name,
                quantity_scu: d.quantity_scu,
                resource_type_uuid: d.resource_type_uuid,
            })
            .collect(),
        tiers: dto
            .tiers
            .into_iter()
            .filter_map(|t| {
                t.requirements.map(|r| BlueprintTier {
                    tier_index: t.tier_index,
                    requirements: req_node_to_group(r),
                    craft_time_seconds: t.craft_time_seconds,
                })
            })
            .collect(),
        summary_properties: dto
            .summary_properties
            .into_iter()
            .filter(|p| !p.property_key.is_empty())
            .map(|p| SummaryProperty {
                property_key: p.property_key,
                label: p.name,
                better_when: p.better_when,
            })
            .collect(),
        aspects: dto
            .aspects
            .map(|env| env.aspects.into_iter().filter_map(aspect_from).collect())
            .unwrap_or_default(),
        unlocking_missions_grouped: dto
            .unlocking_missions_grouped
            .into_iter()
            .map(|g| MissionGroup {
                label: g.label,
                chance: g.chance,
                missions: g
                    .missions
                    .into_iter()
                    .filter(|m| !m.title.is_empty())
                    .map(|m| UnlockingMission {
                        title: m.title,
                        debug_name: m.debug_name,
                        reward_scope: m.reward_scope,
                        count: m.count,
                        web_url: m.web_url,
                    })
                    .collect(),
            })
            .collect(),
        web_url: dto.web_url,
    }
}

impl ScWikiClient {
    /// Craftbare Items (`/api/items?filter[is_craftable]=1`), optional mit
    /// `name_cont`-Suche. Paginiert; leere Namen fliegen raus.
    pub async fn craftable_items(&self, name_search: Option<&str>) -> Result<Vec<CraftableItem>> {
        let mut out = Vec::new();
        let mut page: u32 = 1;
        loop {
            let mut query = vec![
                ("filter[is_craftable]", "1".to_string()),
                ("page[size]", PAGE_SIZE.to_string()),
                ("page[number]", page.to_string()),
            ];
            if let Some(q) = name_search.map(str::trim).filter(|q| !q.is_empty()) {
                query.push(("filter[name_cont]", q.to_string()));
            }
            let env = self.get("/items", &query).await?;
            let meta: PageMeta = serde_json::from_value(env.meta.clone())
                .map_err(|e| ScWikiError::Json(format!("items meta: {e}")))?;
            let items: Vec<CraftItemDto> = serde_json::from_value(env.data)
                .map_err(|e| ScWikiError::Json(format!("items[craftable]: {e}")))?;
            for it in items {
                if it.name.trim().is_empty() {
                    continue;
                }
                out.push(craft_item_from(it));
            }
            if page >= meta.last_page {
                break;
            }
            page += 1;
            if page > MAX_PAGES {
                return Err(ScWikiError::TooManyPages(MAX_PAGES));
            }
        }
        Ok(out)
    }

    /// Blueprint-Detail per UUID (`/api/blueprints/{uuid}`).
    pub async fn blueprint(&self, uuid: &str) -> Result<Blueprint> {
        let uuid = uuid.trim();
        if uuid.is_empty() {
            return Err(ScWikiError::Json("blueprint: uuid leer".into()));
        }
        let env = self.get(&format!("/blueprints/{uuid}"), &[]).await?;
        let dto: BlueprintDto = serde_json::from_value(env.data)
            .map_err(|e| ScWikiError::Json(format!("blueprints/{uuid}: {e}")))?;
        Ok(blueprint_from(dto))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(types: &[(&str, &[&str])], min: u32, max: u32) -> PortSlot {
        PortSlot {
            name: "test_port".into(),
            compatible: types
                .iter()
                .map(|(t, s)| ((*t).to_string(), s.iter().map(|x| x.to_string()).collect()))
                .collect(),
            min_size: Some(min),
            max_size: Some(max),
        }
    }

    // ---------- M8-K4: Kreuzreferenz (SC-Wiki-Name ↔ Strata-Name) ----------

    #[test]
    fn normalize_name_trims_and_lowercases() {
        assert_eq!(
            normalize_name("  10-Series Greatsword  "),
            "10-series greatsword"
        );
        assert_eq!(normalize_name("IRON"), "iron");
    }

    // ---------- t_1dd162e3: Ingredient-Link → Commodity-UUID ----------

    #[test]
    fn link_uuid_parses_last_path_segment() {
        assert_eq!(
            link_uuid(
                "https://api.star-citizen.wiki/api/commodities/728a3d7e-9567-4e1c-87fd-2bd45b458aa6"
            ),
            Some("728a3d7e-9567-4e1c-87fd-2bd45b458aa6".to_owned())
        );
        // Kein UUID-förmiges Segment → None (kein Raten).
        assert_eq!(link_uuid("https://api.star-citizen.wiki/api"), None);
        assert_eq!(link_uuid(""), None);
        assert_eq!(link_uuid("a/b/c/not-a-uuid"), None);
        assert_eq!(link_uuid("x/0123456789-0123-0123-0123-0123456789g"), None);
    }

    #[test]
    fn strata_index_matches_exactly_and_degrades_on_missing() {
        let idx = StrataIndex::build(["10-Series Greatsword Cannon", "  Titanium  "]);
        assert_eq!(idx.len(), 2);
        assert_eq!(
            idx.find("10-series greatsword cannon"),
            Some("10-Series Greatsword Cannon")
        );
        assert_eq!(idx.find("  TITANIUM "), Some("Titanium"));
        // Kein Match = None (Feature degradiert, KEIN hard fail).
        assert_eq!(idx.find("Nicht Vorhanden"), None);
        let empty = StrataIndex::build([""]);
        assert!(empty.is_empty());
    }

    #[test]
    fn port_accepts_matches_type_sub_and_size() {
        let p = port(
            &[("Shield", &["UNDEFINED"]), ("Module", &["DM", "TS"])],
            1,
            2,
        );
        assert!(p.accepts("Shield", "UNDEFINED", Some(1)));
        assert!(p.accepts("shield", "undefined", Some(2))); // Case-insensitive
        assert!(p.accepts("Module", "TS", Some(1)));
        assert!(!p.accepts("PowerPlant", "UNDEFINED", Some(1)));
        assert!(!p.accepts("Shield", "UNDEFINED", Some(3))); // Size außer Range
        assert!(!p.accepts("Shield", "UNDEFINED", None));
    }

    #[test]
    fn build_compat_maps_aurora_style_data() {
        let vehicles = vec![VehiclePorts {
            name: "Aurora Mk II".into(),
            ports: vec![port(&[("Module", &[])], 1, 1)],
        }];
        let items = vec![
            ModuleItem {
                name: "Aurora Mk II DM Module".into(),
                item_type: "Module".into(),
                sub_type: "UNDEFINED".into(),
                size: Some(1),
            },
            ModuleItem {
                name: "Aurora Mk II TS Module".into(),
                item_type: "Module".into(),
                sub_type: "UNDEFINED".into(),
                size: Some(1),
            },
            ModuleItem {
                name: "Apollo Tier 1 Module Left".into(),
                item_type: "Module".into(),
                sub_type: "UNDEFINED".into(),
                size: Some(3),
            },
        ];
        let rows = build_compat(&vehicles, &items);
        assert_eq!(rows.len(), 3);
        let dm = rows.iter().find(|r| r.module_name.contains("DM")).unwrap();
        assert_eq!(dm.category, "moduletype.module");
        assert_eq!(dm.ships, vec!["Aurora Mk II".to_string()]);
        // Size-3-Modul passt nicht in Size-1-Port.
        let apollo = rows
            .iter()
            .find(|r| r.module_name.contains("Apollo"))
            .unwrap();
        assert!(apollo.ships.is_empty());
    }

    #[test]
    fn build_compat_dedupes_items_by_name() {
        let vehicles = vec![VehiclePorts {
            name: "C2 Hercules".into(),
            ports: vec![port(&[("Shield", &[])], 1, 3)],
        }];
        let items = vec![
            ModuleItem {
                name: "5CA 'Akura'".into(),
                item_type: "Shield".into(),
                sub_type: "UNDEFINED".into(),
                size: Some(3),
            },
            ModuleItem {
                name: "5CA 'Akura'".into(),
                item_type: "Shield".into(),
                sub_type: "UNDEFINED".into(),
                size: Some(3),
            },
        ];
        let rows = build_compat(&vehicles, &items);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].ships.len(), 1);
    }
}

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

use std::time::{Duration, Instant};

use serde::Deserialize;

/// SC-Wiki-API-Basis-URL (Research `docs/research/scwiki-modules.md`).
pub const DEFAULT_BASE_URL: &str = "https://api.star-citizen.wiki/api";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Rate-Limit-Etikette: SC-Wiki limitiert auf 60 req/min — 700 ms Abstand
/// (~85/min theoretisch, praktisch deutlich weniger Requests) bleibt safe.
pub const MIN_INTERVAL: Duration = Duration::from_millis(700);
/// Sicherheitsgrenze gegen Endlos-Pagination bei API-Änderungen.
const MAX_PAGES: u32 = 30;
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
        assert_eq!(dm.category, "Modul");
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

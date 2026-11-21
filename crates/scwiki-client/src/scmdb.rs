//! SCMDB-Client (M8-R2, t_7a7844cb) — KrovaxCode/SCMDB_DATA als Quelle der
//! Mining-Qualitätsbänder für die Craft-Snap-Punkte.
//!
//! * **Quelle:** raw.githubusercontent.com/KrovaxCode/SCMDB_DATA —
//!   `data/latest.json` → `channels.live.files.mining_data` (Dateiname inkl.
//!   Version/Build, LIVE-Beleg 2026-11-20: `mining_data-4.10.3-live.13220821.json`).
//!   EIN latest-Fetch + EIN Daten-Fetch, Cache-TTL 7 Tage (Patch-zyklisch,
//!   `cache::TTL_SCWIKI_CRAFT`) — kein Traffic pro UI-Aufruf.
//! * **Struktur:** `mineableElements` (dict uuid→Element): `name`
//!   („Taranite (Raw)"/„Borase (Ore)"/„Aphorite"), `materialName` (kann
//!   null sein!), `qualityBands` (8 Werte, sortiert, endet auf 1000) —
//!   LIVE-Belege: Agricium [346,588,667,796,852,943,971,1000] · Taranite
//!   (Raw) [310,525,646,718,853,925,957,1000] · Hadanite
//!   [274,526,665,762,867,916,959,1000].
//! * **Match ([`normalize_material`]):** Materialname → trim + lowercase +
//!   `(Ore)`/`(Raw)`/`(Pure)`-Suffix STRIPpen → Match gegen die
//!   normalisierte `materialName`-Form ODER die normalisierte `name`-Form
//!   („Taranite (Raw)" trägt materialName Taranite — name ist der
//!   zuverlässigere Anker). Carinite-Dup („Carinite" × 2 + „Carinite
//!   (Pure)"): Bänder werden per Union dedupliziert — identische Werte
//!   kollabieren zu einem Satz.
//! * Kein Raten: kein Match → leere Liste (UI rastet auf 500-only).

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Deserialize;

use super::{Result, ScWikiError};

/// Basis-URL des SCMDB_DATA-Rohdaten-Repos (raw GitHub, keine API).
pub const SCMDB_DEFAULT_BASE_URL: &str =
    "https://raw.githubusercontent.com/KrovaxCode/SCMDB_DATA/main";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// `data/latest.json` — nur der Pfad zum Live-Mining-File.
#[derive(Debug, Deserialize)]
struct LatestDto {
    channels: LatestChannels,
}

#[derive(Debug, Deserialize)]
struct LatestChannels {
    live: LatestChannel,
}

#[derive(Debug, Deserialize)]
struct LatestChannel {
    files: LatestFiles,
}

#[derive(Debug, Deserialize)]
struct LatestFiles {
    mining_data: String,
}

/// Ein abbaubares Element aus `mineableElements` — nur die Felder, die
/// die Snap-Auflösung braucht (Rest ignoriert serde).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MineableElement {
    /// „Taranite (Raw)" / „Borase (Ore)" / „Aphorite"
    pub name: String,
    /// Kann null sein (Raw-/Pure-Formen haben keinen materialName? Doch —
    /// LIVE-Beleg: „Taranite (Raw)" trägt materialName „Taranite"; die
    /// unklassifizierten (Aphorite, Carinite, …) haben null).
    #[serde(rename = "materialName")]
    pub material_name: Option<String>,
    /// 8 Band-Grenzen, sortiert, endet auf 1000.
    #[serde(rename = "qualityBands")]
    pub quality_bands: Vec<u32>,
}

/// geparstes `mining_data-<version>.json` — normalisierter Name → Bänder.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MiningIndex {
    /// normalisiertes `materialName` ODER `name` → Band-Union (sortiert, dedup).
    by_normalized: BTreeMap<String, Vec<u32>>,
}

impl MiningIndex {
    /// Bänder für einen (beliebig geschriebenen) Materialnamen. Kein
    /// Match → leere Liste.
    pub fn bands_for(&self, material: &str) -> Vec<u32> {
        self.by_normalized
            .get(&normalize_material(material))
            .cloned()
            .unwrap_or_default()
    }

    /// Belegte Index-Größe (Tests/Diagnose).
    pub fn len(&self) -> usize {
        self.by_normalized.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_normalized.is_empty()
    }
}

/// Materialname → Match-Key: trim + lowercase + `(Ore)`/`(Raw)`/`(Pure)`-
/// Suffix STRIPpen. „Taranite (Raw)" und „taranite " → „taranite".
pub fn normalize_material(name: &str) -> String {
    let n = name.trim().to_lowercase();
    for suffix in [" (ore)", " (raw)", " (pure)"] {
        if let Some(base) = n.strip_suffix(suffix) {
            return base.trim().to_string();
        }
    }
    n
}

impl MiningIndex {
    /// Aus `mineableElements`: beide Namensformen (materialName und name)
    /// registrieren; mehrfach-Treffer (Carinite-Dup) union+dédup.
    pub fn build(elements: &[MineableElement]) -> Self {
        let mut by_normalized: BTreeMap<String, Vec<u32>> = BTreeMap::new();
        for el in elements {
            for key in [el.material_name.as_deref(), Some(el.name.as_str())]
                .into_iter()
                .flatten()
            {
                let key = normalize_material(key);
                if key.is_empty() {
                    continue;
                }
                let entry = by_normalized.entry(key).or_default();
                let mut vals = el.quality_bands.clone();
                vals.sort_unstable();
                vals.dedup();
                entry.append(&mut vals);
                entry.sort_unstable();
                entry.dedup();
            }
        }
        Self { by_normalized }
    }
}

/// Rohdaten-Client für SCMDB_DATA (raw GitHub — 2 Fetches pro Patch-Zyklus,
/// kein Pacing nötig).
pub struct ScmdbClient {
    client: reqwest::Client,
    base_url: String,
}

impl ScmdbClient {
    pub fn new(base_url: &str) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent(concat!("architects-overlay/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(ScWikiError::Http)?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
        })
    }

    async fn get(&self, path: &str) -> Result<reqwest::Response> {
        let url = format!("{}/{}", self.base_url, path.trim_start_matches('/'));
        let resp = self.client.get(&url).send().await?;
        if !resp.status().is_success() {
            return Err(ScWikiError::Status(resp.status().as_u16()));
        }
        Ok(resp)
    }

    /// Dateiname des Live-Mining-Files (`channels.live.files.mining_data`).
    pub async fn mining_filename(&self) -> Result<String> {
        let latest: LatestDto = self.get("data/latest.json").await?.json().await?;
        let file = latest.channels.live.files.mining_data;
        if file.trim().is_empty() {
            return Err(ScWikiError::Json(
                "latest.json: mining_data-Dateiname leer".into(),
            ));
        }
        Ok(file)
    }

    /// Mining-Daten parsen + in den Namens-Index überführen.
    pub async fn mining_index(&self) -> Result<MiningIndex> {
        let file = self.mining_filename().await?;
        let dto: MiningDto = self
            .get(&format!("data/{file}"))
            .await?
            .json()
            .await
            .map_err(|e| ScWikiError::Json(format!("mining_data ({file}): {e}")))?;
        Ok(MiningIndex::build(
            &dto.mineable_elements.into_values().collect::<Vec<_>>(),
        ))
    }
}

#[derive(Debug, Deserialize)]
struct MiningDto {
    /// dict uuid→Element (LIVE: 39 Einträge).
    #[serde(rename = "mineableElements")]
    mineable_elements: std::collections::BTreeMap<String, MineableElement>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(name: &str, material: Option<&str>, bands: &[u32]) -> MineableElement {
        MineableElement {
            name: name.to_owned(),
            material_name: material.map(str::to_owned),
            quality_bands: bands.to_vec(),
        }
    }

    /// Spec-Belege (Omar live 2026-11-20) — Bänder bleiben erhalten.
    #[test]
    fn build_indexes_both_name_forms() {
        let idx = MiningIndex::build(&[
            el("Taranite (Raw)", Some("Taranite"), &[310, 525, 1000]),
            el("Aphorite", None, &[280, 500, 1000]),
            el("Agricium (Ore)", Some("Agricium"), &[346, 588, 1000]),
        ]);
        assert_eq!(idx.bands_for("Taranite (Raw)"), vec![310, 525, 1000]);
        assert_eq!(idx.bands_for("  taranite "), vec![310, 525, 1000]);
        assert_eq!(idx.bands_for("Taranite"), vec![310, 525, 1000]);
        assert_eq!(idx.bands_for("Aphorite"), vec![280, 500, 1000]);
        assert_eq!(idx.bands_for("agricium (ore)"), vec![346, 588, 1000]);
        assert_eq!(idx.bands_for("Agricium"), vec![346, 588, 1000]);
        // Kein Match → leer.
        assert!(idx.bands_for("Unobtainium").is_empty());
    }

    /// Normalize: Suffixe (Ore)/(Raw)/(Pure) werden gestrippt, Case/Space egal.
    #[test]
    fn normalize_material_strips_suffixes() {
        assert_eq!(normalize_material("Taranite (Raw)"), "taranite");
        assert_eq!(normalize_material("  BORASE (Ore) "), "borase");
        assert_eq!(normalize_material("Carinite (Pure)"), "carinite");
        assert_eq!(normalize_material("Aphorite"), "aphorite");
        assert_eq!(normalize_material("  hadanite\t"), "hadanite");
    }

    /// Carinite-Dup (LIVE: „Carinite" × 2 + „Carinite (Pure)"): Union ohne
    /// Duplikate — identische Bänder kollabieren, gemischte vereinigen.
    #[test]
    fn build_unions_duplicate_keys() {
        let idx = MiningIndex::build(&[
            el("Carinite", None, &[273, 554, 1000]),
            el("Carinite", None, &[273, 554, 1000]),
            el("Carinite (Pure)", None, &[273, 647, 1000]),
        ]);
        assert_eq!(idx.bands_for("Carinite"), vec![273, 554, 647, 1000]);
        assert_eq!(idx.bands_for("Carinite (Pure)"), vec![273, 554, 647, 1000]);
    }

    /// Bänder werden sortiert + dedupliziert, auch wenn die Quelle unsauber ist.
    #[test]
    fn build_sorts_and_dedups_bands() {
        let idx = MiningIndex::build(&[el("Test", None, &[1000, 346, 346, 588])]);
        assert_eq!(idx.bands_for("test"), vec![346, 588, 1000]);
    }
}

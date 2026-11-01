//! Releases-API-Client (Karte Umsetzung 1): EIN GET auf
//! `repos/{repo}/releases/latest` — sonst KEIN Netzverkehr
//! (kein Telemetrie/Phoning, Karte Umsetzung 7).
//!
//! Zwei API-Formen (S5 Endpoint-Switch): GitHub (`https://api.github.com`,
//! Pfad ohne `/api/v1`) und Gitea (`https://git.sebo-home.de`, Pfad mit
//! `/api/v1`). Die JSON-Form ist identisch (tag_name/body/assets mit
//! browser_download_url), daher reicht der URL-Builder-Unterschied.
//!
//! HTTPS-only (Umsetzung 5): eine Base-URL ohne `https://` wird VOR dem
//! Request abgelehnt — unabhängig davon, was in der Config steht.

use serde::Deserialize;

use super::{Result, UpdaterError};

/// Ein Release-Asset (Datei) der Gitea-API.
#[derive(Debug, Clone)]
pub struct Asset {
    pub name: String,
    pub download_url: String,
    pub size: Option<u64>,
}

/// Vereinfachtes latest-Release (nur die Felder, die der Updater braucht).
#[derive(Debug, Clone)]
pub struct Release {
    pub tag_name: String,
    pub body: String,
    pub assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct RawAsset {
    name: String,
    #[serde(rename = "browser_download_url")]
    download_url: String,
    size: Option<u64>,
}

#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

/// GitHub-API-Basis? (`api.github.com` bzw. github.com-Host.)
fn is_github(base: &str) -> bool {
    // Host-Anteil vor dem ersten Pfad-Segment prüfen.
    let rest = base.strip_prefix("https://").unwrap_or(base);
    let host = rest.split('/').next().unwrap_or(rest);
    host == "api.github.com" || host == "github.com"
}

/// Baue die API-URL; lehnt alles ab, was nicht https:// ist (Umsetzung 5).
pub fn releases_url(base_url: &str, repo: &str) -> Result<String> {
    let base = base_url.trim().trim_end_matches('/');
    if !base.starts_with("https://") {
        return Err(UpdaterError::InsecureUrl(base_url.to_owned()));
    }
    let repo = repo.trim().trim_matches('/');
    if repo.is_empty() || !repo.contains('/') {
        return Err(UpdaterError::Malformed(format!(
            "Repo-Angabe unlesbar: {repo}"
        )));
    }
    if is_github(base) {
        Ok(format!("{base}/repos/{repo}/releases/latest"))
    } else {
        Ok(format!("{base}/api/v1/repos/{repo}/releases/latest"))
    }
}

/// GET latest release. Fehlerpfade: kein Netz / Rate-Limit (429) / sonstige
/// HTTP-Fehler → `UpdaterError::Network` — der alte Zustand bleibt intakt,
/// der Check zeigt dann einfach „Prüfung fehlgeschlagen“ in der Note.
pub async fn fetch_latest(base_url: &str, repo: &str) -> Result<Release> {
    let url = releases_url(base_url, repo)?;
    let client = reqwest::Client::builder()
        .user_agent(concat!(
            "architects-overlay-updater/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|e| UpdaterError::Network(e.to_string()))?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| UpdaterError::Network(e.to_string()))?;
    let status = resp.status();
    if status.as_u16() == 429 {
        return Err(UpdaterError::Network("update.reason_rate_limit".into()));
    }
    if !status.is_success() {
        return Err(UpdaterError::Network(format!("HTTP {status}")));
    }
    let raw: RawRelease = resp
        .json()
        .await
        .map_err(|e| UpdaterError::Malformed(e.to_string()))?;
    if raw.tag_name.trim().is_empty() {
        return Err(UpdaterError::Malformed("leerer tag_name".into()));
    }
    Ok(Release {
        tag_name: raw.tag_name,
        body: raw.body,
        assets: raw
            .assets
            .into_iter()
            .map(|a| Asset {
                name: a.name,
                download_url: a.download_url,
                size: a.size,
            })
            .collect(),
    })
}

/// GET eines rohen Texts (Sidecar `SHA256SUMS`). HTTPS-only wie der Rest.
pub async fn fetch_text(url: &str) -> Result<String> {
    if !url.starts_with("https://") {
        return Err(UpdaterError::InsecureUrl(url.to_owned()));
    }
    let client = reqwest::Client::builder()
        .user_agent(concat!(
            "architects-overlay-updater/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|e| UpdaterError::Network(e.to_string()))?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| UpdaterError::Network(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(UpdaterError::Network(format!("HTTP {}", resp.status())));
    }
    resp.text()
        .await
        .map_err(|e| UpdaterError::Network(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_only() {
        assert!(releases_url("http://git.example.com", "sebo/architects-overlay").is_err());
        assert!(releases_url("git.sebo-home.de", "sebo/architects-overlay").is_err());
        let url = releases_url("https://git.sebo-home.de/", "sebo/architects-overlay").unwrap();
        assert_eq!(
            url,
            "https://git.sebo-home.de/api/v1/repos/sebo/architects-overlay/releases/latest"
        );
    }

    #[test]
    fn repo_shape() {
        assert!(releases_url("https://git.sebo-home.de", "norrepo").is_err());
        // GitHub-Form (S5): ohne /api/v1, Default-Repo des öffentlichen Mirrors.
        let gh = releases_url("https://api.github.com", "SeBoOne/architects-overlay").unwrap();
        assert_eq!(
            gh,
            "https://api.github.com/repos/SeBoOne/architects-overlay/releases/latest"
        );
        assert_eq!(
            releases_url("https://api.github.com/", "SeBoOne/architects-overlay").unwrap(),
            "https://api.github.com/repos/SeBoOne/architects-overlay/releases/latest"
        );
        assert!(releases_url("https://git.sebo-home.de", "").is_err());
    }
}

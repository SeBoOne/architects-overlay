//! # updater — Update-Check + on-demand Update für das Overlay
//!
//! UC t_d323be7e, TG: Check ist EIN GET auf `latest.json` (Redirect-Pfad
//! `releases/latest/download/`, NICHT rate-limited; Fallback: Releases-API)
//! — sonst nichts (kein Telemetrie/Phoning, Karte Umsetzung 7). Downloads passieren nur auf expliziten Nutzerwunsch
//! („Jetzt aktualisieren“) und werden vor dem Installieren per SHA256
//! gegen die Release-Checksummen verifiziert (Umsetzung 5) — kein
//! Trust-on-Download ohne Check. Fehlerpfade lassen die laufende
//! Version immer unangetastet (Umsetzung 6).
//!
//! Layout (layered wie der Rest des Workspaces, keine UI-Abhängigkeiten):
//! - `version`  semver-Vergleich inkl. Pre-release-Suffixe (Umsetzung 1)
//! - `gitea`    HTTPS-only GET /repos/{repo}/releases/latest (Umsetzung 1)
//! - `integrity` SHA256-Datei-Hash + Checksums-Parsing (Umsetzung 5)
//! - `plan`     Asset-Auswahl nach Plattform/Installationsart (Umsetzung 3/4)
//! - `download` Streaming-Download mit Größenlimit + Tempfile (Umsetzung 6)
//! - `perform_download` Windows: setup.exe-Pfad zurückgeben (NSIS
//!   übernimmt das Selbst-Erstzen); Linux: AppImage atomar ersetzen

mod download;
mod gitea;
mod integrity;
mod latest_json;
mod plan;
mod version;

pub use download::{MAX_DOWNLOAD_BYTES, download_asset};
pub use gitea::{Asset, Release, fetch_latest};
pub use integrity::{CHECKSUMS_ASSET, parse_checksums, sha256_file};
pub use plan::{
    AppKind, Plan, bare_binary_exe, linux_update_dir, plan_for_release, windows_update_dir,
};
pub use version::{is_newer, parse_version};

/// Fehler eines Update-Vorgangs. T2 (t_c5904bcb): Display liefert
/// `{error_key, params}`-JSON — das Frontend übersetzt via trError
/// (Fallback-Kette gewählt → en → raw Key).
#[derive(Debug)]
pub enum UpdaterError {
    /// Repo-URL/Base-URL ist kein HTTPS — Umsetzung 5: HTTPS-only.
    InsecureUrl(String),
    /// Kein Netz / Gitea nicht erreichbar / HTTP-Fehler.
    Network(String),
    /// Release/Asset-Struktur unlesbar.
    Malformed(String),
    /// Keine Prüfsumme für das Asset auffindbar — bewusst REFUSAL statt
    /// Download (Karte: kein Trust-on-Download ohne Check).
    NoChecksum(String),
    /// SHA256-Mismatch — verifizierter Abbruch, temporäre Datei verworfen.
    ChecksumMismatch { expected: String, actual: String },
    /// Download größer als das Größenlimit (halber/Riesen-Download-Schutz).
    TooLarge { limit_bytes: u64 },
    /// Dateisystemfehler (Temp-Datei, Rename, Rechte).
    Io(String),
    /// Self-Update für die Installationsart nicht möglich (z. B. .deb —
    /// Umsetzung 4: keine root-Aktionen aus der App).
    NotSupported(String),
    /// Kein passendes Asset im Release für diese Plattform (T2: eigener
    /// Key statt Malformed-Freitext).
    NoAsset { tag: String },
}

impl std::fmt::Display for UpdaterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InsecureUrl(url) => {
                write!(
                    f,
                    "{}",
                    i18n_err::err_keyp(
                        "update.err_insecure_url",
                        serde_json::json!({ "url": url }),
                    )
                )
            }
            Self::Network(msg) => write!(
                f,
                "{}",
                i18n_err::err_keyp("update.err_network", serde_json::json!({ "msg": msg }),)
            ),
            Self::Malformed(msg) => write!(
                f,
                "{}",
                i18n_err::err_keyp("update.err_malformed", serde_json::json!({ "msg": msg }),)
            ),
            Self::NoChecksum(asset) => write!(
                f,
                "{}",
                i18n_err::err_keyp(
                    "update.err_no_checksum",
                    serde_json::json!({ "asset": asset }),
                )
            ),
            Self::ChecksumMismatch { expected, actual } => write!(
                f,
                "{}",
                i18n_err::err_keyp(
                    "update.err_checksum_mismatch",
                    serde_json::json!({ "expected": expected, "actual": actual }),
                )
            ),
            Self::TooLarge { limit_bytes } => {
                write!(
                    f,
                    "{}",
                    i18n_err::err_keyp(
                        "update.err_too_large",
                        serde_json::json!({ "limit": limit_bytes }),
                    )
                )
            }
            Self::Io(msg) => write!(
                f,
                "{}",
                i18n_err::err_keyp("update.err_io", serde_json::json!({ "msg": msg }),)
            ),
            Self::NotSupported(msg) => write!(
                f,
                "{}",
                i18n_err::err_keyp(
                    "update.err_not_supported",
                    serde_json::json!({ "reason": msg }),
                )
            ),
            Self::NoAsset { tag } => write!(
                f,
                "{}",
                i18n_err::err_keyp("update.err_no_asset", serde_json::json!({ "tag": tag }),)
            ),
        }
    }
}

impl std::error::Error for UpdaterError {}

pub type Result<T> = std::result::Result<T, UpdaterError>;

impl From<std::io::Error> for UpdaterError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

/// Ergebnis eines Update-Checks: aktuelle + neueste Version und der
/// auf dieser Plattform/Installation passende Plan.
#[derive(Debug)]
pub struct CheckResult {
    pub current_version: String,
    pub latest_version: String,
    pub tag: String,
    pub plan: Plan,
}

/// One-shot Update-Check (Umsetzung 1): GET latest release, Versionsvergleich
/// gegen `current_version` und Plan-Auswahl. Liefert `Ok(None)`, wenn keine
/// neuere Version existiert. Netzverkehr bleibt auf GETs begrenzt (Karte
/// Umsetzung 7): Release-GET + optional EIN GET auf das Sidecar-`SHA256SUMS`
/// (nur wenn der Release eines hat UND eine neuere Version vorliegt).
pub async fn check(
    base_url: &str,
    repo: &str,
    current_version: &str,
) -> Result<Option<CheckResult>> {
    // TG: Check OHNE Releases-API — latest.json über den Redirect-Pfad
    // `github.com/{repo}/releases/latest/download/latest.json` (nicht
    // rate-limited). Fallback: fehlt die Datei (alte Releases), API wie
    // bisher — bei 429 mit sauberer Rate-Limit-Meldung.
    let (release, json_checksums) = match latest_json::fetch_latest_json(base_url, repo).await? {
        Some((release, checksums)) => (release, Some(checksums)),
        None => (fetch_latest(base_url, repo).await?, None),
    };
    if !version::is_newer(current_version, &release.tag_name) {
        return Ok(None);
    }
    // Checksummen: bevorzugt aus latest.json (SHA256s pro Asset), sonst
    // Sidecar-`SHA256SUMS` einsammeln, sobald klar ist, dass ein Update
    // vorliegt (spart den zweiten GET im Normalfall "aktuell").
    let checksums_text = match json_checksums {
        Some(text) => text,
        None => match plan::checksums_asset(&release) {
            Some(asset) => match gitea::fetch_text(&asset.download_url).await {
                Ok(text) => Some(text),
                // Sidecar nicht erreichbar → Fallback auf Release-Body; erst
                // der spätere Download-Versuch schlägt ggf. mit NoChecksum fehl.
                Err(e) => {
                    log::warn!("updater: SHA256SUMS sidecar nicht lesbar: {e}");
                    None
                }
            },
            None => None,
        },
    };
    let plan = plan::plan_for_release(&release, checksums_text.as_deref()).ok_or_else(|| {
        UpdaterError::NoAsset {
            tag: release.tag_name.clone(),
        }
    })?;
    Ok(Some(CheckResult {
        current_version: current_version.to_owned(),
        latest_version: release.tag_name.trim_start_matches('v').to_owned(),
        tag: release.tag_name.clone(),
        plan,
    }))
}

/// Execute the install step of a plan (Umsetzung 3/4). Windows: downloads
/// setup.exe, verifies SHA256, returns the installer path — the app module
/// spawns it detached and exits (decision documented in docs/UPDATER.md).
/// Linux AppImage: downloads, verifies, atomically replaces the running
/// AppImage, returns the new path for a user-consented relaunch. Deb:
/// never installs (NotSupported) — the UI shows the apt hint instead.
pub async fn perform_download(plan: &Plan) -> Result<std::path::PathBuf> {
    match plan {
        Plan::WindowsNsis {
            asset_name,
            asset_url,
            checksum,
        } => {
            let path =
                download_asset(asset_url, &windows_update_dir()?, MAX_DOWNLOAD_BYTES).await?;
            integrity::verify_or_fail(&path, checksum.as_deref(), asset_name)?;
            Ok(path)
        }
        Plan::LinuxAppImage {
            asset_name,
            asset_url,
            checksum,
            ..
        } => {
            let path = download_asset(asset_url, &linux_update_dir()?, MAX_DOWNLOAD_BYTES).await?;
            integrity::verify_or_fail(&path, checksum.as_deref(), asset_name)?;
            Ok(path)
        }
        Plan::LinuxBin {
            asset_name,
            asset_url,
            checksum,
            exe_path,
        } => {
            // TH t_df9646a3: Download ZUSAMMEN mit der exe (gleiche Dateisystem —
            // das atomare Rename über die laufende Datei ist nur garantiert im
            // selben Dateisystem; der eigentliche Swap passiert beim
            // aufrufenden Install-Schritt in der App-Schicht).
            let dir = exe_path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            let path = download_asset(asset_url, &dir, MAX_DOWNLOAD_BYTES).await?;
            integrity::verify_or_fail(&path, checksum.as_deref(), asset_name)?;
            Ok(path)
        }
        Plan::DebHint { .. } => Err(UpdaterError::NotSupported("update.reason_deb_apt".into())),
    }
}

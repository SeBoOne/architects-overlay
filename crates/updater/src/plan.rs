//! Asset-Auswahl + Installationsplan (Karte Umsetzung 3/4).
//!
//! Plattform-Matrix:
//! - Windows: NSIS-Setup (`*-setup.exe`) — NSIS-Installer übernimmt das
//!   Selbst-Erstzen (die laufende exe kann sich nicht selbst ersetzen).
//!   Ausführungsentscheidung (dokumentiert in docs/UPDATER.md): die App
//!   startet das Setup SILENT (`/S`) als CHILD-PROZESS und beendet sich
//!   dann sauber selbst; NSIS schließt eine laufende Instanz ab und
//!   installiert. Kein auto-restart — der Nutzer startet das Overlay
//!   selbst neu (Startmenü-Eintrag des Setups).
//! - Linux AppImage (env `APPIMAGE` gesetzt oder `.AppImage`-Pfad): das
//!   neue AppImage wird an den Pfad der laufenden Datei atomar ersetzt
//!   (rename old → rename new → exec-Rechte), Nutzerdaten unangetastet.
//! - Linux .deb (kein AppImage, Binary unter /usr|/opt): KEIN Self-Update
//!   (Umsetzung 4: keine root-Aktionen aus der App) — Hint mit apt-Befehl.

use std::path::PathBuf;

use super::gitea::{Asset, Release};
use super::integrity::CHECKSUMS_ASSET;
use super::integrity::find_checksum;

/// Welche Installationsart liegt vor (für Plan + UI-Text).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppKind {
    Nsis,
    AppImage,
    Deb,
}

/// Der für diese Plattform passende Update-Plan.
#[derive(Debug, Clone)]
pub enum Plan {
    /// Windows: setup.exe herunterladen, verifizieren, ausführen (NSIS).
    WindowsNsis {
        asset_name: String,
        asset_url: String,
        checksum: Option<String>,
    },
    /// Linux AppImage: Datei herunterladen, verifizieren, atomar am
    /// laufenden AppImage-Pfad ersetzen.
    LinuxAppImage {
        asset_name: String,
        asset_url: String,
        checksum: Option<String>,
        appimage_path: PathBuf,
    },
    /// Linux .deb: kein Self-Update — Hint-Text mit apt-Befehl für die UI.
    DebHint {
        asset_name: String,
        asset_url: String,
        deb_path_hint: String,
    },
}

impl AppKind {
    /// Kurzname für die UI („nsis“ / „appimage“ / „deb“).
    pub fn as_str(self) -> &'static str {
        match self {
            AppKind::Nsis => "nsis",
            AppKind::AppImage => "appimage",
            AppKind::Deb => "deb",
        }
    }
}

impl Plan {
    /// Kurzname für die UI.
    pub fn kind(&self) -> AppKind {
        match self {
            Plan::WindowsNsis { .. } => AppKind::Nsis,
            Plan::LinuxAppImage { .. } => AppKind::AppImage,
            Plan::DebHint { .. } => AppKind::Deb,
        }
    }

    /// Haupt-Asset dieses Plans (Name + URL) — für Chip/Notiz und Check.
    pub fn asset(&self) -> (&str, &str) {
        match self {
            Plan::WindowsNsis {
                asset_name,
                asset_url,
                ..
            }
            | Plan::LinuxAppImage {
                asset_name,
                asset_url,
                ..
            }
            | Plan::DebHint {
                asset_name,
                asset_url,
                ..
            } => (asset_name, asset_url),
        }
    }

    pub fn checksum(&self) -> Option<&str> {
        match self {
            Plan::WindowsNsis { checksum, .. } | Plan::LinuxAppImage { checksum, .. } => {
                checksum.as_deref()
            }
            Plan::DebHint { .. } => None,
        }
    }

    /// UI-Hinweis für .deb-Installationen (Umsetzung 4 — kein root aus der
    /// App). Andere Plans: `None` (die App installiert selbst).
    pub fn deb_hint(&self) -> Option<String> {
        if let Plan::DebHint {
            asset_url,
            deb_path_hint,
            ..
        } = self
        {
            Some(format!(
                "Self-Update ist für .deb-Installationen nicht möglich (keine root-Aktionen aus der App).\
                 \nUpdate per Download + apt:\
                 \n  curl -Lo {deb_path_hint} \"{asset_url}\"\
                 \n  sudo apt install {deb_path_hint}"
            ))
        } else {
            None
        }
    }
}

/// Prüfe, ob `asset` zum gesuchten Muster passt (endet mit Suffix).
fn find_asset<'a>(release: &'a Release, ends_with: &str) -> Option<&'a Asset> {
    release
        .assets
        .iter()
        .find(|a| a.name.to_lowercase().ends_with(ends_with))
}

/// Baue den Plan für das laufende System. `checksums_text` ist der Inhalt
/// des Sidecar-`SHA256SUMS`-Assets (falls das Release eines hat — die
/// Check-Schicht holt es nur, wenn es existiert) bzw. `None`. Die Prüfung
/// wählt die Checksumme aus Sidecar (Bevorzugung) oder Release-Body; fehlt
/// beides für das Asset, schlägt der spätere Download bewusst mit
/// `NoChecksum` fehl (kein Trust-on-Download ohne Check).
pub fn plan_for_release(release: &Release, checksums_text: Option<&str>) -> Option<Plan> {
    if cfg!(windows) {
        let asset = find_asset(release, "-setup.exe")?;
        return Some(Plan::WindowsNsis {
            checksum: checksum_for(&asset.name, release, checksums_text),
            asset_name: asset.name.clone(),
            asset_url: asset.download_url.clone(),
        });
    }
    if cfg!(target_os = "linux") {
        if let Some(path) = appimage_path() {
            let asset = find_asset(release, ".appimage")?;
            return Some(Plan::LinuxAppImage {
                checksum: checksum_for(&asset.name, release, checksums_text),
                asset_name: asset.name.clone(),
                asset_url: asset.download_url.clone(),
                appimage_path: path,
            });
        }
        if deb_like_install() {
            let asset = find_asset(release, ".deb")?;
            return Some(Plan::DebHint {
                asset_name: asset.name.clone(),
                asset_url: asset.download_url.clone(),
                deb_path_hint: format!("/tmp/{}", asset.name),
            });
        }
    }
    None
}

fn checksum_for(
    asset_name: &str,
    release: &Release,
    checksums_text: Option<&str>,
) -> Option<String> {
    find_checksum(asset_name, checksums_text, &release.body)
}

/// Pfad des LAUFENDEN AppImage. Tauri setzt `APPIMAGE` für AppImage-Starts;
/// als Fallback zählt ein aktueller exe-Pfad, der auf `.AppImage` endet.
pub fn appimage_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("APPIMAGE") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    let exe = std::env::current_exe().ok()?;
    if exe.extension()?.to_str()?.eq_ignore_ascii_case("appimage") {
        return Some(exe);
    }
    None
}

/// Läuft die App als deb-/Systeminstallation (kein AppImage)? Heuristik:
/// Binary unter /usr (deb-Paketpfad) und kein AppImage.
pub fn deb_like_install() -> bool {
    if appimage_path().is_some() {
        return false;
    }
    let Some(exe) = std::env::current_exe()
        .ok()
        .and_then(|p| p.canonicalize().ok())
    else {
        return false;
    };
    exe.starts_with("/usr") || exe.starts_with("/opt")
}

/// Zielverzeichnis für den Windows-Setup-Download (Temp-Unterordner).
pub fn windows_update_dir() -> super::Result<PathBuf> {
    let dir = std::env::temp_dir().join("architects-overlay-update");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Zielverzeichnis für den Linux-Download: ZUSAMMEN mit dem AppImage (das
/// atomare Replace ist nur garantiert im selben Dateisystem).
pub fn linux_update_dir() -> super::Result<PathBuf> {
    let Some(path) = appimage_path() else {
        return Err(super::UpdaterError::NotSupported(
            "Kein laufendes AppImage gefunden".into(),
        ));
    };
    let dir = path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Für die Check-Schicht: finde das Sidecar-Asset (SHA256SUMS) im Release.
pub fn checksums_asset(release: &Release) -> Option<&Asset> {
    release
        .assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case(CHECKSUMS_ASSET))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(assets: &[(&str, &str)]) -> Release {
        Release {
            tag_name: "v0.5.14".into(),
            body: String::new(),
            assets: assets
                .iter()
                .map(|(n, u)| Asset {
                    name: n.to_string(),
                    download_url: u.to_string(),
                    size: None,
                })
                .collect(),
        }
    }

    #[test]
    fn finds_assets_by_suffix() {
        let rel = release(&[
            ("architects-overlay_0.5.14_x64-setup.exe", "u1"),
            ("architects-overlay_0.5.14_amd64.AppImage", "u2"),
            ("SHA256SUMS", "u3"),
        ]);
        assert!(find_asset(&rel, "-setup.exe").is_some());
        assert!(find_asset(&rel, ".appimage").is_some());
        assert!(checksums_asset(&rel).is_some());
        let empty = release(&[]);
        assert!(find_asset(&empty, "-setup.exe").is_none());
        assert!(checksums_asset(&empty).is_none());
    }

    #[test]
    fn plan_none_without_matching_install() {
        // Auf einem Nicht-AppImage/dev-Host (kein APPIMAGE-Env, Binary nicht
        // unter /usr|/opt) gibt es keinen Linux-Plan — kein Update-Hinweis,
        // statt ein falscher Installationspfad.
        let rel = release(&[("architects-overlay_0.5.14_amd64.AppImage", "u2")]);
        if !cfg!(windows) && super::appimage_path().is_none() && !super::deb_like_install() {
            assert!(plan_for_release(&rel, None).is_none());
        }
    }
}

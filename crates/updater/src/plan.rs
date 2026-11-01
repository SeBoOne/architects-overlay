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
//! - Linux Bare-Binary (TH t_df9646a3: exe heißt `architects-overlay` und
//!   ist weder AppImage noch deb-Kontext, Standard: `~/.local/bin/…`): das
//!   rohe Binary-Asset (`*_linux64`) herunterladen → SHA256 → exec-Rechte
//!   → atomares Rename über die laufende Datei (rename über die aktive exe
//!   geht auf Linux) — KEIN Auto-Restart, die App zeigt den Hinweis
//!   „Neustart des Overlays anwenden lassen“ (systemd/Overlay schließen+öffnen).

use std::path::PathBuf;

use super::integrity::CHECKSUMS_ASSET;
use super::integrity::find_checksum;
use super::release_api::{Asset, Release};

/// Welche Installationsart liegt vor (für Plan + UI-Text).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppKind {
    Nsis,
    AppImage,
    Deb,
    /// Linux Bare-Binary (z. B. cp nach ~/.local/bin) — TH t_df9646a3.
    LinuxBin,
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
    /// Linux Bare-Binary (TH t_df9646a3): rohes Binary-Asset herunterladen,
    /// verifizieren, chmod +x und atomar über die laufende exe renamen
    /// (kein Auto-Restart — die App zeigt nur den Neustart-Hinweis).
    LinuxBin {
        asset_name: String,
        asset_url: String,
        checksum: Option<String>,
        exe_path: PathBuf,
    },
}

impl AppKind {
    /// Kurzname für die UI („nsis“ / „appimage“ / „deb“).
    pub fn as_str(self) -> &'static str {
        match self {
            AppKind::Nsis => "nsis",
            AppKind::AppImage => "appimage",
            AppKind::Deb => "deb",
            AppKind::LinuxBin => "linux-bin",
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
            Plan::LinuxBin { .. } => AppKind::LinuxBin,
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
            }
            | Plan::LinuxBin {
                asset_name,
                asset_url,
                ..
            } => (asset_name, asset_url),
        }
    }

    pub fn checksum(&self) -> Option<&str> {
        match self {
            Plan::WindowsNsis { checksum, .. }
            | Plan::LinuxAppImage { checksum, .. }
            | Plan::LinuxBin { checksum, .. } => checksum.as_deref(),
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
            // T2 (t_c5904bcb): key-JSON — die UI übersetzt, Params sind
            // Download-URL + Zielpfad.
            Some(i18n_err::err_keyp(
                "update.deb_hint",
                serde_json::json!({ "url": asset_url, "path": deb_path_hint }),
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
        // TH t_df9646a3: Bare-Binary (z. B. cp nach ~/.local/bin) — weder
        // AppImage noch deb-Kontext. Asset: rohes Binary `*_linux64`.
        if let Some(exe_path) = bare_binary_exe() {
            let asset = find_asset(release, "linux64")?;
            return Some(Plan::LinuxBin {
                checksum: checksum_for(&asset.name, release, checksums_text),
                asset_name: asset.name.clone(),
                asset_url: asset.download_url.clone(),
                exe_path,
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

/// Bare-Binary-Installation (TH t_df9646a3): die laufende exe heißt
/// `architects-overlay` und ist weder AppImage noch deb-Kontext — der
/// Standardfall für eine `cp`-Installation nach `~/.local/bin`.
pub fn bare_binary_exe() -> Option<PathBuf> {
    if appimage_path().is_some() || deb_like_install() {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let file_name = exe.file_name()?.to_str()?.to_owned();
    if file_name != "architects-overlay" {
        return None;
    }
    Some(exe)
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
            "update.reason_no_appimage".into(),
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
    fn finds_linux64_asset_for_bare_binary() {
        // TH t_df9646a3: das rohe Binary-Asset `*_linux64` muss für den
        // Bare-Binary-Plan auffindbar sein (Suffix-Format wie die anderen).
        let rel = release(&[
            ("architects-overlay_0.5.20_amd64.AppImage", "u1"),
            ("architects-overlay_0.5.20_linux64", "u2"),
            ("SHA256SUMS", "u3"),
        ]);
        let asset = find_asset(&rel, "linux64").expect("linux64-Asset fehlt");
        assert!(asset.name.starts_with("architects-overlay_"));
        assert!(asset.name.ends_with("linux64"));
        assert_eq!(asset.download_url, "u2");
    }

    #[test]
    fn plan_bare_binary_branches() {
        // Auf einer Bare-Binary-Installation (kein APPIMAGE, kein /usr|/opt)
        // muss der Linux-Zweig den LinuxBin-Plan liefern — hier im
        // Test-Runner (cargo test läuft als `architects-overlay`-kombi-Name
        // nicht, daher der konditionale Schutz analog zu oben).
        let rel = release(&[
            ("architects-overlay_0.5.20_amd64.AppImage", "u1"),
            ("architects-overlay_0.5.20_linux64", "u2"),
        ]);
        if !cfg!(windows) && appimage_path().is_none() && !deb_like_install() {
            let exe = std::env::current_exe().unwrap();
            let is_bare = exe
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n == "architects-overlay")
                .unwrap_or(false);
            let plan = plan_for_release(&rel, None);
            if is_bare {
                // Bare-Binary-Installation → LinuxBin-Plan mit dem linux64-Asset.
                assert!(matches!(plan, Some(Plan::LinuxBin { .. })));
            } else {
                // Test-Runner heißt nicht architects-overlay → kein Plan
                // (kein falscher Installationspfad für dev/cargo-Läufe).
                assert!(plan.is_none());
            }
        }
    }

    #[test]
    fn bare_binary_exe_rejects_non_overlay_names() {
        // Der Test-Runner heißt nie `architects-overlay` → der Helper muss
        // None liefern (AppImage/deb-Kontext prüfen die Zwänge abseits).
        if appimage_path().is_none() && !deb_like_install() {
            assert!(bare_binary_exe().is_none());
        }
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

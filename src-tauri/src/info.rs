//! TW (t_f178b084): Info-Popup — das „ⓘ“-Chip im spot-status-Bereich der
//! Fußzeile (rechts neben Fokus/Klick) öffnet ein modales Info-Popup mit
//! Community-Disclaimer, UEX-/SC-Wiki-Badges und Repo-/Release-Links. Alle
//! externen Links darin öffnen im Systembrowser (tauri-plugin-opener, das
//! gleiche Muster wie die Deep-Link-Commands in [`crate::trade`]).
//!
//! Whitelist-Command: die UI nennt nur einen `kind`-String, die URL-Auswahl
//! bleibt komplett im Backend — dieselbe Sicherheits-Motivation wie
//! `open_created_listing` (niemals freie URLs aus dem Frontend öffnen).

use tauri::AppHandle;

/// Offizielle Ziel-URLs des Info-Popups (kinds). Fest verdrahtet, keine
/// Zusammensetzung aus Frontend-Eingaben:
/// - `uex`        → UEX Corp (Daten & Marktplatz, api.uexcorp.space)
/// - `uex_api`    → UEX API-Dokumentation (uexcorp.space/api)
/// - `scwiki`     → Star Citizen Wiki (star-citizen.wiki)
/// - `scwiki_api` → SC-Wiki API (api.star-citizen.wiki, Modul-Kompatibilität)
/// - `repo`       → öffentliches GitHub-Repo (SeBoOne/architects-overlay)
/// - `releases`   → Release-Seite (Changelog/Downloads)
const URLS: &[(&str, &str)] = &[
    ("uex", "https://uexcorp.space/"),
    ("uex_api", "https://uexcorp.space/api/"),
    ("scwiki", "https://star-citizen.wiki/"),
    ("scwiki_api", "https://api.star-citizen.wiki/"),
    ("repo", "https://github.com/SeBoOne/architects-overlay"),
    (
        "releases",
        "https://github.com/SeBoOne/architects-overlay/releases",
    ),
];

/// Einen Info-Popup-Link im Systembrowser öffnen. Reine externe Navigation —
/// nichts wird gelesen oder geschrieben.
#[tauri::command]
pub fn open_info_link(app: AppHandle, kind: String) -> Result<String, String> {
    let kind = kind.trim().to_ascii_lowercase();
    let url = URLS
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, u)| *u)
        .ok_or_else(|| format!("Unbekannter Info-Link: {kind}"))?;
    let _ = &app; // AppHandle nur für Signatur-Symmetrie mit den Geschwister-Commands
    tauri_plugin_opener::open_url(url, None::<&str>)
        .map_err(|e| format!("Browser nicht erreichbar: {e}"))?;
    Ok(url.to_string())
}

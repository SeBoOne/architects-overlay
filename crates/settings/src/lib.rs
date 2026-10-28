//! # settings
//!
//! Persists user configuration in the OS App-Config-Dir (via `dirs`): UEX API
//! key now, hotkey/window mode later. The API key is stored **only locally** —
//! never shipped, never logged, written with 0600 permissions on Unix.
//!
//! File: `<config-dir>/architects-overlay/config.toml`
//!
//! ```toml
//! [uex]
//! api_key = "…"
//!
//! [ui]
//! palette = "amber"        # amber | mfcd | bloom
//! detail_variant = "c"     # c | a
//! scale = 100              # K2 P12: 50..200 % (Schrötwerte 80/90/100/110/125)
//!
//! [data]
//! commodity_source = "wiki" # wiki | uex  (commodity PRICE source; wiki
//!                           # first, UEX as fallback for gaps)
//!
//! [shortcuts]
//! toggle = "Ctrl+Shift+U"      # C2a: Overlay-Toggle (kollisionsgeprüft)
//! click_through = "Ctrl+Shift+O" # Click-through-Umschalter
//! sync_omarchy = true          # C2b: Consent — Kürzel via OmaSettings an
//!                              # Hyprland-Binds andocken (Default: false,
//!                              # einmalige Zustimmung in der Settings-UI)
//! ```

use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// App sub-directory inside the OS config dir.
const APP_DIR: &str = "architects-overlay";
const CONFIG_FILE: &str = "config.toml";

/// K2 (t_5fe8eb4c, P12): Overlay font/icon scale in percent. Persisted in the
/// `ui` section; the frontend applies it as `zoom: var(--ui-scale)` on the
/// Shell. The UI offers 80/90/100/110/125 chips, the backend accepts any
/// value inside [`SCALE_MIN`..`SCALE_MAX`] — hand-edited out-of-range values
/// are normalized (`norm_scale`), never rejected, so the overlay boots.
pub const SCALE_DEFAULT: u16 = 100;
pub const SCALE_MIN: u16 = 50;
pub const SCALE_MAX: u16 = 200;

pub fn norm_scale(pct: u16) -> u16 {
    pct.clamp(SCALE_MIN, SCALE_MAX)
}

/// Colour palette of the overlay UI. Rendered as `data-palette` on `<html>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Palette {
    #[default]
    Amber,
    Mfcd,
    Bloom,
}

impl Palette {
    pub fn as_str(&self) -> &'static str {
        match self {
            Palette::Amber => "amber",
            Palette::Mfcd => "mfcd",
            Palette::Bloom => "bloom",
        }
    }

    pub fn from_name(name: &str) -> Option<Palette> {
        match name.trim().to_ascii_lowercase().as_str() {
            "amber" => Some(Palette::Amber),
            "mfcd" => Some(Palette::Mfcd),
            "bloom" => Some(Palette::Bloom),
            _ => None,
        }
    }
}

impl fmt::Display for Palette {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Price-detail layout variant in the search result view.
/// `C` = classic/compact default, `A` = alternative rendering (owned by the
/// parallel frontend task; this crate only carries the switch + CSS scope).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DetailVariant {
    #[default]
    C,
    A,
}

impl DetailVariant {
    pub fn as_str(&self) -> &'static str {
        match self {
            DetailVariant::C => "c",
            DetailVariant::A => "a",
        }
    }

    pub fn from_name(name: &str) -> Option<DetailVariant> {
        match name.trim().to_ascii_lowercase().as_str() {
            "c" => Some(DetailVariant::C),
            "a" => Some(DetailVariant::A),
            _ => None,
        }
    }
}

impl fmt::Display for DetailVariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Primary commodity PRICE source. `Wiki` (default): the wiki commodity
/// detail endpoint embeds the UEX terminal price table (`uex_prices.purchase`)
/// and is fetched first; UEX remains the fallback when the wiki row carries
/// no prices or the fetch fails. `Uex` reverts to the pure-UEX path (A/B
/// comparison switch).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CommoditySource {
    #[default]
    Wiki,
    Uex,
}

impl CommoditySource {
    pub fn as_str(&self) -> &'static str {
        match self {
            CommoditySource::Wiki => "wiki",
            CommoditySource::Uex => "uex",
        }
    }

    pub fn from_name(name: &str) -> Option<CommoditySource> {
        match name.trim().to_ascii_lowercase().as_str() {
            "wiki" => Some(CommoditySource::Wiki),
            "uex" => Some(CommoditySource::Uex),
            _ => None,
        }
    }
}

impl fmt::Display for CommoditySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---- C2a (t_6565fe8b): konfigurierbare globale Tastenkürzel ----------------
//
// Kanonische Quelle für beide Kürzel (Overlay-Toggle / Click-through). Der
// String ist plattformneutral im `global_hotkey`-Format (Mod-Tokens + Taste,
// '+'-separiert, case-insensitive parsebar — identisch zu dem Format, das
// tauri-plugin-global-shortcut per `Shortcut::from_str` akzeptiert). C2b
// (Hyprland-Binder) liest exakt diese Werte aus der config und übersetzt sie
// in Compositor-Syntax — die config bleibt die einzige kanonische Quelle.

/// Erstmigration: fehlende Settings → die historischen Phase-1-Defaults.
pub const DEFAULT_TOGGLE_SHORTCUT: &str = "Ctrl+Shift+U";
pub const DEFAULT_CLICK_SHORTCUT: &str = "Ctrl+Shift+O";

/// Hotkeys, die die Overlay-UI selbst verarbeitet, solange das Overlay
/// fokussiert ist (onKey in +page.svelte). Die In-App-Handler prüfen KEINE
/// Mod-Tasten (`k === 'F5'` feuert auch bei Strg+F5), ein globales Kürzel auf
/// denselben Tasten würde also IMMER beide Aktionen auslösen — daher als
/// Konflikt abgelehnt und in der Fehlermeldung als Konfliktliste sichtbar.
/// (F7–F11 sind frei.)
pub const RESERVED_INTERNAL_KEYS: &[(&str, &str)] = &[
    ("F1", "Modus Suche"),
    ("F2", "Marktplatz"),
    ("F3", "Inventar"),
    ("F4", "Hangar"),
    ("F5", "Preis-/Market-Sync"),
    ("F6", "Handelsrouten"),
    ("F12", "Einstellungen"),
    ("ENTER", "Bestätigen (Detail/Auswahl)"),
    ("TAB", "Angebote ⇄ Routen"),
    ("ARROWUP", "Listen-Navigation"),
    ("ARROWDOWN", "Listen-Navigation"),
    ("ARROWLEFT", "Detail zurück"),
    ("ARROWRIGHT", "Detail öffnen"),
    ("M", "Marktplatz verlassen"),
];

/// Mod-Tokens in kanonischer Ausgabereihenfolge.
const MOD_ORDER: [(&str, &[&str]); 4] = [
    // kanonischer Name: akzeptierte Aliases (u.a. deutsch für Hand-Edits)
    ("Ctrl", &["CTRL", "CONTROL", "STRG"]),
    ("Alt", &["ALT", "OPTION"]),
    ("Shift", &["SHIFT"]),
    (
        "Super",
        &["SUPER", "META", "WIN", "WINDOWS", "COMMAND", "CMD"],
    ),
];

/// Tasten, die `global_hotkey::parse_key` (und damit der Windows-/X11-
/// Registrierungspfad) als Haupttaste akzeptiert. Aliases so gehalten, dass
/// der Roundtrip parsebar bleibt.
const KEY_ALIASES: &[(&str, &[&str])] = &[
    ("Space", &["SPACE"]),
    ("Enter", &["ENTER", "RETURN"]),
    ("Tab", &["TAB"]),
    ("Escape", &["ESCAPE", "ESC"]),
    ("Backspace", &["BACKSPACE"]),
    ("Delete", &["DELETE"]),
    ("Home", &["HOME"]),
    ("End", &["END"]),
    ("Insert", &["INSERT"]),
    ("PageUp", &["PAGEUP", "PGUP"]),
    ("PageDown", &["PAGEDOWN", "PGDN"]),
    ("PrintScreen", &["PRINTSCREEN", "PRTSC"]),
    ("ScrollLock", &["SCROLLLOCK"]),
    ("CapsLock", &["CAPSLOCK"]),
    ("Pause", &["PAUSE", "PAUSEBREAK"]),
    ("ArrowUp", &["ARROWUP", "UP"]),
    ("ArrowDown", &["ARROWDOWN", "DOWN"]),
    ("ArrowLeft", &["ARROWLEFT", "LEFT"]),
    ("ArrowRight", &["ARROWRIGHT", "RIGHT"]),
    ("Backquote", &["BACKQUOTE", "`"]),
    ("Backslash", &["BACKSLASH", "\\"]),
    ("BracketLeft", &["BRACKETLEFT", "["]),
    ("BracketRight", &["BRACKETRIGHT", "]"]),
    ("Comma", &["COMMA", ","]),
    ("Equal", &["EQUAL", "="]),
    ("Minus", &["MINUS", "-"]),
    ("Period", &["PERIOD", "."]),
    ("Quote", &["QUOTE", "'"]),
    ("Semicolon", &["SEMICOLON", ";"]),
    ("Slash", &["SLASH", "/"]),
];

/// Eingabe (frei formatiert, z. B. `strg+shift+U`, `Meta+Space`, `Ctrl+F8`)
/// → kanonischer String `Ctrl+Alt+Shift+Super+Key` in fixer Mod-Reihenfolge.
/// Regeln: ≥1 Mod-Taste, genau 1 Haupttaste, keine unbekannten Tokens.
/// Wird NUR über diese Normalisierung persistiert; Fehler sind deutsch, weil
/// sie 1:1 im Settings-Feld sichtbar werden.
pub fn parse_shortcut_combo(input: &str) -> std::result::Result<String, String> {
    let tokens: Vec<String> = input
        .split('+')
        .map(|t| t.trim().to_ascii_uppercase())
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.is_empty() {
        return Err("Leere Kombination — Mod-Taste + normale Taste eingeben.".into());
    }

    let mut mods: Vec<&'static str> = Vec::new();
    let mut key: Option<String> = None;
    for tok in &tokens {
        if let Some((canon, _)) = MOD_ORDER
            .iter()
            .find(|(_, aliases)| aliases.contains(&tok.as_str()))
        {
            if !mods.contains(canon) {
                mods.push(canon);
            }
            continue;
        }
        if key.is_some() {
            return Err("Mehrere Haupttasten — genau eine normale Taste pro Kürzel.".into());
        }
        key = Some(normalize_key_token(tok)?);
    }

    if mods.is_empty() {
        return Err(
            "Mindestens eine Mod-Taste (Strg/Alt/Shift/Win) plus eine normale Taste.".into(),
        );
    }
    let Some(key) = key else {
        return Err("Es fehlt eine normale Haupttaste (nur Mod-Tasten reichen nicht).".into());
    };

    // Kanonische Reihenfolge Ctrl → Alt → Shift → Super, Haupttaste zuletzt.
    let mut out: Vec<String> = MOD_ORDER
        .iter()
        .filter(|(canon, _)| mods.contains(canon))
        .map(|(canon, _)| canon.to_string())
        .collect();
    out.push(key);
    Ok(out.join("+"))
}

/// Haupttasten-Normalisierung: `KeyU`→`U`, `Digit5`→`5`, Aliases→kanonisch,
/// A–Z / 0–9 / F1–F24 direkt.
fn normalize_key_token(tok: &str) -> std::result::Result<String, String> {
    if tok.len() == 1 && tok.as_bytes()[0].is_ascii_alphabetic() {
        return Ok(tok.to_string());
    }
    if tok.len() == 1 && tok.as_bytes()[0].is_ascii_digit() {
        return Ok(tok.to_string());
    }
    if let Ok(n) = tok[1..].parse::<u8>() {
        if tok.starts_with('F') && (1..=24).contains(&n) {
            return Ok(format!("F{n}"));
        }
    }
    // `KEYU`/`DIGIT5` schreiben dieselbe Taste explizit — akzeptieren.
    if let Some(rest) = tok.strip_prefix("KEY") {
        if rest.len() == 1 && rest.as_bytes()[0].is_ascii_alphabetic() {
            return Ok(rest.to_string());
        }
    }
    if let Some(rest) = tok.strip_prefix("DIGIT") {
        if rest.len() == 1 && rest.as_bytes()[0].is_ascii_digit() {
            return Ok(rest.to_string());
        }
    }
    if tok.starts_with("NUMPAD") || tok.starts_with("NUM") {
        return Err("Ziffernblock-Tasten werden nicht unterstützt — Haupttastatur nutzen.".into());
    }
    if let Some((canon, _)) = KEY_ALIASES
        .iter()
        .find(|(_, aliases)| aliases.contains(&tok))
    {
        return Ok(canon.to_string());
    }
    Err(format!(
        "Unbekannte Taste {tok:?} — nicht als globales Kürzel registrierbar."
    ))
}

/// Vollvalidierung eines Kürzels inkl. interner Konflikte (Konfliktliste in
/// der Fehlermeldung). Escape wird explizit abgelehnt (Esc = Capture-Abbruch
/// und UI-Zurück in jeder Ansicht). Alt+Zahl ist intern für die
/// Badge-Navigation belegt (Handler prüft nur altKey+Digit, ignoriert
/// weitere Mod-Tasten) — gleicher Grund wie die F-Tasten.
pub fn validate_shortcut_combo(input: &str) -> std::result::Result<String, String> {
    let canonical = parse_shortcut_combo(input)?;
    let key = canonical.rsplit('+').next().expect("canonical has a key");
    let key_upper = key.to_ascii_uppercase();

    if key_upper == "ESCAPE" || key_upper == "ESC" {
        return Err(
            "Escape ist als Kürzel nicht erlaubt (Abbrechen/Zurück in jeder Ansicht).".into(),
        );
    }
    if let Some((_, what)) = RESERVED_INTERNAL_KEYS.iter().find(|(k, _)| *k == key_upper) {
        return Err(format!(
            "Intern belegt: {key} ({what}) — im fokussierten Overlay würde beides feuern. Andere Taste wählen."
        ));
    }
    let mods_part = canonical[..canonical.len() - key.len() - 1].to_ascii_uppercase();
    if mods_part.contains("ALT") && key.len() == 1 && key.as_bytes()[0].is_ascii_digit() {
        return Err(format!(
            "Intern belegt: Alt+{key} (Angebots-Treffer {key} fokussieren) — andere Kombination wählen."
        ));
    }
    Ok(canonical)
}

/// Kanonischer String robust validieren (Erstmigration/Hand-Edit-Pfad):
/// ungültige Werte führen nie zum Absturz, sondern zu `fallback` — wie
/// `norm_scale` bei der Scale.
pub fn normalized_or(input: Option<&str>, fallback: &str) -> String {
    input
        .and_then(|s| validate_shortcut_combo(s).ok())
        .unwrap_or_else(|| fallback.to_owned())
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RawConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    uex: Option<UexSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ui: Option<UiSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data: Option<DataSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shortcuts: Option<ShortcutsSection>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct UexSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    api_key: Option<String>,
    /// M2a-Fix (t_2f197d1c): persönlicher UEX-Secret-Key (40 Zeichen) für
    /// user-scoped Reads (`/user_trades`, `/user`, eigene `marketplace_listings`).
    /// Wird NIE im Snapshot/Logs ausgegeben — nur bool-only (`secret_key_set`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secret_key: Option<String>,
    /// M2a-Fix: UEX-Benutzername (exakt wie auf UEX registriert) für den
    /// `marketplace_listings?username=`-Pull des eigenen Shops.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    username: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct DataSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    commodity_source: Option<CommoditySource>,
}

/// C2a: globale Kürzel. Als Strings gespeichert (kanonisch normalisiert),
/// damit C2b (Hyprland-Binder) sie ohne Rust-Kenntnis lesen kann.
/// C2b: `sync_omarchy` ist die EINMALIGE Zustimmung des Nutzers, dass die
/// App die OmaSettings-Bindungen des Systems verändern darf. Fehlend =
/// false — die App schreibt ohne gesetztes Flag niemals in Systemkonfig.
#[derive(Debug, Default, Serialize, Deserialize)]
struct ShortcutsSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    toggle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    click_through: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sync_omarchy: Option<bool>,
}

/// UI preferences (palette, detail variant). Fields are optional in the file
/// so older configs stay valid; missing values fall back to the enum default.
#[derive(Debug, Default, Serialize, Deserialize)]
struct UiSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    palette: Option<Palette>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    detail_variant: Option<DetailVariant>,
    // K2 (t_5fe8eb4c, P12): Schrift-/Icon-Skalierung in Prozent (F12-View).
    // Normalized via `norm_scale` on load so a hand-edited out-of-range value
    // cannot break the layout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scale: Option<u16>,
}

/// Settings I/O failures.
#[derive(Debug)]
pub enum SettingsError {
    /// The OS config directory could not be resolved.
    NoConfigDir,
    /// Reading or writing the config file failed.
    Io(String),
    /// The config file exists but is not valid TOML/schema.
    Parse(String),
}

impl std::fmt::Display for SettingsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingsError::NoConfigDir => write!(f, "config directory not available"),
            SettingsError::Io(msg) => write!(f, "config file I/O error: {msg}"),
            SettingsError::Parse(msg) => write!(f, "invalid config file: {msg}"),
        }
    }
}

impl std::error::Error for SettingsError {}

pub type Result<T> = std::result::Result<T, SettingsError>;

/// The application settings. Load once, mutate, save.
#[derive(Debug)]
pub struct Settings {
    path: PathBuf,
    uex_api_key: Option<String>,
    /// M2a-Fix (t_2f197d1c): persönlicher UEX-Secret-Key (user-scoped Reads).
    uex_secret_key: Option<String>,
    /// M2a-Fix: UEX-Benutzername für `marketplace_listings?username=`-Pull.
    uex_username: Option<String>,
    palette: Palette,
    detail_variant: DetailVariant,
    commodity_source: CommoditySource,
    scale: u16,
    /// C2a: kanonische Kürzel-Strings (siehe `parse_shortcut_combo`).
    shortcut_toggle: String,
    shortcut_click_through: String,
    /// C2b: Consent für die OmaSettings-Bind-Sync (Default false — ohne
    /// Zustimmung schreibt die App nie in die Systemkonfiguration).
    shortcut_sync_omarchy: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            path: PathBuf::from(CONFIG_FILE),
            uex_api_key: None,
            uex_secret_key: None,
            uex_username: None,
            palette: Palette::default(),
            detail_variant: DetailVariant::default(),
            commodity_source: CommoditySource::default(),
            scale: SCALE_DEFAULT,
            shortcut_toggle: DEFAULT_TOGGLE_SHORTCUT.to_owned(),
            shortcut_click_through: DEFAULT_CLICK_SHORTCUT.to_owned(),
            shortcut_sync_omarchy: false,
        }
    }
}

impl Settings {
    /// Path to the config file in the OS App-Config-Dir, resolved via `dirs`.
    /// May be explicitly injected (tests) via [`Settings::with_path`].
    pub fn config_path() -> Result<PathBuf> {
        let base = dirs::config_dir().ok_or(SettingsError::NoConfigDir)?;
        Ok(base.join(APP_DIR).join(CONFIG_FILE))
    }

    /// Load settings from the standard config path; missing file → defaults.
    pub fn load() -> Result<Settings> {
        let path = Self::config_path()?;
        Self::load_from(&path)
    }

    /// Load from an explicit path (missing file → defaults).
    pub fn load_from(path: &std::path::Path) -> Result<Settings> {
        let raw = match std::fs::read_to_string(path) {
            Ok(content) => toml::from_str::<RawConfig>(&content)
                .map_err(|e| SettingsError::Parse(e.to_string()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => RawConfig::default(),
            Err(e) => return Err(SettingsError::Io(e.to_string())),
        };
        Ok(Settings {
            path: path.to_path_buf(),
            uex_api_key: raw.uex.as_ref().and_then(|u| u.api_key.clone()),
            uex_secret_key: raw.uex.as_ref().and_then(|u| u.secret_key.clone()),
            uex_username: raw.uex.as_ref().and_then(|u| u.username.clone()),
            palette: raw.ui.as_ref().and_then(|u| u.palette).unwrap_or_default(),
            detail_variant: raw
                .ui
                .as_ref()
                .and_then(|u| u.detail_variant)
                .unwrap_or_default(),
            commodity_source: raw
                .data
                .as_ref()
                .and_then(|d| d.commodity_source)
                .unwrap_or_default(),
            scale: raw
                .ui
                .as_ref()
                .and_then(|u| u.scale)
                .map(norm_scale)
                .unwrap_or(SCALE_DEFAULT),
            // Erstmigration: fehlende/ungültige Kürzel → Defaults (U/O).
            shortcut_toggle: normalized_or(
                raw.shortcuts.as_ref().and_then(|s| s.toggle.as_deref()),
                DEFAULT_TOGGLE_SHORTCUT,
            ),
            shortcut_click_through: normalized_or(
                raw.shortcuts
                    .as_ref()
                    .and_then(|s| s.click_through.as_deref()),
                DEFAULT_CLICK_SHORTCUT,
            ),
            // C2b: fehlend = kein Consent — die App verhält sich exakt wie
            // vor C2b, bis der Nutzer in der Settings-UI zustimmt.
            shortcut_sync_omarchy: raw
                .shortcuts
                .as_ref()
                .and_then(|s| s.sync_omarchy)
                .unwrap_or(false),
        })
    }

    /// Settings handle pinned to a given file path (tests/embedded use).
    pub fn with_path(path: PathBuf) -> Settings {
        Settings {
            path,
            uex_api_key: None,
            uex_secret_key: None,
            uex_username: None,
            palette: Palette::default(),
            detail_variant: DetailVariant::default(),
            commodity_source: CommoditySource::default(),
            scale: SCALE_DEFAULT,
            shortcut_toggle: DEFAULT_TOGGLE_SHORTCUT.to_owned(),
            shortcut_click_through: DEFAULT_CLICK_SHORTCUT.to_owned(),
            shortcut_sync_omarchy: false,
        }
    }

    /// Configured UEX API key, or None.
    pub fn uex_api_key(&self) -> Option<&str> {
        self.uex_api_key.as_deref()
    }

    /// Set the UEX API key in memory (persist via [`Settings::save`]).
    pub fn set_uex_api_key(&mut self, key: Option<String>) {
        self.uex_api_key = key;
    }

    /// Configured UEX personal secret key, or None (M2a-Fix t_2f197d1c).
    /// Secret — darf NIE in Snapshot/Logs ausgegeben werden.
    pub fn uex_secret_key(&self) -> Option<&str> {
        self.uex_secret_key.as_deref()
    }

    /// Set the UEX secret key (persist via [`Settings::save`]).
    pub fn set_uex_secret_key(&mut self, key: Option<String>) {
        self.uex_secret_key = key;
    }

    /// Configured UEX username, or None (für `marketplace_listings?username=`).
    pub fn uex_username(&self) -> Option<&str> {
        self.uex_username.as_deref()
    }

    /// Set the UEX username (persist via [`Settings::save`]).
    pub fn set_uex_username(&mut self, name: Option<String>) {
        self.uex_username = name;
    }

    /// Active colour palette (default: Amber).
    pub fn palette(&self) -> Palette {
        self.palette
    }

    /// Set the colour palette in memory (persist via [`Settings::save`]).
    pub fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
    }

    /// Active price-detail variant (default: C).
    pub fn detail_variant(&self) -> DetailVariant {
        self.detail_variant
    }

    /// Set the price-detail variant in memory (persist via [`Settings::save`]).
    pub fn set_detail_variant(&mut self, variant: DetailVariant) {
        self.detail_variant = variant;
    }

    /// Primary commodity price source (default: Wiki — UEX fallback keeps
    /// gaps covered).
    pub fn commodity_source(&self) -> CommoditySource {
        self.commodity_source
    }

    /// Set the commodity price source in memory (persist via [`Settings::save`]).
    pub fn set_commodity_source(&mut self, source: CommoditySource) {
        self.commodity_source = source;
    }

    /// Overlay font/icon scale in % (K2 P12; default 100).
    pub fn scale(&self) -> u16 {
        self.scale
    }

    /// Set the font scale in memory; out-of-range values are normalized,
    /// never rejected (persist via [`Settings::save`]).
    pub fn set_scale(&mut self, pct: u16) {
        self.scale = norm_scale(pct);
    }

    /// Overlay-Toggle-Kürzel (kanonisch, z. B. `Ctrl+Shift+U`).
    pub fn shortcut_toggle(&self) -> &str {
        &self.shortcut_toggle
    }

    /// Click-through-Kürzel (kanonisch, z. B. `Ctrl+Shift+O`).
    pub fn shortcut_click_through(&self) -> &str {
        &self.shortcut_click_through
    }

    /// Set the toggle shortcut in memory (persist via [`Settings::save`]).
    /// Takes the VALIDATED canonical string from `validate_shortcut_combo` —
    /// this setter does not re-validate, the IPC command does.
    pub fn set_shortcut_toggle(&mut self, canonical: String) {
        self.shortcut_toggle = canonical;
    }

    /// Set the click-through shortcut in memory (persist via [`Settings::save`]).
    /// Same contract as [`Settings::set_shortcut_toggle`].
    pub fn set_shortcut_click_through(&mut self, canonical: String) {
        self.shortcut_click_through = canonical;
    }

    /// C2b: Consent-Flag — true erst nach einmaliger Zustimmung in der
    /// Settings-UI. Nur dann darf die App OmaSettings-Binds ändern.
    pub fn shortcut_sync_omarchy(&self) -> bool {
        self.shortcut_sync_omarchy
    }

    /// Set the OmaSettings-sync consent in memory (persist via [`Settings::save`]).
    pub fn set_shortcut_sync_omarchy(&mut self, on: bool) {
        self.shortcut_sync_omarchy = on;
    }

    /// Write the config file atomically (temp file + rename). On Unix the
    /// file is created with 0600 permissions so the API key stays user-only.
    pub fn save(&self) -> Result<()> {
        let raw = RawConfig {
            uex: Some(UexSection {
                api_key: self.uex_api_key.clone(),
                secret_key: self.uex_secret_key.clone(),
                username: self.uex_username.clone(),
            }),
            ui: Some(UiSection {
                palette: Some(self.palette),
                detail_variant: Some(self.detail_variant),
                scale: Some(self.scale),
            }),
            data: Some(DataSection {
                commodity_source: Some(self.commodity_source),
            }),
            shortcuts: Some(ShortcutsSection {
                toggle: Some(self.shortcut_toggle.clone()),
                click_through: Some(self.shortcut_click_through.clone()),
                sync_omarchy: Some(self.shortcut_sync_omarchy),
            }),
        };
        let content = toml::to_string_pretty(&raw)
            .map_err(|e| SettingsError::Parse(format!("serialize: {e}")))?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| SettingsError::Io(e.to_string()))?;
        }
        let tmp = self.path.with_extension("toml.tmp");
        std::fs::write(&tmp, content).map_err(|e| SettingsError::Io(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(&tmp, perms).map_err(|e| SettingsError::Io(e.to_string()))?;
        }
        std::fs::rename(&tmp, &self.path).map_err(|e| SettingsError::Io(e.to_string()))?;
        Ok(())
    }
}

/// Path to the per-user config file (App-Config-Dir / architects-overlay/config.toml).
pub fn config_path() -> Option<String> {
    Settings::config_path()
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

/// Returns the configured UEX API key, or None if unset.
pub fn uex_api_key() -> Option<String> {
    Settings::load().ok()?.uex_api_key().map(str::to_owned)
}

/// Persist the UEX API key.
pub fn set_uex_api_key(key: &str) {
    let mut settings = Settings::load().unwrap_or_else(|_| Settings::default());
    settings.path = Settings::config_path().unwrap_or_else(|_| PathBuf::from("config.toml"));
    settings.set_uex_api_key(Some(key.to_string()));
    if let Err(e) = settings.save() {
        log::warn!("settings: could not persist UEX API key: {e}");
    }
}

/// Returns the configured UEX personal secret key, or None if unset.
/// M2a-Fix (t_2f197d1c): user-scoped Reads (`/user_trades`, `/user`, eigene
/// `marketplace_listings`) verlangen zusätzlich zum App-Token diesen Key.
pub fn uex_secret_key() -> Option<String> {
    Settings::load().ok()?.uex_secret_key().map(str::to_owned)
}

/// Persist the UEX personal secret key (maskiert — nie loggen/ausgeben).
pub fn set_uex_secret_key(key: &str) {
    let mut settings = Settings::load().unwrap_or_else(|_| Settings::default());
    settings.path = Settings::config_path().unwrap_or_else(|_| PathBuf::from("config.toml"));
    settings.set_uex_secret_key(Some(key.to_string()));
    if let Err(e) = settings.save() {
        log::warn!("settings: could not persist UEX secret key: {e}");
    }
}

/// Returns the configured UEX username, or None if unset.
pub fn uex_username() -> Option<String> {
    Settings::load().ok()?.uex_username().map(str::to_owned)
}

/// Persist the UEX username (für `marketplace_listings?username=`-Pull).
pub fn set_uex_username(name: &str) {
    let mut settings = Settings::load().unwrap_or_else(|_| Settings::default());
    settings.path = Settings::config_path().unwrap_or_else(|_| PathBuf::from("config.toml"));
    settings.set_uex_username(Some(name.to_string()));
    if let Err(e) = settings.save() {
        log::warn!("settings: could not persist UEX username: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(tmp: &tempfile::TempDir, name: &str) -> PathBuf {
        tmp.path().join("nested").join(name)
    }

    #[test]
    fn missing_file_yields_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let settings = Settings::load_from(&tmp_path(&tmp, "config.toml")).unwrap();
        assert_eq!(settings.uex_api_key(), None);
        assert_eq!(settings.palette(), Palette::Amber);
        assert_eq!(settings.detail_variant(), DetailVariant::C);
        assert!(!settings.path.exists());
    }

    #[test]
    fn palette_and_variant_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp_path(&tmp, "config.toml");

        let mut s = Settings::with_path(path.clone());
        s.set_palette(Palette::Bloom);
        s.set_detail_variant(DetailVariant::A);
        s.save().unwrap();

        let loaded = Settings::load_from(&path).unwrap();
        assert_eq!(loaded.palette(), Palette::Bloom);
        assert_eq!(loaded.detail_variant(), DetailVariant::A);
        assert_eq!(loaded.uex_api_key(), None);

        // Switch back to the defaults and verify persistence again.
        let mut s2 = Settings::load_from(&path).unwrap();
        s2.set_palette(Palette::Mfcd);
        s2.set_detail_variant(DetailVariant::C);
        s2.save().unwrap();
        let loaded2 = Settings::load_from(&path).unwrap();
        assert_eq!(loaded2.palette(), Palette::Mfcd);
        assert_eq!(loaded2.detail_variant(), DetailVariant::C);
    }

    #[test]
    fn scale_roundtrip_and_clamp() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp_path(&tmp, "config.toml");

        // Default without a scale entry: 100 %.
        let mut s = Settings::with_path(path.clone());
        assert_eq!(s.scale(), SCALE_DEFAULT);
        s.set_scale(125);
        s.save().unwrap();
        let loaded = Settings::load_from(&path).unwrap();
        assert_eq!(loaded.scale(), 125);

        // Hand-edited out-of-range value: normalized on load, never rejected.
        std::fs::write(
            &path,
            "[ui]\npalette = \"amber\"\ndetail_variant = \"c\"\nscale = 9000\n",
        )
        .unwrap();
        let clamped = Settings::load_from(&path).unwrap();
        assert_eq!(clamped.scale(), SCALE_MAX);

        let mut s2 = Settings::load_from(&path).unwrap();
        s2.set_scale(1);
        assert_eq!(s2.scale(), SCALE_MIN);
        s2.save().unwrap();
        assert_eq!(Settings::load_from(&path).unwrap().scale(), SCALE_MIN);
    }

    #[test]
    fn legacy_config_without_ui_section_uses_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "[uex]\napi_key = \"old-key\"\n").unwrap();
        let loaded = Settings::load_from(&path).unwrap();
        assert_eq!(loaded.uex_api_key(), Some("old-key"));
        assert_eq!(loaded.palette(), Palette::Amber);
        assert_eq!(loaded.detail_variant(), DetailVariant::C);
        assert_eq!(loaded.commodity_source(), CommoditySource::Wiki);
    }

    #[test]
    fn commodity_source_roundtrip_and_parsing() {
        assert_eq!(
            CommoditySource::from_name("wiki"),
            Some(CommoditySource::Wiki)
        );
        assert_eq!(
            CommoditySource::from_name("UEX"),
            Some(CommoditySource::Uex)
        );
        assert_eq!(CommoditySource::from_name("both"), None);

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        let mut s = Settings::with_path(path.clone());
        s.set_commodity_source(CommoditySource::Uex);
        s.save().unwrap();
        let loaded = Settings::load_from(&path).unwrap();
        assert_eq!(loaded.commodity_source(), CommoditySource::Uex);
        // Default stays wiki.
        let mut s2 = Settings::with_path(path.clone());
        s2.set_commodity_source(CommoditySource::Wiki);
        s2.save().unwrap();
        assert_eq!(
            Settings::load_from(&path).unwrap().commodity_source(),
            CommoditySource::Wiki
        );
    }

    #[test]
    fn palette_name_parsing() {
        assert_eq!(Palette::from_name("amber"), Some(Palette::Amber));
        assert_eq!(Palette::from_name("MFCD"), Some(Palette::Mfcd));
        assert_eq!(Palette::from_name(" bloom "), Some(Palette::Bloom));
        assert_eq!(Palette::from_name("pink"), None);
        assert_eq!(DetailVariant::from_name("A"), Some(DetailVariant::A));
        assert_eq!(DetailVariant::from_name("x"), None);
    }

    #[test]
    fn roundtrip_persists_key() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp_path(&tmp, "config.toml");

        let mut s = Settings::with_path(path.clone());
        s.set_uex_api_key(Some(String::from("secret-abc-123")));
        s.save().unwrap();
        assert!(path.exists());

        let loaded = Settings::load_from(&path).unwrap();
        assert_eq!(loaded.uex_api_key(), Some("secret-abc-123"));

        // overwrite wipes the old value
        let mut s2 = Settings::with_path(path.clone());
        s2.set_uex_api_key(None);
        s2.save().unwrap();
        assert_eq!(Settings::load_from(&path).unwrap().uex_api_key(), None);
    }

    #[test]
    fn roundtrip_persists_secret_key_and_username() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp_path(&tmp, "config.toml");

        let mut s = Settings::with_path(path.clone());
        s.set_uex_secret_key(Some(String::from(
            "sk-1234567890-1234567890-1234567890-1234567890",
        )));
        s.set_uex_username(Some(String::from("sebo_one")));
        s.save().unwrap();

        let loaded = Settings::load_from(&path).unwrap();
        assert_eq!(
            loaded.uex_secret_key(),
            Some("sk-1234567890-1234567890-1234567890-1234567890")
        );
        assert_eq!(loaded.uex_username(), Some("sebo_one"));

        // Entfernen (None) löscht beide Felder wieder.
        let mut s2 = Settings::with_path(path.clone());
        s2.set_uex_secret_key(None);
        s2.set_uex_username(None);
        s2.save().unwrap();
        let loaded2 = Settings::load_from(&path).unwrap();
        assert_eq!(loaded2.uex_secret_key(), None);
        assert_eq!(loaded2.uex_username(), None);
    }

    #[test]
    fn legacy_config_without_secret_fields_loads_defaults() {
        // Konfig aus der Zeit VOR M2a-Fix: kein secret_key/username im TOML —
        // Load darf nicht scheitern (serde(default)).
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp_path(&tmp, "config.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "[uex]\napi_key = \"old-key\"\n").unwrap();
        let loaded = Settings::load_from(&path).unwrap();
        assert_eq!(loaded.uex_api_key(), Some("old-key"));
        assert_eq!(loaded.uex_secret_key(), None);
        assert_eq!(loaded.uex_username(), None);
    }

    #[test]
    fn invalid_toml_is_parse_error() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "not [ valid toml ===").unwrap();
        let err = Settings::load_from(&path).unwrap_err();
        assert!(matches!(err, SettingsError::Parse(_)));
    }

    #[cfg(unix)]
    #[test]
    fn key_file_is_user_only() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        let mut s = Settings::with_path(path.clone());
        s.set_uex_api_key(Some(String::from("topsecret")));
        s.save().unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "config file must be 0600, got {:o}",
            mode & 0o777
        );
    }

    // ---- C2a (t_6565fe8b): Shortcut-Validierung + Persistenz --------------

    #[test]
    fn shortcuts_migrate_from_defaults_when_missing() {
        // Erstmigration: altes config.toml ohne [shortcuts] → U/O-Defaults.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "[uex]\napi_key = \"old-key\"\n").unwrap();
        let loaded = Settings::load_from(&path).unwrap();
        assert_eq!(loaded.shortcut_toggle(), DEFAULT_TOGGLE_SHORTCUT);
        assert_eq!(loaded.shortcut_click_through(), DEFAULT_CLICK_SHORTCUT);

        // Komplett fehlende Datei: dieselben Defaults.
        let fresh = Settings::load_from(&tmp.path().join("none.toml")).unwrap();
        assert_eq!(fresh.shortcut_toggle(), "Ctrl+Shift+U");
        assert_eq!(fresh.shortcut_click_through(), "Ctrl+Shift+O");
    }

    #[test]
    fn shortcuts_roundtrip_and_canonical_order() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");

        let mut s = Settings::with_path(path.clone());
        s.set_shortcut_toggle(settings_canonical("meta+ctrl+shift+k")); // freie Reihenfolge, gemischt
        s.set_shortcut_click_through(settings_canonical("Alt+Space"));
        s.save().unwrap();

        let loaded = Settings::load_from(&path).unwrap();
        // kanonische Mod-Reihenfolge Ctrl → Alt → Shift → Super
        assert_eq!(loaded.shortcut_toggle(), "Ctrl+Shift+Super+K");
        assert_eq!(loaded.shortcut_click_through(), "Alt+Space");

        // Kanonische Strings sind 1:1 im TOML sichtbar (C2b-Lesbarkeit).
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("toggle = \"Ctrl+Shift+Super+K\""));
        assert!(content.contains("click_through = \"Alt+Space\""));
    }

    #[test]
    fn omarchy_sync_consent_migrates_off_and_roundtrips() {
        // Erstmigration: altes config ohne sync_omarchy -> false (App schreibt
        // ohne Zustimmung NIE in die Systemkonfiguration).
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "[shortcuts]\ntoggle = \"Ctrl+Shift+U\"\n").unwrap();
        let loaded = Settings::load_from(&path).unwrap();
        assert!(!loaded.shortcut_sync_omarchy());

        let mut s = Settings::load_from(&path).unwrap();
        s.set_shortcut_sync_omarchy(true);
        s.save().unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("sync_omarchy = true"),
            "C2b-Lesbarkeit: {content}"
        );
        assert!(Settings::load_from(&path).unwrap().shortcut_sync_omarchy());

        // Ausschalten persistiert ebenfalls.
        let mut s2 = Settings::load_from(&path).unwrap();
        s2.set_shortcut_sync_omarchy(false);
        s2.save().unwrap();
        assert!(!Settings::load_from(&path).unwrap().shortcut_sync_omarchy());
    }

    #[test]
    fn hand_edited_invalid_shortcut_falls_back_to_default() {
        // wie norm_scale: kaputter Wert darf den Start nie blockieren.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "[shortcuts]\ntoggle = \"Kauderwelsch\"\n").unwrap();
        let loaded = Settings::load_from(&path).unwrap();
        assert_eq!(loaded.shortcut_toggle(), DEFAULT_TOGGLE_SHORTCUT);
        assert_eq!(loaded.shortcut_click_through(), DEFAULT_CLICK_SHORTCUT);
    }

    fn settings_canonical(input: &str) -> String {
        validate_shortcut_combo(input).unwrap_or_else(|e| panic!("valid combo rejected: {e}"))
    }

    #[test]
    fn parse_normalizes_aliases_and_order() {
        assert_eq!(settings_canonical("strg+shift+u"), "Ctrl+Shift+U");
        assert_eq!(settings_canonical("CTRL+U"), "Ctrl+U");
        assert_eq!(settings_canonical("Meta+Space"), "Super+Space");
        assert_eq!(settings_canonical("WIN+F8"), "Super+F8");
        assert_eq!(settings_canonical("alt+option+k"), "Alt+K"); // dedup, Mods nach Taste ok
        assert_eq!(settings_canonical("Ctrl+C+Shift"), "Ctrl+Shift+C"); // Reihenfolge egal → kanonisch
        assert_eq!(settings_canonical("Ctrl+KEYU"), "Ctrl+U");
        assert_eq!(settings_canonical("Shift+DIGIT7"), "Shift+7");
        assert_eq!(settings_canonical("Ctrl+ pgup "), "Ctrl+PageUp");
        assert_eq!(settings_canonical("ctrl+f10"), "Ctrl+F10"); // F7–F11 frei
    }

    #[test]
    fn parse_rejects_structurally_invalid_combos() {
        for bad in [
            "U",            // keine Mod-Taste
            "Ctrl+Shift",   // keine Haupttaste
            "Ctrl+C+V",     // zwei Haupttasten
            "",             // leer
            "Ctrl+",        // leeres Token nach Filter → nur Ctrl
            "CmdOrCtrl+K",  // OS-konditional — kein kanonisches Token
            "Ctrl+Numpad5", // Ziffernblock explizit abgelehnt
            "Ctrl+F25",     // außerhalb F1–F24
        ] {
            let err = validate_shortcut_combo(bad).unwrap_err();
            assert!(!err.is_empty(), "rejection needs a visible message");
        }
    }

    #[test]
    fn reserved_internal_keys_are_conflicts_with_list() {
        // F-Tasten des Overlays + UI-Grundtasten (Handler prüft keine Mods).
        let err = validate_shortcut_combo("Ctrl+F5").unwrap_err();
        assert!(
            err.contains("F5") && err.contains("Preis"),
            "conflict list: {err}"
        );
        for reserved in [
            "Ctrl+Shift+F2",
            "Alt+F12",
            "Super+Enter",
            "Ctrl+Tab",
            "Alt+Up",
            "Ctrl+Alt+M",
        ] {
            assert!(
                validate_shortcut_combo(reserved).is_err(),
                "{reserved} must collide with an internal hotkey"
            );
        }
        // Escape als Kombination ablehnen (Task-Vorgabe).
        let err = validate_shortcut_combo("Ctrl+Shift+Escape").unwrap_err();
        assert!(err.contains("Escape"), "message names Escape: {err}");
        // Alt+Zahl ist intern die Badge-Navigation (ignoriert weitere Mods).
        let err = validate_shortcut_combo("Ctrl+Alt+3").unwrap_err();
        assert!(err.contains("Alt+3"), "conflict list: {err}");
    }

    #[test]
    fn canonical_strings_parse_by_global_hotkey_format() {
        // Der persistierte String MUSS im global_hotkey-Format stehen (Windows-
        // Registrierungspfad nutzt Shortcut::from_str mit demselben Parser).
        // Hier Spiegel-Test ohne global_hotkey-Import: dieselbe Token-Semantik.
        for canonical in [
            "Ctrl+Shift+U",
            "Ctrl+Shift+O",
            "Ctrl+Shift+Super+K",
            "Alt+Space",
            "Alt+Super+F9",
        ] {
            let tokens: Vec<&str> = canonical.split('+').collect();
            assert!(tokens.len() >= 2, "shortcut needs mod + key: {canonical}");
            assert!(
                validate_shortcut_combo(canonical).unwrap() == canonical,
                "canonical must roundtrip through validation: {canonical}"
            );
        }
    }
}

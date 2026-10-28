//! # Settings IPC commands
//!
//! Bridges the `settings` crate (config.toml persistence) to the palette
//! frontend. The frontend reads the full settings snapshot once at boot
//! (to apply `data-palette` / `data-detail-variant`) and writes individual
//! values from the Config-View. Every setter persists immediately — there is
//! no save button, matching the Config-View contract (t_163074f3).
//!
//! Secrets: the UEX API key is never returned to the frontend, only its
//! "set / not set" status.

use std::sync::Mutex;

use serde::Serialize;
use settings::{CommoditySource, DetailVariant, Palette, Settings};

/// Shared settings handle. Loaded once at setup; every setter mutates and
/// immediately persists the same instance (single source of truth).
pub struct SettingsState {
    inner: Mutex<Settings>,
    /// Live commodity-source switch shared with the composite provider —
    /// see `providers::SourceSwitch`. Kept in sync with the persisted value
    /// by every setter.
    pub source_switch: crate::providers::SourceSwitch,
}

impl SettingsState {
    /// Load from the standard config path; a broken/missing config falls back
    /// to defaults (a broken config must not prevent the overlay from
    /// starting — same policy as the API-key load in `setup_trade`).
    pub fn load() -> SettingsState {
        let settings = settings::Settings::load().unwrap_or_default();
        SettingsState {
            source_switch: crate::providers::SourceSwitch::new(settings.commodity_source()),
            inner: Mutex::new(settings),
        }
    }

    /// The LIVE part of `settings_set_commodity_source`: mutate the settings
    /// snapshot AND flip the live source switch the composite provider reads
    /// on every price fetch. The tauri command adds only the disk persist on
    /// top — one shared code path so the regression wiring test
    /// (lib.rs `wiring_tests`) exercises exactly the setter's flip sequence.
    pub(crate) fn set_commodity_source_live(&self, source: CommoditySource) {
        self.inner
            .lock()
            .expect("settings lock")
            .set_commodity_source(source);
        self.source_switch.set(source);
    }

    /// C2a (t_6565fe8b): the canonical shortcut strings. overlay::init reads
    /// them at startup; the setter commands write them. Single source of
    /// truth for both the OS registration and C2b's compositor binds.
    /// C2b: full shortcut state (combos + consent) for the sync gates.
    pub fn shortcut_state(&self) -> (String, String, bool) {
        let s = self.inner.lock().expect("settings lock");
        (
            s.shortcut_toggle().to_owned(),
            s.shortcut_click_through().to_owned(),
            s.shortcut_sync_omarchy(),
        )
    }

    pub fn shortcut_combos(&self) -> (String, String) {
        let s = self.inner.lock().expect("settings lock");
        (
            s.shortcut_toggle().to_owned(),
            s.shortcut_click_through().to_owned(),
        )
    }

    fn persist(&self, settings: &Settings) {
        if let Err(e) = settings.save() {
            log::warn!("settings: persist failed: {e}");
        }
    }
}

/// C2b (t_1adbc017): letzte Bind-Sync-Meldung fuer die Settings-UI (deutsch,
/// 1:1 sichtbar nach dem K6-Muster). Kein persistierter Zustand — die
/// Wahrheit liegt im OmaSettings-Store, die Note ist die Rueckmeldung des
/// letzten Sync-Versuchs dieser App-Instanz.
// Clonebar (Arc-Inneres): Kommando-Handler koennen eine Kopie an den
// Sync-Hintergrundthread weitergeben, ohne den Main-Thread zu blockieren
// (ANR-Schutz, t_1adbc017: synchrone Sync-Ausfuehrung in settings_set_*
// hing den Wayland-Ping des Fensters fest — 'Application Not Responding').
#[derive(Debug, Default, Clone)]
pub struct SyncNote(pub std::sync::Arc<std::sync::Mutex<String>>);

impl SyncNote {
    pub fn set(&self, msg: impl Into<String>) {
        *self.0.lock().expect("sync note lock") = msg.into();
    }
    pub fn take(&self) -> String {
        self.0.lock().expect("sync note lock").clone()
    }
}

/// Frontend-facing snapshot. The API key is intentionally only a boolean —
/// the raw value never leaves the backend.
///
/// Wire contract (t_9b385831, v0.3.3): field names serialize as written
/// (snake_case). The frontend reads them snake_case (`cfg.uex_api_key_set`,
/// `cfg.commodity_source`, `cfg.detail_variant`). A `rename_all = "camelCase"`
/// here silently renamed those three fields to `uexApiKeySet`/
/// `commoditySource`/`detailVariant` on the wire: the frontend saw
/// `undefined` for every underscore field — the API-key status was forever
/// "nicht gesetzt" and the price-source chips never highlighted, even though
/// the backend persisted both correctly. The serialization test below pins
/// the exact key set; single-word fields (palette, scale) never broke, which
/// is why the failure looked partial.
#[derive(Debug, Serialize)]
pub struct SettingsSnapshot {
    pub palette: String,
    pub detail_variant: String,
    pub uex_api_key_set: bool,
    /// M2a-Fix (t_2f197d1c): bool-only wie der API-Key — der rohe persönliche
    /// Secret-Key verlässt das Backend NIE (Security-Auflage der Karte).
    pub uex_secret_key_set: bool,
    /// M2a-Fix: UEX-Benutzername (kein Secret — Textfeld-Wert für den
    /// `marketplace_listings?username=`-Pull). Leer = nicht gesetzt.
    pub uex_username: String,
    /// Primary commodity price source ("wiki" | "uex"); wiki first, UEX
    /// fallback. Exposed so the Config-View can A/B the sources.
    pub commodity_source: String,
    /// K2 (t_5fe8eb4c, P12): font/icon scale in % (50..200, default 100).
    pub scale: u16,
    /// C2a (t_6565fe8b): kanonische, persistierte Kürzel (Soll — das, was
    /// der Nutzer gestellt hat und was überlebt).
    pub shortcut_toggle: String,
    pub shortcut_click_through: String,
    /// C2a: Ist — meldet, ob das OS die globalen Kürzel WIRKLICH registriert
    /// hat (Wayland: false, dort greift der Compositor-Pfad; C2b bindet aus
    /// den Soll-Werten). Soll-Ist wie beim Preisquellen-Feedback (K6).
    pub global_shortcuts_active: bool,
    /// C2b: persistierter Consent (einmalige Zustimmung, Default false).
    pub shortcuts_sync_omarchy: bool,
    /// PB (t_1c9bc240, B1): persistierte Mode-Hotkeys (F-Keys der In-App-Modi).
    pub mode_hotkeys: settings::ModeHotkeys,
    /// M2b Sicherheitsmodell: 2-Schritt-Bestätigung für Marktplatz-Writes
    /// ("every_action" = jede schreibende Aktion, Default; "delete_only" = nur
    /// Abmelden bestätigt). Keine Batch-Aktionen.
    pub marketplace_confirm: String,
    /// C2b: Ist-Zustand der Compositor-Anbindung. Nur wenn BEIDES true ist,
    /// greift der OmaSettings-Pfad: Hyprland-Session aktiv
    /// (HYPRLAND_INSTANCE_SIGNATURE) und Plugin-Bin vorhanden. Sonst bleibt
    /// die App lesend/passiv und verweist auf docs/WAYLAND.md.
    pub hyprland_session: bool,
    pub omasettings_available: bool,
    /// C2b: Rueckmeldung des letzten Sync-Versuchs (leer = nichts getan).
    pub sync_note: String,
}

impl SettingsSnapshot {
    /// C2b: Compositor-Fakten werden live am Snapshot-Hang berechnet — ein
    /// stat()-Aufruf, kein Schreibvorgang; die UI zeigt dadurch immer den
    /// IST-Zustand der Anbindung (Soll-Ist-Muster K6).
    fn from_settings(s: &Settings, global_shortcuts_active: bool, sync_note: &str) -> Self {
        SettingsSnapshot {
            palette: s.palette().as_str().to_owned(),
            detail_variant: s.detail_variant().as_str().to_owned(),
            uex_api_key_set: s.uex_api_key().is_some(),
            uex_secret_key_set: s.uex_secret_key().is_some(),
            uex_username: s.uex_username().unwrap_or_default().to_owned(),
            commodity_source: s.commodity_source().as_str().to_owned(),
            scale: s.scale(),
            shortcut_toggle: s.shortcut_toggle().to_owned(),
            shortcut_click_through: s.shortcut_click_through().to_owned(),
            global_shortcuts_active,
            shortcuts_sync_omarchy: s.shortcut_sync_omarchy(),
            mode_hotkeys: s.mode_hotkeys().clone(),
            marketplace_confirm: s.marketplace_confirm().as_str().to_owned(),
            hyprland_session: crate::hyprland_sync::hyprland_session_active(),
            omasettings_available: crate::hyprland_sync::OmarchyPaths::detect().is_some(),
            sync_note: sync_note.to_owned(),
        }
    }
}

/// C2a pure validation stage shared by both shortcut commands: settings-crate
/// rules (≥1 Mod + 1 Taste, no Escape, internal conflict list) PLUS the
/// cross-field rule — the two fields must not end up with the same combo.
/// Kept tauri-free so it is unit-testable without a live app.
fn validate_shortcut_input(
    combo: &str,
    peer_combo: &str,
    peer_label: &str,
) -> Result<String, String> {
    let canonical = settings::validate_shortcut_combo(combo)?;
    if canonical == peer_combo {
        return Err(format!(
            "Diese Kombination ist bereits das {peer_label}-Kürzel — zwei Kürzel derselben Taste doppelt vergeben geht nicht."
        ));
    }
    Ok(canonical)
}

/// C2b: gate + execute one slot's compositor sync. Gates (card mechanism 6):
/// consent flag AND live Hyprland session AND plugin binary — without ALL
/// three, nothing is ever written. Result lands in the UI note; a failed
/// sync never rolls back the persisted combo (that would silently lose the
/// user's new value — the note names the manual recovery instead).
fn sync_slot(consent: bool, note: &SyncNote, canonical: &str, command: &str, description: &str) {
    // Deadlock-Fix (t_1adbc017): sync_slot darf auf KEINEN Zugriff auf
    // SettingsState haben — die Command-Handler rufen es, waehrend sie den
    // `inner`-MutexGuard halten (std::Mutex ist nicht reentrant).
    // Live-Beobachtung: shortcut_state() re-lockt -> 'sync_slot ein'
    // erscheint nie, der Command kehrt nie zurueck, der Store wird nie
    // geschrieben. Consent-Wert wird darum explizit uebergeben.
    eprintln!("[ao-sync] sync_slot ein: {canonical} consent={consent}");
    if !consent {
        eprintln!("[ao-sync] slot skipped: consent=false");
        return; // no consent — compositor config stays untouched
    }
    if !crate::hyprland_sync::hyprland_session_active() {
        eprintln!("[ao-sync] slot skipped: HYPRLAND_INSTANCE_SIGNATURE fehlt");
        return;
    }
    let Some(paths) = crate::hyprland_sync::OmarchyPaths::detect() else {
        eprintln!("[ao-sync] slot skipped: Plugin nicht gefunden");
        note.set("Compositor-Sync: Hyprland läuft, aber das OmaSettings-Plugin wurde nicht gefunden. Manuelles Anlegen der Binds: docs/WAYLAND.md.");
        return;
    };
    // ANR-Schutz (t_1adbc017): sync_combo fuehrt mehrere omasettings-CLI-
    // Laeufe + Reload + Verifikations-Polling aus — das blockiert Minuten
    // bis Sekunden. Laeuft es im Kommando-Handler (Main-Thread), haengt der
    // Thread den CLI-Kind selbst fest: der Kompositor reloadet, pingt das
    // Overlay-Fenster, dessen Main-Thread auf den CLI wartet —
    // Ping-Deadlock bis zum 'Application Not Responding'-Dialog
    // (live beobachtet: jeder UI-Kuerzel-Commit erzeugte genau dieses
    // Muster; Startup-Drift lief dagegen im Thread und funktionierte).
    // Deshalb: Heavywork in den Hintergrundthread, Main-Thread frei.
    let note = note.clone();
    let canonical = canonical.to_owned();
    let command = command.to_owned();
    let description = description.to_owned();
    std::thread::spawn(move || {
        run_sync_serialized(paths, &canonical, &command, &description, &note);
    });
}

/// Nur ein Compositor-Sync darf gleichzeitig CLI-Kommandos + Reloads
/// ausloesen (read-modify-write auf omasettings.json verTRAegt keine
/// Parallelitaet; zwei Reloads gleichzeitig ping-pong-nieren den
/// Kompositor). Der Lock blockiert einen Hintergrundthread, nie den
/// Main-Thread.
static SYNC_CLI_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn run_sync_serialized(
    paths: crate::hyprland_sync::OmarchyPaths,
    canonical: &str,
    command: &str,
    description: &str,
    note: &SyncNote,
) {
    let _serial = SYNC_CLI_LOCK.lock().unwrap_or_else(|p| {
        eprintln!("[ao-sync] Lock vergiftet — Sync faehrt fort");
        p.into_inner()
    });
    match crate::hyprland_sync::sync_combo(&paths, canonical, command, description) {
        Ok(outcome) if outcome.changed => {
            log::info!("hyprland bind sync: {}", outcome.note);
            eprintln!("[ao-sync] slot OK: {}", outcome.note);
            note.set(outcome.note);
        }
        Ok(_) => {
            // store already matched — nothing written, nothing to report
            eprintln!("[ao-sync] slot no-op: Store entspricht bereits {canonical}");
        }
        Err(err) => {
            log::warn!("hyprland bind sync failed: {err}");
            eprintln!("[ao-sync] slot FAIL: {err}");
            note.set(format!("Compositor-Sync fehlgeschlagen: {err}"));
        }
    }
}

/// C2b: drift sync for BOTH slots (used on consent-enable and at app start).
/// Consent-gates werden synchron geprueft, die Heavywork laeuft im
/// Hintergrundthread (ANR-Schutz, wie sync_slot — t_1adbc017).
fn sync_all(state: &SettingsState, note: &SyncNote) {
    if !state.shortcut_state().2 || !crate::hyprland_sync::hyprland_session_active() {
        return;
    }
    let Some(paths) = crate::hyprland_sync::OmarchyPaths::detect() else {
        note.set("Compositor-Sync: Hyprland läuft, aber das OmaSettings-Plugin wurde nicht gefunden. Manuelles Anlegen der Binds: docs/WAYLAND.md.");
        return;
    };
    let (toggle, click, _) = state.shortcut_state();
    let note = note.clone();
    std::thread::spawn(move || {
        let _serial = SYNC_CLI_LOCK.lock().unwrap_or_else(|p| {
            eprintln!("[ao-sync] Lock vergiftet — Sync faehrt fort");
            p.into_inner()
        });
        match crate::hyprland_sync::startup_sync_drift(&paths, &toggle, &click) {
            Ok(outcomes) => {
                if outcomes.is_empty() {
                    eprintln!(
                        "[ao-sync] drift no-op: System-Binds entsprechen bereits den gespeicherten Kürzeln"
                    );
                    note.set(
                        "Compositor-Sync aktiv — System-Binds entsprechen den gespeicherten Kürzeln.",
                    );
                }
                for o in outcomes {
                    log::info!("hyprland bind sync: {}", o.note);
                    eprintln!("[ao-sync] drift OK: {}", o.note);
                    note.set(o.note);
                }
            }
            Err(err) => {
                log::warn!("hyprland bind sync failed: {err}");
                eprintln!("[ao-sync] drift FAIL: {err}");
                note.set(format!("Compositor-Sync fehlgeschlagen: {err}"));
            }
        }
    });
}

/// C2b (t_1adbc017): startup drift check, called from lib.rs setup thread
/// after a short delay. Consent-gated: without `[shortcuts] sync_omarchy =
/// true` this returns before touching anything.
pub fn startup_hypr_sync<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::Manager;
    let (Some(state), Some(note)) = (
        app.try_state::<SettingsState>(),
        app.try_state::<SyncNote>(),
    ) else {
        return;
    };
    sync_all(&state, &note);
}

/// Read the current settings snapshot.
#[tauri::command]
pub fn settings_get(
    state: tauri::State<SettingsState>,
    app: tauri::AppHandle,
    sync: tauri::State<SyncNote>,
) -> SettingsSnapshot {
    let s = state.inner.lock().expect("settings lock");
    SettingsSnapshot::from_settings(
        &s,
        crate::overlay::global_shortcuts_active(&app),
        &sync.take(),
    )
}

/// Set the colour palette (amber | mfcd | bloom). Persists immediately.
#[tauri::command]
pub fn settings_set_palette(
    state: tauri::State<SettingsState>,
    app: tauri::AppHandle,
    sync: tauri::State<SyncNote>,
    name: String,
) -> Result<SettingsSnapshot, String> {
    let palette = Palette::from_name(&name).ok_or_else(|| format!("unknown palette: {name}"))?;
    let mut s = state.inner.lock().expect("settings lock");
    s.set_palette(palette);
    state.persist(&s);
    Ok(SettingsSnapshot::from_settings(
        &s,
        crate::overlay::global_shortcuts_active(&app),
        &sync.take(),
    ))
}

/// Set the price-detail variant (c | a). Persists immediately.
#[tauri::command]
pub fn settings_set_detail_variant(
    state: tauri::State<SettingsState>,
    app: tauri::AppHandle,
    sync: tauri::State<SyncNote>,
    name: String,
) -> Result<SettingsSnapshot, String> {
    let variant =
        DetailVariant::from_name(&name).ok_or_else(|| format!("unknown variant: {name}"))?;
    let mut s = state.inner.lock().expect("settings lock");
    s.set_detail_variant(variant);
    state.persist(&s);
    Ok(SettingsSnapshot::from_settings(
        &s,
        crate::overlay::global_shortcuts_active(&app),
        &sync.take(),
    ))
}

/// Set (Some) or remove (None) the UEX API key. Persists immediately.
/// The key itself is never echoed back.
#[tauri::command]
pub fn settings_set_uex_api_key(
    state: tauri::State<SettingsState>,
    app: tauri::AppHandle,
    sync: tauri::State<SyncNote>,
    key: Option<String>,
) -> SettingsSnapshot {
    let mut s = state.inner.lock().expect("settings lock");
    s.set_uex_api_key(key.filter(|k| !k.trim().is_empty()));
    state.persist(&s);
    SettingsSnapshot::from_settings(
        &s,
        crate::overlay::global_shortcuts_active(&app),
        &sync.take(),
    )
}

/// M2a-Fix (t_2f197d1c): Set (Some) or remove (None) the UEX persönlicher
/// Secret-Key. Persists immediately; the secret key is NEVER echoed back —
/// the snapshot carries only the bool `uex_secret_key_set`.
#[tauri::command]
pub fn settings_set_uex_secret_key(
    state: tauri::State<SettingsState>,
    app: tauri::AppHandle,
    sync: tauri::State<SyncNote>,
    key: Option<String>,
) -> SettingsSnapshot {
    let mut s = state.inner.lock().expect("settings lock");
    s.set_uex_secret_key(key.filter(|k| !k.trim().is_empty()));
    state.persist(&s);
    SettingsSnapshot::from_settings(
        &s,
        crate::overlay::global_shortcuts_active(&app),
        &sync.take(),
    )
}

/// M2a-Fix (t_2f197d1c): Set (Some) or remove (None) the UEX-Username
/// (exakt wie auf UEX registriert) — für den `marketplace_listings?username=`
/// -Pull des eigenen Shops. Persists immediately.
#[tauri::command]
pub fn settings_set_uex_username(
    state: tauri::State<SettingsState>,
    app: tauri::AppHandle,
    sync: tauri::State<SyncNote>,
    name: Option<String>,
) -> SettingsSnapshot {
    let mut s = state.inner.lock().expect("settings lock");
    s.set_uex_username(name.filter(|n| !n.trim().is_empty()));
    state.persist(&s);
    SettingsSnapshot::from_settings(
        &s,
        crate::overlay::global_shortcuts_active(&app),
        &sync.take(),
    )
}

/// M2b Sicherheitsmodell: `.swap` marketplace_confirm-Modus für
/// Marktplatz-Writes (every_action | delete_only). Ungültige Werte → 400
/// Meldung; Default bleibt every_action.
#[tauri::command]
pub fn settings_set_marketplace_confirm(
    state: tauri::State<SettingsState>,
    app: tauri::AppHandle,
    sync: tauri::State<SyncNote>,
    mode: String,
) -> Result<SettingsSnapshot, String> {
    let confirm = settings::MarketplaceConfirm::from_name(&mode)
        .ok_or_else(|| format!("unbekannter Bestätigungs-Modus: {mode}"))?;
    let mut s = state.inner.lock().expect("settings lock");
    s.set_marketplace_confirm(confirm);
    state.persist(&s);
    Ok(SettingsSnapshot::from_settings(
        &s,
        crate::overlay::global_shortcuts_active(&app),
        &sync.take(),
    ))
}

/// PB (t_1c9bc240, B1): EINEN Mode-Hotkey setzen (nur die konfigurierbaren
/// Slots — F1 Suche, F2 Marktplatz und F12 Einstellungen bleiben fest).
/// Validierung: gültiger F-Key (F1–F12), `F5` reserviert für den Resync,
/// keine Doppelbelegung (Settings-Crate). Persistiert sofort.
#[tauri::command]
pub fn settings_set_mode_hotkey(
    state: tauri::State<SettingsState>,
    app: tauri::AppHandle,
    sync: tauri::State<SyncNote>,
    field: String,
    key: String,
) -> Result<SettingsSnapshot, String> {
    let key = settings::validate_fkey(&key)?;
    let field_norm = field.trim().to_lowercase();
    if key == settings::MODE_HOTKEY_RESERVED {
        return Err(format!("{field_norm}: F5 ist für den Resync reserviert"));
    }
    let mut s = state.inner.lock().expect("settings lock");
    let mut hotkeys = s.mode_hotkeys().clone();
    match field_norm.as_str() {
        "hangar" => hotkeys.hangar = key.clone(),
        "routes" => hotkeys.routes = key.clone(),
        "inventory" => hotkeys.inventory = key.clone(),
        _ => {
            return Err(format!(
                "Modus nicht konfigurierbar: {field} (F1/F2/F12 sind fest belegt)"
            ));
        }
    }
    hotkeys
        .validate()
        .map_err(|e| format!("{e} — Belegung unverändert"))?;
    s.set_mode_hotkeys(hotkeys);
    state.persist(&s);
    Ok(SettingsSnapshot::from_settings(
        &s,
        crate::overlay::global_shortcuts_active(&app),
        &sync.take(),
    ))
}

/// PB (t_1c9bc240, B1): Mode-Hotkeys auf die Standard-Belegung zurücksetzen
/// (F1 Suche · F2 Marktplatz · F3 Routen · F4 Inventar · F11 Hangar · F12).
#[tauri::command]
pub fn settings_reset_mode_hotkeys(
    state: tauri::State<SettingsState>,
    app: tauri::AppHandle,
    sync: tauri::State<SyncNote>,
) -> Result<SettingsSnapshot, String> {
    let mut s = state.inner.lock().expect("settings lock");
    s.set_mode_hotkeys(settings::ModeHotkeys::default());
    state.persist(&s);
    Ok(SettingsSnapshot::from_settings(
        &s,
        crate::overlay::global_shortcuts_active(&app),
        &sync.take(),
    ))
}

/// Set the primary commodity price source (wiki | uex). Persists immediately
/// AND flips the live source switch the composite provider reads on every
/// price fetch — the next price request already uses the new source.
#[tauri::command]
pub fn settings_set_commodity_source(
    state: tauri::State<SettingsState>,
    app: tauri::AppHandle,
    sync: tauri::State<SyncNote>,
    name: String,
) -> Result<SettingsSnapshot, String> {
    let source = CommoditySource::from_name(&name)
        .ok_or_else(|| format!("unknown commodity source: {name}"))?;
    state.set_commodity_source_live(source);
    let s = state.inner.lock().expect("settings lock");
    state.persist(&s);
    Ok(SettingsSnapshot::from_settings(
        &s,
        crate::overlay::global_shortcuts_active(&app),
        &sync.take(),
    ))
}

/// K2 (t_5fe8eb4c, P12): Set the overlay font/icon scale in % — the F12-View
/// chips expose 80/90/100/110/125, the command accepts any whole percent
/// `SCALE_MIN..SCALE_MAX`. Out-of-range is rejected (clean note for the UI);
/// a hand-edited config value on the load side is instead normalized by the
/// settings crate (`norm_scale`), so the app always boots. Persists
/// immediately and returns the fresh snapshot; the frontend re-applies it
/// live (`--ui-scale` → Shell zoom), like the palette.
#[tauri::command]
pub fn settings_set_scale(
    state: tauri::State<SettingsState>,
    app: tauri::AppHandle,
    sync: tauri::State<SyncNote>,
    pct: u16,
) -> Result<SettingsSnapshot, String> {
    if pct < settings::SCALE_MIN || pct > settings::SCALE_MAX {
        return Err(format!("scale out of range: {pct}"));
    }
    let mut s = state.inner.lock().expect("settings lock");
    s.set_scale(pct);
    state.persist(&s);
    Ok(SettingsSnapshot::from_settings(
        &s,
        crate::overlay::global_shortcuts_active(&app),
        &sync.take(),
    ))
}

/// C2a (t_6565fe8b): Overlay-Toggle-Kürzel setzen. Ablauf (bewusst in
/// dieser Reihenfolge — der OS-Pfad entscheidet, ob persistiert wird):
/// 1. rein prüfen (settings-Crate-Regeln + Doppelvergabe gegen das andere
///    Feld) — Fehler hier ändern NICHTS.
/// 2. `overlay::rebind_shortcut`: deregistriere alt → registriere neu.
///    Scheitert die Registrierung (Belegung durch eine andere App — der
///    Windows-Fall), wird der alte Wert re-registriert und der Fehler
///    sichtbar ans Settings-Feld zurückgegeben; der persistierte Wert
///    bleibt unberührt (= alter Kürzel bleibt aktiv).
/// 3. erst jetzt: persistieren + frischer Snapshot (Soll-Ist-Zeilen im UI).
#[tauri::command]
pub fn settings_set_shortcut_toggle(
    state: tauri::State<SettingsState>,
    app: tauri::AppHandle,
    sync: tauri::State<SyncNote>,
    combo: String,
) -> Result<SettingsSnapshot, String> {
    let peer = state
        .inner
        .lock()
        .expect("settings lock")
        .shortcut_click_through()
        .to_owned();
    eprintln!("[ao-sync] TOGGLE-CMD ein: combo={combo:?}");
    let canonical = validate_shortcut_input(&combo, &peer, "Click-through")?;
    eprintln!("[ao-sync] TOGGLE-CMD validiert: {canonical}");
    crate::overlay::rebind_shortcut(&app, crate::overlay::ShortcutKind::Toggle, &canonical)?;
    eprintln!("[ao-sync] TOGGLE-CMD rebind ok");
    // Deadlock-Fix (t_1adbc017): Snapshot + Consent-Wert werden innerhalb
    // des Guard-Scopes gelesen; der Guard wird VOR sync_slot verworfen —
    // sonst re-lockt sync_slot den nicht-reentranten `inner`-Mutex und der
    // Command haengt fuer immer (Store bleibt ungeschrieben).
    let (consent, snap) = {
        let mut s = state.inner.lock().expect("settings lock");
        s.set_shortcut_toggle(canonical.clone());
        state.persist(&s);
        eprintln!("[ao-sync] TOGGLE-CMD persistiert");
        (
            s.shortcut_sync_omarchy(),
            SettingsSnapshot::from_settings(
                &s,
                crate::overlay::global_shortcuts_active(&app),
                &sync.take(),
            ),
        )
    };
    // C2b: persistierter Soll-Wert wird Compositor-Bind (Consent-Gates in
    // sync_slot). Fehler hier schlagen NICHT auf die Kürzel-Änderung zurück —
    // das Kürzel ist gültig gespeichert, die Note zeigt den Sync-Fehler
    // (verzögertes Snapshot-Refresh im UI).
    eprintln!("[ao-sync] TOGGLE-CMD rufe sync_slot");
    sync_slot(
        consent,
        &sync,
        &canonical,
        crate::hyprland_sync::TOGGLE_COMMAND,
        crate::hyprland_sync::TOGGLE_DESCRIPTION,
    );
    Ok(snap)
}

/// C2a: Click-through-Kürzel setzen — identisches Protokoll wie
/// `settings_set_shortcut_toggle`, nur anderes Feld (und anderer Peer).
#[tauri::command]
pub fn settings_set_shortcut_click_through(
    state: tauri::State<SettingsState>,
    app: tauri::AppHandle,
    sync: tauri::State<SyncNote>,
    combo: String,
) -> Result<SettingsSnapshot, String> {
    let peer = state
        .inner
        .lock()
        .expect("settings lock")
        .shortcut_toggle()
        .to_owned();
    let canonical = validate_shortcut_input(&combo, &peer, "Overlay-Toggle")?;
    crate::overlay::rebind_shortcut(&app, crate::overlay::ShortcutKind::ClickThrough, &canonical)?;
    // Deadlock-Fix (t_1adbc017): wie beim Toggle-Feld — Guard vor sync_slot
    // droppen (nicht-reentranter Mutex), Snapshot/Consent im Scope lesen.
    let (consent, snap) = {
        let mut s = state.inner.lock().expect("settings lock");
        s.set_shortcut_click_through(canonical.clone());
        state.persist(&s);
        (
            s.shortcut_sync_omarchy(),
            SettingsSnapshot::from_settings(
                &s,
                crate::overlay::global_shortcuts_active(&app),
                &sync.take(),
            ),
        )
    };
    // C2b: wie beim Toggle-Feld — Sync nach Persistenz, Consent-gegate't.
    sync_slot(
        consent,
        &sync,
        &canonical,
        crate::hyprland_sync::CLICK_COMMAND,
        crate::hyprland_sync::CLICK_DESCRIPTION,
    );
    Ok(snap)
}

/// C2b: EINMALIGE Zustimmung umschalten ("Tastenkürzel mit System (Omarchy)
/// synchronisieren"). An = App darf die zwei `arch-overlay …`-Binds im
/// OmaSettings-Store führen (Backup vor erstem Schreibvorgang, Verifikation
/// via hyprctl binds + Lua-Syntaxcheck — alles in hyprland_sync). Beim
/// Einschalten wird sofort drift-synchronisiert, damit Store und gespeicherte
/// Kürzel nicht mehr auseinanderlaufen. Aus = App lässt die Systemkonfig in
/// Ruhe (fremde Binds bleiben, wie sie sind).
#[tauri::command]
pub fn settings_set_shortcuts_sync(
    state: tauri::State<SettingsState>,
    app: tauri::AppHandle,
    sync: tauri::State<SyncNote>,
    on: bool,
) -> SettingsSnapshot {
    {
        let mut s = state.inner.lock().expect("settings lock");
        s.set_shortcut_sync_omarchy(on);
        state.persist(&s);
    }
    if on {
        sync_all(&state, &sync);
    } else {
        sync.set("Compositor-Sync deaktiviert — gespeicherte Kürzel bleiben aktiv, Systems-Binds werden nicht mehr verändert.");
    }
    let s = state.inner.lock().expect("settings lock");
    SettingsSnapshot::from_settings(
        &s,
        crate::overlay::global_shortcuts_active(&app),
        &sync.take(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression (t_9b385831, v0.3.3 P1): the snapshot's JSON keys are the
    /// frontend contract. The frontend reads `cfg.uex_api_key_set`,
    /// `cfg.commodity_source`, `cfg.detail_variant` (snake_case). A stray
    /// `rename_all = "camelCase"` made every underscore field `undefined` in
    /// the UI — API-key status stuck at "nicht gesetzt", source chips never
    /// active — while single-word fields (palette, scale) kept working.
    #[test]
    fn snapshot_json_keys_match_frontend_snake_case_contract() {
        let snap = SettingsSnapshot::from_settings(
            &Settings::with_path(std::path::PathBuf::from("/dev/null")),
            true,
            "",
        );
        let keys: Vec<String> = {
            let v = serde_json::to_value(&snap).unwrap();
            let map = v.as_object().expect("snapshot serializes as object");
            map.keys().cloned().collect()
        };
        for required in [
            "palette",
            "detail_variant",
            "uex_api_key_set",
            // M2a-Fix (t_2f197d1c): bool-only Secret-Key + Username — dieselbe
            // Snake-Case-Vertrag; camelCase wäre im UI unsichtbar.
            "uex_secret_key_set",
            "uex_username",
            "commodity_source",
            "scale",
            // C2a (t_6565fe8b): dieselbe Snake-Case-Vertrag gilt für die
            // Shortcut-Felder — camelCase würde sie im UI unsichtbar machen.
            "shortcut_toggle",
            "shortcut_click_through",
            "global_shortcuts_active",
            // C2b (t_1adbc017): Consent-Flag + Compositor-Ist + Sync-Note —
            // dieselbe Snake-Case-Vertrag; camelCase wäre im UI unsichtbar.
            "shortcuts_sync_omarchy",
            // M2b: 2-Schritt-Bestätigung für Marktplatz-Writes (snake_case).
            "marketplace_confirm",
            "hyprland_session",
            "omasettings_available",
            "sync_note",
        ] {
            assert!(
                keys.iter().any(|k| k == required),
                "frontend expects key '{required}', wire has {keys:?}"
            );
        }
        // No camelCase aliases may reappear: they read as undefined in the UI.
        for forbidden in ["uexApiKeySet", "commoditySource", "detailVariant"] {
            assert!(
                !keys.iter().any(|k| k == forbidden),
                "wire key '{forbidden}' is invisible to the frontend (snake_case contract)"
            );
        }
    }

    /// C2b (t_1adbc017): the sync note is a single shared slot — the last
    /// operation's feedback wins, which is what the UI shows (K6 pattern).
    #[test]
    fn sync_note_last_write_wins() {
        let note = SyncNote::default();
        assert_eq!(note.take(), "");
        note.set("erste Meldung");
        note.set("zweite Meldung");
        assert_eq!(note.take(), "zweite Meldung");
    }

    /// Regression (t_c3d46cff): the switch handed to the composite provider
    /// (lib.rs clones `state.source_switch`) MUST be the same instance the
    /// setter flips — a clone shares the inner Arc, a fresh switch would
    /// freeze the boot value until the next app restart.
    #[test]
    fn live_source_switch_flips_without_restart() {
        let state = SettingsState::load();
        let reader = state.source_switch.clone();
        // what settings_set_commodity_source does on a live flip:
        state.source_switch.set(CommoditySource::Uex);
        assert_eq!(reader.get(), CommoditySource::Uex);
        state.source_switch.set(CommoditySource::Wiki);
        assert_eq!(reader.get(), CommoditySource::Wiki);
    }

    /// C2a (t_6565fe8b): cross-field rule of the shortcut setter — the two
    /// fields may not end up on the same combo; settings-crate rules pass
    /// through untranslated (German, shown 1:1 in the Settings field).
    #[test]
    fn shortcut_pair_validation() {
        // ok: fresh canonical combo, peer untouched
        assert_eq!(
            validate_shortcut_input("ctrl+alt+k", "Ctrl+Shift+O", "Click-through").unwrap(),
            "Ctrl+Alt+K"
        );
        // Doppelvergabe: identical to the other field's active combo
        let err =
            validate_shortcut_input("Ctrl+Shift+O", "Ctrl+Shift+O", "Click-through").unwrap_err();
        assert!(
            err.contains("Click-through"),
            "names the colliding field: {err}"
        );
        // internal conflict from the settings crate surfaces verbatim
        let err = validate_shortcut_input("Ctrl+F2", "Ctrl+Shift+O", "Click-through").unwrap_err();
        assert!(err.contains("F2"), "conflict list names F2: {err}");
        // structurally invalid (no main key)
        assert!(validate_shortcut_input("Ctrl+Shift", "Ctrl+Shift+O", "Click-through").is_err());
    }
}

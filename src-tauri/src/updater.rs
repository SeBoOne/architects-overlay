//! # updater — Update-Check + on-demand Update (UC t_d323be7e)
//!
//! Brückt die `updater`-Crates-Logik an die App: Status-State für die
//! Settings-UI (Chip „Update vX.Y.Z verfügbar“ + Button „Jetzt
//! aktualisieren“), periodischer Check (Start + alle N Stunden, Karte
//! Umsetzung 1) und der on-demand-Installationsvorgang (Karte Umsetzung
//! 3/4). NICHTS passiert ohne Nutzerwunsch: der periodische Check ändert
//! nur die UI-Note; Downloads/Installation laufen ausschließlich über
//! `updater_install` — der Klick auf „Jetzt aktualisieren“ ist die
//! Bestätigung (Karte Umsetzung 2: nie auto-install ohne Wunsch).
//!
//! Ausführungsentscheidung Windows (dokumentiert in docs/UPDATER.md):
//! die App lädt setup.exe herunter, verifiziert SHA256, startet das Setup
//! SILENT (`/S`) + `/R` als CHILD-PROZESS (detached, CREATE_NO_WINDOW —
//! gleiche Mechanik wie der tasklist-Fix t_d323b29e) und beendet sich dann
//! sauber selbst; NSIS schließt eine laufende Instanz ab, installiert und
//! startet das Overlay nach Abschluss selbst neu (`/R` — Sebo-Wunsch:
//! nach dem Update wird das Overlay wieder gestartet).
//! Linux AppImage: neue Datei atomar am laufenden AppImage-Pfad ersetzen
//! (alte umbenennen → neue schreiben → exec-Rechte → alte löschen,
//! Umsetzung 4) und relaunchen. .deb: KEIN Self-Update — Hint mit dem
//! apt-Befehl (keine root-Aktionen aus der App, Umsetzung 4).
//! Linux Bare-Binary (TH t_df9646a3): rohes Binary-Asset `*_linux64`
//! herunterladen → SHA256 → atomar über die laufende exe renamen + Hinweis
//! „Neustart des Overlays anwenden lassen“ — KEIN Auto-Restart/Exit.

use crate::i18n_err::{err_key, err_keyp};
use serde_json::json;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use updater::{CheckResult, Plan, UpdaterError};

use crate::app_settings::SettingsState;

/// UI-Status des letzten Checks (Note in der Config-View, K6-Muster).
#[derive(Debug, Default)]
pub struct UpdaterState {
    note: Mutex<String>,
    available: Mutex<Option<AvailableUpdate>>,
    last_check_secs: Mutex<Option<u64>>,
    checking: AtomicBool,
}

/// Ein verfügbares Update (Plan liegt ROH im State — die Status-Sicht
/// fürs Frontend serialisiert nur die lesbaren Felder).
#[derive(Debug, Clone)]
pub struct AvailableUpdate {
    pub version: String,
    pub plan: Plan,
}

/// Frontend-Sicht des Updater-Status (snake_case wie die anderen Snapshots).
#[derive(Debug, Serialize)]
pub struct UpdaterStatus {
    pub enabled: bool,
    pub interval_hours: u32,
    pub app_version: String,
    pub note: String,
    pub checking: bool,
    pub last_check_secs: Option<u64>,
    pub available: Option<AvailableInfo>,
}

#[derive(Debug, Serialize)]
pub struct AvailableInfo {
    pub version: String,
    /// "nsis" | "appimage" | "deb" — die UI blendet den Install-Button nur
    /// für self-update-fähige Kinds ein.
    pub kind: String,
    pub asset_name: String,
}

impl UpdaterState {
    fn set_note(&self, msg: impl Into<String>) {
        *self.note.lock().expect("updater note lock") = msg.into();
    }

    fn note(&self) -> String {
        self.note.lock().expect("updater note lock").clone()
    }
}

/// Fehler → deutsche UI-Note (die `updater`-Crates-Meldungen sind bereits
/// deutsch; hier ggf. anreichern).
fn friendly(err: &UpdaterError) -> String {
    err.to_string()
}

/// Ein Check-Lauf: Status schreiben + Log-Zeile (Umsetzung 6: Update-Log).
async fn run_check(state: &UpdaterState, settings: &SettingsState) {
    let (_, _, repo, base_url) = settings.updater_config();
    let current = env!("CARGO_PKG_VERSION");
    state.checking.store(true, Ordering::SeqCst);
    let result = updater::check(&base_url, &repo, current).await;
    state.checking.store(false, Ordering::SeqCst);
    *state.last_check_secs.lock().expect("updater ts lock") = Some(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    );
    match result {
        Ok(Some(CheckResult {
            latest_version,
            plan,
            ..
        })) => {
            log::info!(
                "updater: Update verfügbar {current} -> {latest_version} ({})",
                plan.kind().as_str()
            );
            *state.available.lock().expect("updater avail lock") = Some(AvailableUpdate {
                version: latest_version.clone(),
                plan,
            });
            state.set_note(err_keyp(
                "update.note_available",
                json!({ "version": latest_version }),
            ));
        }
        Ok(None) => {
            *state.available.lock().expect("updater avail lock") = None;
            state.set_note(err_keyp(
                "update.note_uptodate",
                json!({ "version": current }),
            ));
        }
        Err(err) => {
            // Fehlerpfad (Umsetzung 6): alter Zustand bleibt intakt — nur die
            // Note meldet den Fehlversuch, ein vorhandenes Available bleibt.
            log::warn!("updater: check failed: {err}");
            // T2: Note = der key-JSON-Fehler selbst (kein Wrapping — die
            // Details leben im Log; die UI zeigt die übersetzte Meldung).
            state.set_note(friendly(&err));
        }
    }
}

/// Periodischer Check (Start + alle N Stunden): läuft als eigener Thread,
/// liest Settings JE Runde frisch (Intervall/Repo-Änderungen greifen ohne
/// Neustart). Disabled (`[updater] enabled=false`) → thread idelt weiter,
/// aber checkt nicht — Konfig-Änderungen greifen ohne Neustart.
pub fn start_background_check(app: tauri::AppHandle) {
    use tauri::Manager;
    std::thread::spawn(move || {
        loop {
            let state = app.try_state::<UpdaterState>();
            let settings = app.try_state::<SettingsState>();
            let Some(settings) = settings else {
                // State noch nicht verwaltet (früher Setup-Zeitpunkt) — warten.
                std::thread::sleep(Duration::from_secs(5));
                continue;
            };
            let Some(state) = state else {
                std::thread::sleep(Duration::from_secs(5));
                continue;
            };
            let (enabled, _interval, _, _) = settings.updater_config();
            if enabled {
                tauri::async_runtime::block_on(run_check(&state, &settings));
                emit_status(&app);
            }
            // Danach schlafen — Intervall aus der (frisch gelesenen) Config.
            let (_, hours, _, _) = settings.updater_config();
            std::thread::sleep(Duration::from_secs(u64::from(hours) * 3600));
        }
    });
}

/// Status an alle Webviews senden (Config-View kann live nachziehen).
fn emit_status<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri::{Emitter, Manager};
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.emit("updater://status", ());
    }
}

/// Status-Sicht für die Commands.
fn status<R: tauri::Runtime>(
    _app: &tauri::AppHandle<R>,
    state: &UpdaterState,
    settings: &SettingsState,
) -> UpdaterStatus {
    let (enabled, interval, _, _) = settings.updater_config();
    UpdaterStatus {
        enabled,
        interval_hours: interval, // consumed below via the struct field
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        note: state.note(),
        checking: state.checking.load(Ordering::SeqCst),
        last_check_secs: *state.last_check_secs.lock().expect("updater ts lock"),
        available: state
            .available
            .lock()
            .expect("updater avail lock")
            .as_ref()
            .map(|a| {
                let (asset_name, _) = a.plan.asset();
                AvailableInfo {
                    version: a.version.clone(),
                    kind: a.plan.kind().as_str().to_owned(),
                    asset_name: asset_name.to_owned(),
                }
            }),
    }
}

/// Status der UI melden.
#[tauri::command]
pub fn updater_status(
    app: tauri::AppHandle,
    state: tauri::State<UpdaterState>,
    settings: tauri::State<SettingsState>,
) -> UpdaterStatus {
    status(&app, state.inner(), settings.inner())
}

/// Manuelles Prüfen (Button „Jetzt prüfen“ in der Config-View).
#[tauri::command]
pub async fn updater_check_now(
    app: tauri::AppHandle,
    state: tauri::State<'_, UpdaterState>,
    settings: tauri::State<'_, SettingsState>,
) -> Result<UpdaterStatus, String> {
    // Gegen Parallel-Checks: nur ein Lauf gleichzeitig.
    if state.checking.swap(true, Ordering::SeqCst) {
        return Ok(status(&app, state.inner(), settings.inner()));
    }
    state.checking.store(false, Ordering::SeqCst);
    state.set_note(err_key("update.note_checking"));
    run_check(&state, settings.inner()).await;
    emit_status(&app);
    Ok(status(&app, state.inner(), settings.inner()))
}

/// Windows: Setup SILENT (`/S`) als detached child-process starten und die
/// App sauber beenden — Entscheidung dokumentiert in docs/UPDATER.md.
#[cfg(windows)]
fn spawn_nsis_silent(installer: &std::path::Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    // `/S` silent + `/R`: NSIS-Template startet die App nach Abschluss der
    // Installation selbst neu (Sebo-Wunsch: nach dem Update wird das Overlay
    // wieder gestartet, nicht nur installiert).
    std::process::Command::new(installer)
        .arg("/S")
        .arg("/R")
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn()
        .map_err(|e| err_keyp("update.err_spawn_setup", json!({ "err": e.to_string() })))?;
    Ok(())
}

/// Linux: das neue AppImage atomar am laufenden Pfad ersetzen (alte
/// umbenennen → neue benennen → exec-Rechte → alte löschen) und das
/// Overlay relaunchen. Nutzerdaten sind nicht berührt (AppImage-Pfad ≠
/// Config-Dir). Schlägt ein Schritt fehl, bleibt die ALTE Datei am Platz
/// (der .old-Rename wird zurückgerollt) — nie kaputte Installation.
#[cfg(target_os = "linux")]
fn install_appimage(new_path: &std::path::Path, target: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let old = target.with_extension("AppImage.old");
    if old.exists() {
        let _ = std::fs::remove_file(&old);
    }
    std::fs::rename(target, &old).map_err(|e| format!("AppImage ersetzen (alt): {e}"))?;
    if let Err(e) = std::fs::rename(new_path, target) {
        // Rollback: alte Datei zurückbenennen — nie kaputt hinterlassen.
        let _ = std::fs::rename(&old, target);
        return Err(format!("AppImage ersetzen (neu): {e}"));
    }
    std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("AppImage exec-Rechte: {e}"))?;
    let _ = std::fs::remove_file(&old);
    std::process::Command::new(target)
        .spawn()
        .map_err(|e| format!("Relaunch fehlgeschlagen: {e}"))?;
    Ok(())
}

/// Linux Bare-Binary (TH t_df9646a3): das verifizierte Binary-Asset mit
/// exec-Rechten atomar über die laufende exe renamen (rename über die
/// aktive exe geht auf Linux). KEIN Auto-Restart, kein systemctl-Eingriff
/// aus der App — die App zeigt nur den Hinweis „Neustart des Overlays
/// anwenden lassen“ (systemd restart oder Overlay schließen/öffnen).
#[cfg(target_os = "linux")]
fn install_bare_bin(new_path: &std::path::Path, target: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let old = target.with_extension("bin.old");
    if old.exists() {
        let _ = std::fs::remove_file(&old);
    }
    std::fs::rename(target, &old).map_err(|e| format!("Binary ersetzen (alt): {e}"))?;
    if let Err(e) = std::fs::rename(new_path, target) {
        // Rollback: alte Datei zurückbenennen — nie kaputt hinterlassen.
        let _ = std::fs::rename(&old, target);
        return Err(format!("Binary ersetzen (neu): {e}"));
    }
    std::fs::set_permissions(target, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("Binary exec-Rechte: {e}"))?;
    let _ = std::fs::remove_file(&old);
    Ok(())
}

/// On-demand Install (Karte Umsetzung 3/4): der Button-Klick IST die
/// Bestätigung. Windows → setup.exe (silent child) + Exit; Linux AppImage →
/// atomar ersetzen + Relaunch + Exit; Linux Bare-Binary → atomar ersetzen
/// + Neustart-Hinweis (KEIN Auto-Restart/Exit); .deb → Err mit dem apt-Hint.
#[tauri::command]
pub async fn updater_install(
    app: tauri::AppHandle,
    state: tauri::State<'_, UpdaterState>,
) -> Result<UpdaterStatus, String> {
    use tauri::Manager;
    let Some(avail) = state.available.lock().expect("updater avail lock").clone() else {
        return Err(err_key("update.err_none_available"));
    };
    state.set_note(err_keyp(
        "update.note_downloading",
        json!({ "version": avail.version }),
    ));
    if let Plan::DebHint { .. } = &avail.plan {
        // Umsetzung 4: keine root-Aktionen aus der App — UI zeigt den
        // apt-Hint (bereits in der Note); hier nur der Guards-Fehler.
        let hint = avail
            .plan
            .deb_hint()
            .unwrap_or_else(|| err_key("update.err_apt_hint_missing"));
        let msg = hint.clone();
        state.set_note(msg.clone());
        return Err(hint);
    }
    let new_path = updater::perform_download(&avail.plan).await.map_err(|e| {
        // Umsetzung 6: Fehlerpfad — alte Version bleibt intakt, Note meldet.
        let msg = friendly(&e);
        log::warn!("updater: download failed: {msg}");
        state.set_note(msg.clone()); // key-JSON direkt (kein Wrapping)
        msg
    })?;
    match &avail.plan {
        Plan::WindowsNsis { .. } => {
            #[cfg(windows)]
            {
                spawn_nsis_silent(&new_path)?;
                state.set_note(err_keyp(
                    "update.note_nsis_done",
                    json!({ "version": avail.version }),
                ));
            }
            #[cfg(not(windows))]
            {
                let _ = new_path;
                return Err(err_key("update.err_windows_only"));
            }
        }
        Plan::LinuxAppImage { appimage_path, .. } => {
            #[cfg(target_os = "linux")]
            {
                let target = appimage_path.clone();
                install_appimage(&new_path, &target)?;
                state.set_note(format!(
                    "Update v{} installiert — Overlay wird neu gestartet.",
                    avail.version
                ));
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = new_path;
                let _ = appimage_path;
                return Err(err_key("update.err_appimage_only"));
            }
        }
        Plan::LinuxBin { exe_path, .. } => {
            #[cfg(target_os = "linux")]
            {
                let target = exe_path.clone();
                install_bare_bin(&new_path, &target)?;
                state.set_note(err_keyp(
                    "update.note_barebin_done",
                    json!({ "version": avail.version }),
                ));
                // TH t_df9646a3: kein Auto-Restart, kein systemctl aus der
                // App — nur Datei-Swap + Nutzerhinweis. Die App läuft WEITER.
                log::info!("updater: bare-bin install done -> {}", avail.version);
                let settings_state = app.state::<SettingsState>();
                return Ok(status(&app, state.inner(), settings_state.inner()));
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = new_path;
                let _ = exe_path;
                return Err(err_key("update.err_binary_only"));
            }
        }
        Plan::DebHint { .. } => unreachable!("oben geprüft"),
    }
    log::info!("updater: install done -> {}", avail.version);
    // Kurze Sichtbarkeit der Erfolgs-Note, dann beenden (on-demand; der
    // Nutzer hat mit dem Klick zugestimmt — Umsetzung 2).
    std::thread::sleep(Duration::from_millis(1200));
    app.exit(0);
    // Status vor dem geplanten Exit: die UI soll die Erfolgs-Note noch
    // sehen; nach dem `app.exit(0)` kehrt der Command ohnehin nicht mehr
    // in den Webview zurück.
    let settings_state = app.state::<SettingsState>();
    Ok(status(&app, state.inner(), settings_state.inner()))
}

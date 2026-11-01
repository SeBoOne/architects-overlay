//! # C2b (t_1adbc017): Hyprland/OmaSettings-Bind-Sync
//!
//! On Hyprland the overlay's toggle/click-through fire through COMPOSITOR
//! keybinds (the `global-shortcut` plugin is X11-only). Sebo manages those
//! binds with the Omarchy plugin `io.github.twiking.omasettings`:
//!
//! - store: `~/.config/omarchy/omasettings.json` → `bindings.added[]`
//! - rendered into `~/.config/hypr/bindings.lua` (marker block
//!   `-- >>> omasettings bindings`) and `hyprctl reload`d — all by the
//!   plugin's own `keys add/remove/disable` subcommands.
//!
//! This module makes the persisted C2a canonical combos the single source of
//! truth for those two binds. Rules (store hygiene, from the plugin's own
//! contract):
//!
//! 1. The store is NEVER edited as JSON — every change goes through the
//!    `omasettings keys ...` CLI with POSITIONAL args (no flags). The plugin
//!    then rewrites the store, re-renders the Lua block and reloads Hyprland.
//! 2. The command field stays quote-free (`arch-overlay toggle`) — nested
//!    quotes break the generated Lua silently.
//! 3. Nothing is ever written without the user's ONE-TIME consent
//!    (`[shortcuts] sync_omarchy`, default false) — and the first mutating
//!    write is preceded by a dated backup of the store.
//! 4. The sync only ever touches the two `arch-overlay …` entries. An old
//!    combo that OmaSettings had disabled (Omarchy defaults CTRL+SHIFT+U/O)
//!    is re-disabled after removal so it stays dead; a combo that was NOT
//!    disabled is only un-bound by removal — foreign binds are never
//!    silenced or rewritten.
//! 5. Detection: the Omarchy path activates only when the plugin binary
//!    exists AND a Hyprland session is live (`HYPRLAND_INSTANCE_SIGNATURE`).
//!    Otherwise the app writes NOTHING and the Settings UI points at the
//!    documented manual path in `docs/WAYLAND.md`.
//!
//! Key-token mapping (`to_system_combo`) was probed live on this machine
//! with `hyprctl eval hl.bind(...)` (Hyprland 0.56, Lua config): tokens that
//! are valid keysyms returned `ok`, invalid ones failed with
//! `Unknown keysym` (ENTER, PAGEUP, PRTSC, ARROWUP are invalid — RETURN,
//! PAGE_UP, PRINT, UP are the correct spellings). Modmasks verified live via
//! `hyprctl binds -j`: SHIFT=1, CTRL=4, ALT=8, SUPER=64.

use crate::i18n_err::{err_key, err_keyp};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const TOGGLE_COMMAND: &str = "arch-overlay toggle";
pub const CLICK_COMMAND: &str = "arch-overlay clickthrough";
pub const TOGGLE_DESCRIPTION: &str = "Architects Overlay: Palette ein-/ausblenden";
pub const CLICK_DESCRIPTION: &str = "Architects Overlay: Click-through umschalten";

const PLUGIN_BIN: &str = "omarchy/plugins/io.github.twiking.omasettings/bin/omasettings";
const STORE_FILE: &str = "omarchy/omasettings.json";
const BINDINGS_LUA: &str = "hypr/bindings.lua";

// ---- canonical combo <-> OmaSettings/Hyprland key string -------------------

/// Canonical combo (`Ctrl+Shift+U`) → OmaSettings key string (`CTRL + SHIFT + U`).
/// Unknown tokens are rejected with a German message — an unwritable combo
/// must surface in the Settings UI, never reach the Lua block half-mapped.
pub fn to_system_combo(canonical: &str) -> Result<String, String> {
    let mut parts: Vec<String> = Vec::new();
    for tok in canonical.split('+') {
        let up = tok.trim().to_ascii_uppercase();
        parts.push(match up.as_str() {
            "CTRL" => "CTRL".to_owned(),
            "ALT" => "ALT".to_owned(),
            "SHIFT" => "SHIFT".to_owned(),
            "SUPER" => "SUPER".to_owned(),
            _ => hyprland_key_token(&up)?,
        });
    }
    if parts.len() < 2 {
        return Err(err_key("shortcuts.err_need_mod"));
    }
    Ok(parts.join(" + "))
}

/// Haupttasten-Token → Hyprland-keysym-Schreibweise. Mapping empirisch gegen
/// den laufenden Hyprland belegt (modulinterner Test dokumentiert die
/// abgelehnten Varianten: ENTER/PAGEUP/PRTSC/ARROWUP → `Unknown keysym`).
fn hyprland_key_token(up: &str) -> Result<String, String> {
    // A-Z, 0-9, F1–F24 pass through unchanged (probed: K, 5, F1, F13, F24 ok).
    if up.len() == 1 && up.as_bytes()[0].is_ascii_alphabetic() {
        return Ok(up.to_owned());
    }
    if up.len() == 1 && up.as_bytes()[0].is_ascii_digit() {
        return Ok(up.to_owned());
    }
    if let Ok(n) = up[1..].parse::<u8>()
        && up.starts_with('F')
        && (1..=24).contains(&n)
    {
        return Ok(up.to_owned());
    }
    let mapped = match up {
        "SPACE" => "SPACE",
        "ENTER" => "RETURN",
        "TAB" => "TAB",
        "BACKSPACE" => "BACKSPACE",
        "DELETE" => "DELETE",
        "HOME" => "HOME",
        "END" => "END",
        "INSERT" => "INSERT",
        "PAGEUP" => "PAGE_UP",
        "PAGEDOWN" => "PAGE_DOWN",
        "PRINTSCREEN" => "PRINT",
        "SCROLLLOCK" => "SCROLL_LOCK",
        "CAPSLOCK" => "CAPS_LOCK",
        "PAUSE" => "PAUSE",
        "ARROWUP" => "UP",
        "ARROWDOWN" => "DOWN",
        "ARROWLEFT" => "LEFT",
        "ARROWRIGHT" => "RIGHT",
        "BACKQUOTE" => "GRAVE",
        "BACKSLASH" => "BACKSLASH",
        "BRACKETLEFT" => "BRACKETLEFT",
        "BRACKETRIGHT" => "BRACKETRIGHT",
        "COMMA" => "COMMA",
        "EQUAL" => "EQUAL",
        "MINUS" => "MINUS",
        "PERIOD" => "PERIOD",
        "QUOTE" => "APOSTROPHE",
        "SEMICOLON" => "SEMICOLON",
        "SLASH" => "SLASH",
        _ => {
            return Err(err_keyp("sync.err_unbindable_key", json!({ "key": up })));
        }
    };
    Ok(mapped.to_owned())
}

/// Normalisierte Form beider Schreibrichtungen für Vergleiche:
/// `ctrl+shift + u` / `CTRL SHIFT U` → `CTRL + SHIFT + U`.
pub fn normalize_system_keys(keys: &str) -> String {
    let toks: Vec<String> = keys
        .split(['+', ' '])
        .filter(|t| !t.trim().is_empty())
        .map(|t| t.trim().to_ascii_uppercase())
        .collect();
    toks.join(" + ")
}

/// `hyprctl binds`-modmask der kanonischen Mods (live belegt: CTRL+SHIFT=5,
/// SUPER+CTRL=68, SUPER+SHIFT=65, SUPER+ALT=72).
pub fn modmask(canonical: &str) -> u32 {
    let mut mask = 0u32;
    for tok in canonical.split('+') {
        match tok.trim().to_ascii_uppercase().as_str() {
            "SHIFT" => mask |= 1,
            "CTRL" => mask |= 4,
            "ALT" => mask |= 8,
            "SUPER" => mask |= 64,
            _ => {}
        }
    }
    mask
}

/// Vergleich von dumpedem Bind-Key gegen erwarteten Token: toleriert
/// `_`-Setzung/Großschrift des Compositors (`PAGE_UP` vs `Page_Up`).
fn bind_key_matches(dumped: &str, expected: &str) -> bool {
    let strip = |s: &str| s.to_ascii_uppercase().replace('_', "");
    strip(dumped) == strip(expected)
}

// ---- store reading (read-only; writes go through the CLI only) -------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedEntry {
    pub keys: String,
    pub description: String,
    pub command: String,
}

/// `bindings.added[]` aus dem OmaSettings-Store (purer String-Layer, damit
/// die Funktion ohne serde-Modelle für den Store testbar bleibt).
pub fn store_added(store_json: &str) -> Vec<AddedEntry> {
    let value: serde_json::Value = match serde_json::from_str(store_json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let Some(added) = value.get("bindings").and_then(|b| b.get("added")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(arr) = added.as_array() {
        for e in arr {
            out.push(AddedEntry {
                keys: e
                    .get("keys")
                    .and_then(|k| k.as_str())
                    .unwrap_or("")
                    .to_owned(),
                description: e
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_owned(),
                command: e
                    .get("command")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_owned(),
            });
        }
    }
    out
}

pub fn store_disabled(store_json: &str) -> Vec<String> {
    let value: serde_json::Value = match serde_json::from_str(store_json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let Some(disabled) = value.get("bindings").and_then(|b| b.get("disabled")) else {
        return Vec::new();
    };
    disabled
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|d| d.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Die dem Overlay-Command zugeordnete Combo im Store (normalisiert), oder
/// None wenn die App dort noch keinen Bind hat.
pub fn store_combo_for_command(store_json: &str, command: &str) -> Option<String> {
    store_added(store_json)
        .into_iter()
        .find(|e| e.command == command)
        .map(|e| normalize_system_keys(&e.keys))
}

// ---- sync planning (pure; unit-tested) -------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// `keys remove "<KEYS>"` — takes our added entry out (and, with the
    /// plugin's semantics, its disabled marker too).
    Remove { keys: String },
    /// `keys disable "<KEYS>"` — re-arms the unbind ONLY when the old combo
    /// was already disabled before (Omarchy-default silencing preserved).
    Disable { keys: String },
    /// `keys add "<KEYS>" "<description>" "<command>"` — the plugin's own
    /// add path (binds the new combo; also unbinds whatever Hyprland had).
    Add {
        keys: String,
        description: String,
        command: String,
    },
}

/// Plan for one overlay command slot. `target_keys` must already be
/// normalized. Empty plan = store already matches (no write, no backup).
pub fn plan_ops(store_json: &str, command: &str, target_keys: &str, description: &str) -> Vec<Op> {
    let target = normalize_system_keys(target_keys);
    let current = store_combo_for_command(store_json, command);
    if current.as_deref() == Some(target.as_str()) {
        return Vec::new();
    }
    let mut ops = Vec::new();
    if let Some(old) = current {
        let was_disabled = store_disabled(store_json)
            .iter()
            .any(|d| normalize_system_keys(d) == old);
        ops.push(Op::Remove { keys: old.clone() });
        if was_disabled {
            // Keep the silencing: the old combo must stay dead (Akzeptanz:
            // "altes Kürzel tot"), but only because IT was already dead —
            // never newly silence a foreign default.
            ops.push(Op::Disable { keys: old });
        }
    }
    ops.push(Op::Add {
        keys: target,
        description: description.to_owned(),
        command: command.to_owned(),
    });
    ops
}

// ---- CLI plumbing -----------------------------------------------------------

/// Paths of the Omarchy installation. Testable via explicit fields; the
/// production constructor resolves the real `~/.config` locations.
#[derive(Debug, Clone)]
pub struct OmarchyPaths {
    pub bin: PathBuf,
    pub store: PathBuf,
    pub bindings_lua: PathBuf,
    /// Overridable in tests so verification steps can be faked.
    pub hyprctl: PathBuf,
    pub lua: PathBuf,
}

fn user_config_dir() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
        && !xdg.is_empty()
    {
        return Some(PathBuf::from(xdg));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))
}

impl OmarchyPaths {
    /// Detect the Omarchy path: the plugin binary must exist. The Hyprland
    /// session check lives in `hyprland_session_active()` — both gates must
    /// pass before any write happens.
    pub fn detect() -> Option<OmarchyPaths> {
        let cfg = user_config_dir()?;
        let bin = cfg.join(PLUGIN_BIN);
        if !is_executable(&bin) {
            return None;
        }
        Some(OmarchyPaths {
            store: cfg.join(STORE_FILE),
            bindings_lua: cfg.join(BINDINGS_LUA),
            bin,
            hyprctl: PathBuf::from("hyprctl"),
            lua: PathBuf::from("lua"),
        })
    }

    pub fn store_json(&self) -> Result<String, String> {
        std::fs::read_to_string(&self.store).map_err(|e| {
            format!(
                "OmaSettings-Store nicht lesbar ({}): {e}",
                self.store.display()
            )
        })
    }

    /// Dated pre-write backup: `omasettings.json.bak-<YYYY-MM-DD>`, created
    /// once per day, only if the store actually exists. Returns the path.
    pub fn ensure_backup(&self) -> Result<Option<PathBuf>, String> {
        if !self.store.exists() {
            return Ok(None);
        }
        let date = today_date();
        let bak = self.store.with_extension(format!("json.bak-{date}"));
        if bak.exists() {
            return Ok(Some(bak));
        }
        std::fs::copy(&self.store, &bak)
            .map_err(|e| format!("Backup {} fehlgeschlagen: {e}", bak.display()))?;
        Ok(Some(bak))
    }

    fn run_keys(&self, args: &[&str]) -> Result<(), String> {
        let out = Command::new(&self.bin)
            .arg("keys")
            .args(args)
            .output()
            .map_err(|e| err_keyp("sync.err_omasettings_run", json!({ "err": e.to_string() })))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_owned();
            return Err(err_keyp(
                "sync.err_omasettings_keys",
                json!({ "args": args.join(" "), "err": err }),
            ));
        }
        Ok(())
    }

    /// Hyprland's live bind table as JSON, or None when hyprctl is
    /// unreachable (not a Hyprland session).
    pub fn hyprctl_binds(&self) -> Option<String> {
        let out = Command::new(&self.hyprctl)
            .arg("binds")
            .arg("-j")
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Lua syntax gate (Akzeptanz-Punkt): `assert(loadfile(bindings.lua))`.
    /// Skips with Ok when no lua binary is available — the check is an
    /// extra safety net, not a hard dependency of the plugin's own path.
    pub fn lua_syntax_ok(&self) -> Result<(), String> {
        if self.lua.as_os_str().is_empty() || Command::new(&self.lua).arg("-v").output().is_err() {
            return Ok(());
        }
        let expr = format!("assert(loadfile({:?}))", self.bindings_lua);
        let out = Command::new(&self.lua)
            .arg("-e")
            .arg(&expr)
            .output()
            .map_err(|e| err_keyp("sync.err_lua_run", json!({ "err": e.to_string() })))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "Lua-Syntaxcheck der generierten Bindings fehlgeschlagen: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }
}

/// Unix executable bit check. On non-Unix (Windows cross-compile) this
/// module never activates at runtime (no OmaSettings there), but it must
/// still compile: existence check only.
#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(md) => md.is_file() && md.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn today_date() -> String {
    // Days since Unix epoch → YYYY-MM-DD (no chrono dependency).
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Howard Hinnant's `civil_from_days` (Zeller-based, proleptic Gregorian).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn hyprland_session_active() -> bool {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
}

// ---- orchestration ----------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncOutcome {
    /// true when the store actually needed a change.
    pub changed: bool,
    /// German one-liner for the Settings UI / log.
    pub note: String,
}

/// Full sync for ONE overlay command slot: read store → plan → (backup once)
/// → run ops through the CLI → verify via hyprctl binds + Lua syntax gate.
/// Never mutates the store outside the plugin CLI. On plan==no-op it writes
/// nothing at all (startup sync stays idempotent).
pub fn sync_combo(
    paths: &OmarchyPaths,
    canonical: &str,
    command: &str,
    description: &str,
) -> Result<SyncOutcome, String> {
    let target = to_system_combo(canonical)?;
    let store = paths.store_json()?;
    let ops = plan_ops(&store, command, &target, description);
    if ops.is_empty() {
        return Ok(SyncOutcome {
            changed: false,
            note: String::new(),
        });
    }

    let backup = paths.ensure_backup()?;
    for op in &ops {
        match op {
            Op::Remove { keys } => paths.run_keys(&["remove", keys])?,
            Op::Disable { keys } => paths.run_keys(&["disable", keys])?,
            Op::Add {
                keys,
                description,
                command,
            } => paths.run_keys(&["add", keys, description, command])?,
        }
    }

    // Verification (Akzeptanz): the live bind table must carry our combo.
    // The plugin reloads Hyprland ASYNC (hyprctl reload returns before the
    // new bind table is live) — a single immediate query can still see the
    // OLD table (observed live t_1adbc017: startup drift aborted because the
    // verification raced the reload). Poll a few short rounds before failing.
    // T2 (t_c5904bcb): Notes als error_key + params — Frontend übersetzt
    // via trError (Fallback-Kette gewählt → en → raw Key). `note` wird in
    // jedem Pfad gesetzt (verified / noverify / backup), daher kein Initialwert.
    let mut note: String;
    let want_mask = modmask(canonical);
    let want_key =
        to_system_combo(canonical).map(|c| c.rsplit(" + ").next().unwrap_or("").to_owned())?;
    if paths.hyprctl_binds().is_some() {
        let mut last: Option<(String, u32)> = None; // key+mask of the seen row
        let mut seen_any = false;
        let mut verified = false;
        for attempt in 0..5 {
            let binds_json = match paths.hyprctl_binds() {
                Some(j) => j,
                None => break, // not a Hyprland session — verification skipped
            };
            match find_bind_by_description(&binds_json, description) {
                Some(b) => {
                    seen_any = true;
                    if b.modmask == want_mask && bind_key_matches(&b.key, &want_key) {
                        verified = true;
                        break;
                    }
                    last = Some((b.key, b.modmask));
                }
                None => last = None,
            }
            if !verified && attempt < 4 {
                std::thread::sleep(std::time::Duration::from_millis(600));
            }
        }
        if verified {
            note = err_keyp(
                "sync.compositor_verified",
                json!({ "command": command, "target": target }),
            );
        } else if seen_any {
            let (key, mask) = last.unwrap();
            return Err(err_keyp(
                "sync.err_mismatch",
                json!({ "command": command, "key": key, "mask": mask, "want_key": want_key, "want_mask": want_mask }),
            ));
        } else {
            return Err(err_keyp(
                "sync.err_nobind",
                json!({ "description": description }),
            ));
        }
    } else {
        note = err_keyp(
            "sync.compositor_ok_noverify",
            json!({ "command": command, "target": target }),
        );
    }

    if let Err(e) = paths.lua_syntax_ok() {
        return Err(err_keyp(
            "sync.err_lua_syntax",
            json!({
                "command": command,
                "err": e.to_string(),
                "backup": backup
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "omasettings keys remove".into()),
            }),
        ));
    }
    if let Some(p) = &backup {
        // Backup-Pfad hängt die UI als Param an (Key sync.compositor_backup).
        note = err_keyp(
            "sync.compositor_backup",
            json!({ "command": command, "target": target, "backup": p.display().to_string() }),
        );
    }
    Ok(SyncOutcome {
        changed: true,
        note,
    })
}

#[derive(Debug, Clone)]
/// Relevant identity of a dumped bind row: only key + modmask are ever
/// compared — the description is the lookup handle, not carried data.
struct DumpedBind {
    key: String,
    modmask: u32,
}

fn find_bind_by_description(binds_json: &str, description: &str) -> Option<DumpedBind> {
    let value: serde_json::Value = serde_json::from_str(binds_json).ok()?;
    let arr = value.as_array()?;
    arr.iter().find_map(|b| {
        let d = b.get("description").and_then(|d| d.as_str())?;
        if d != description {
            return None;
        }
        Some(DumpedBind {
            key: b
                .get("key")
                .and_then(|k| k.as_str())
                .unwrap_or("")
                .to_owned(),
            modmask: b.get("modmask").and_then(|m| m.as_u64()).unwrap_or(0) as u32,
        })
    })
}

/// Startup drift check for BOTH slots (card mechanism 1, Wayland branch):
/// sync only when the store combo differs from the persisted canonical combo.
/// One slot's failure must NOT abort the other slot (observed live
/// t_1adbc017: a verification error in slot 1 silently skipped slot 2 —
/// the store then carried half the Soll state). Errors become per-slot notes.
pub fn startup_sync_drift(
    paths: &OmarchyPaths,
    toggle: &str,
    click: &str,
) -> Result<Vec<SyncOutcome>, String> {
    let store = paths.store_json()?;
    let mut out = Vec::new();
    for (canonical, command, description) in [
        (toggle, TOGGLE_COMMAND, TOGGLE_DESCRIPTION),
        (click, CLICK_COMMAND, CLICK_DESCRIPTION),
    ] {
        let target = match to_system_combo(canonical) {
            Ok(t) => t,
            Err(e) => {
                out.push(SyncOutcome {
                    changed: false,
                    note: err_keyp(
                        "sync.compositor_skipped",
                        json!({ "command": command, "err": e }),
                    ),
                });
                continue;
            }
        };
        if store_combo_for_command(&store, command).as_deref() == Some(target.as_str()) {
            continue; // no drift → no write
        }
        let store_now = paths.store_json()?;
        let ops = plan_ops(&store_now, command, &target, description);
        if ops.is_empty() {
            continue;
        }
        match sync_combo(paths, canonical, command, description) {
            Ok(o) => out.push(o),
            Err(e) => out.push(SyncOutcome {
                changed: false,
                note: format!("Compositor-Sync {command} fehlgeschlagen: {e}"),
            }),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live-geprüfte Schreibweisen (hyprctl eval probe auf Sebos Hyprland):
    /// RETURN/PAGE_UP/PRINT/UP sind korrekt; ENTER/PAGEUP/PRTSC/ARROWUP sind
    /// keine Keysyms — unser Mapping muss sie übersetzen, nie durchreichen.
    #[test]
    fn canonical_maps_to_probed_hyprland_tokens() {
        assert_eq!(to_system_combo("Ctrl+Shift+U").unwrap(), "CTRL + SHIFT + U");
        assert_eq!(to_system_combo("Ctrl+Alt+K").unwrap(), "CTRL + ALT + K");
        assert_eq!(
            to_system_combo("Ctrl+Shift+Super+K").unwrap(),
            "CTRL + SHIFT + SUPER + K"
        );
        assert_eq!(to_system_combo("Alt+Space").unwrap(), "ALT + SPACE");
        assert_eq!(to_system_combo("Ctrl+Enter").unwrap(), "CTRL + RETURN");
        assert_eq!(to_system_combo("Ctrl+PageUp").unwrap(), "CTRL + PAGE_UP");
        assert_eq!(to_system_combo("Ctrl+PrintScreen").unwrap(), "CTRL + PRINT");
        assert_eq!(to_system_combo("Super+ArrowUp").unwrap(), "SUPER + UP");
        assert_eq!(to_system_combo("Ctrl+Quote").unwrap(), "CTRL + APOSTROPHE");
        assert_eq!(to_system_combo("Ctrl+F13").unwrap(), "CTRL + F13");
        assert_eq!(to_system_combo("Shift+5").unwrap(), "SHIFT + 5");
        assert_eq!(
            to_system_combo("Ctrl+Semicolon").unwrap(),
            "CTRL + SEMICOLON"
        );
        // Unbekannt → Fehler mit sichtbarer Meldung (landet im Settings-Feld).
        let err = to_system_combo("Ctrl+Hyperkey").unwrap_err();
        assert!(err.contains("HYPERKEY"), "message names the token: {err}");
    }

    #[test]
    fn normalize_and_modmask_match_live_values() {
        assert_eq!(normalize_system_keys("ctrl+shift + u"), "CTRL + SHIFT + U");
        assert_eq!(normalize_system_keys("CTRL SHIFT U"), "CTRL + SHIFT + U");
        // live gemessen: U/O (arch-overlay) = 5; OmarCalc SUPER+CTRL = 68.
        assert_eq!(modmask("Ctrl+Shift+U"), 5);
        assert_eq!(modmask("Ctrl+Alt+K"), 12);
        assert_eq!(modmask("Ctrl+Super"), 68);
        assert_eq!(modmask("Ctrl+Shift+Super+K"), 69);
        assert!(bind_key_matches("Page_Up", "PAGE_UP"));
        assert!(bind_key_matches("U", "U"));
        assert!(!bind_key_matches("K", "U"));
    }

    fn real_store() -> &'static str {
        // Echtes Store-Snippet (Ausschnitt aus ~/.config/omarchy/omasettings.json,
        // live verifiziert t_1adbc017): beide arch-overlay Binds vorhanden.
        r#"{
  "bindings": {
    "added": [
      {"keys": "SUPER + CTRL + Q", "description": "OmarCalc", "command": "/home/sebo/.local/bin/omarcalc-v2.AppImage"},
      {"keys": "CTRL + SHIFT + U", "description": "Architects Overlay: Palette ein-/ausblenden", "command": "arch-overlay toggle"},
      {"keys": "CTRL + SHIFT + O", "description": "Architects Overlay: Click-through umschalten", "command": "arch-overlay clickthrough"}
    ],
    "disabled": ["CTRL + SHIFT + O", "CTRL + SHIFT + U", "SUPER + CTRL + Q"]
  }
}"#
    }

    #[test]
    fn store_reads_match_real_fixture() {
        assert_eq!(
            store_combo_for_command(real_store(), TOGGLE_COMMAND).as_deref(),
            Some("CTRL + SHIFT + U")
        );
        assert_eq!(
            store_combo_for_command(real_store(), CLICK_COMMAND).as_deref(),
            Some("CTRL + SHIFT + O")
        );
        assert_eq!(store_combo_for_command(real_store(), "other cmd"), None);
        assert_eq!(store_disabled(real_store()).len(), 3);
    }

    #[test]
    fn plan_noop_when_store_already_matches() {
        let ops = plan_ops(
            real_store(),
            TOGGLE_COMMAND,
            "CTRL + SHIFT + U",
            TOGGLE_DESCRIPTION,
        );
        assert!(ops.is_empty(), "kein Drift → keine Schreibvorgänge");
    }

    #[test]
    fn plan_replaces_entry_and_preserves_disabled_state() {
        // U ist disabled (Omarchy-Default wurde bereits stummgeschaltet) →
        // remove + disable: altes Kürzel bleibt tot.
        let ops = plan_ops(
            real_store(),
            TOGGLE_COMMAND,
            "CTRL + ALT + K",
            TOGGLE_DESCRIPTION,
        );
        assert_eq!(
            ops,
            vec![
                Op::Remove {
                    keys: "CTRL + SHIFT + U".into()
                },
                Op::Disable {
                    keys: "CTRL + SHIFT + U".into()
                },
                Op::Add {
                    keys: "CTRL + ALT + K".into(),
                    description: TOGGLE_DESCRIPTION.into(),
                    command: TOGGLE_COMMAND.into(),
                },
            ]
        );

        // Fremdes Store-Szenario: Bind war NICHT disabled → nur remove
        // (fremde Defaults werden nicht neu stummgeschaltet).
        let store = r#"{"bindings": {"added": [{"keys": "CTRL + ALT + J", "description": "x", "command": "arch-overlay toggle"}], "disabled": []}}"#;
        let ops = plan_ops(store, TOGGLE_COMMAND, "CTRL + ALT + K", TOGGLE_DESCRIPTION);
        assert_eq!(
            ops,
            vec![
                Op::Remove {
                    keys: "CTRL + ALT + J".into()
                },
                Op::Add {
                    keys: "CTRL + ALT + K".into(),
                    description: TOGGLE_DESCRIPTION.into(),
                    command: TOGGLE_COMMAND.into(),
                },
            ]
        );

        // App hatte noch gar keinen Bind: nur add.
        let store2 = r#"{"bindings": {"added": [], "disabled": []}}"#;
        let ops2 = plan_ops(store2, CLICK_COMMAND, "CTRL + SHIFT + O", CLICK_DESCRIPTION);
        assert_eq!(
            ops2,
            vec![Op::Add {
                keys: "CTRL + SHIFT + O".into(),
                description: CLICK_DESCRIPTION.into(),
                command: CLICK_COMMAND.into(),
            }]
        );
    }

    #[test]
    fn binds_verification_uses_real_dump_shape() {
        // Echtes Fragment aus `hyprctl binds -j` (live, t_1adbc017).
        let binds = r#"[
  {"modmask": 5, "key": "U", "description": "Architects Overlay: Palette ein-/ausblenden", "dispatcher": "__lua", "arg": "15"},
  {"modmask": 68, "key": "K", "description": "Herdr keybindings", "dispatcher": "__lua", "arg": "65"}
]"#;
        let b = find_bind_by_description(binds, TOGGLE_DESCRIPTION).expect("bind found");
        assert_eq!(b.key, "U");
        assert_eq!(b.modmask, 5); // live gemessene Modmaske CTRL+SHIFT
        assert_eq!(b.modmask, modmask("Ctrl+Shift+U"));
        assert!(
            find_bind_by_description(binds, "Architects Overlay: Click-through umschalten")
                .is_none()
        );
    }

    #[test]
    fn civil_date_is_stable() {
        // Epoch-Tag 20753 = 2026-10-27 (python gegenverifiziert).
        let (y, m, d) = civil_from_days(20753);
        assert_eq!((y, m, d), (2026, 10, 27));
        assert_eq!((civil_from_days(0)), (1970, 1, 1));
    }

    // End-to-end against a FAKE omasettings CLI + fake hyprctl in a tempdir:
    // proves the exact positional-arg sequence, backup-before-write, and the
    // verification step — without touching Sebo's real store. Shell-script
    // fakes = unix-only test.
    #[cfg(unix)]
    #[test]
    fn sync_combo_end_to_end_against_fake_cli() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = tmp.path();
        std::fs::create_dir_all(cfg.join("omarchy/plugins/x/bin")).unwrap();
        let bin = cfg.join("omarchy/plugins/x/bin/omasettings");
        let calls = cfg.join("calls.log");
        let store = cfg.join("omasettings.json");
        std::fs::write(&store, real_store()).unwrap();
        // Fake CLI: logs "<action> <args…>", no-op succeeds.
        std::fs::write(
            &bin,
            format!("#!/bin/sh\necho \"$@\" >> {}\nexit 0\n", calls.display()),
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        // Fake hyprctl: reports the NEW combo after the sync.
        let hyprctl = cfg.join("hyprctl");
        std::fs::write(
            &hyprctl,
            r#"#!/bin/sh
echo '[{"modmask": 12, "key": "K", "description": "Architects Overlay: Palette ein-/ausblenden", "dispatcher": "__lua", "arg": "99"}]'
"#,
        )
        .unwrap();
        std::fs::set_permissions(
            &hyprctl,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();

        let paths = OmarchyPaths {
            bin: bin.clone(),
            store: store.clone(),
            bindings_lua: cfg.join("bindings.lua"),
            hyprctl,
            lua: PathBuf::new(), // lua check skipped (no -v success)
        };

        let outcome = sync_combo(&paths, "Ctrl+Alt+K", TOGGLE_COMMAND, TOGGLE_DESCRIPTION).unwrap();
        assert!(outcome.changed);
        // T2 (t_c5904bcb): Note ist key-JSON — Params tragen die Fakten.
        let note: serde_json::Value = serde_json::from_str(&outcome.note).unwrap();
        assert!(note["error_key"].as_str().unwrap().starts_with("sync."));
        let params = note["params"].as_object().unwrap();
        assert!(
            params
                .values()
                .any(|v| v.as_str().unwrap_or("").contains("CTRL + ALT + K"))
        );
        assert!(
            params
                .values()
                .any(|v| v.as_str().unwrap_or("").contains("bak-"))
        );

        let log = std::fs::read_to_string(&calls).unwrap();
        let lines: Vec<&str> = log.lines().collect();
        assert_eq!(
            lines,
            vec![
                "keys remove CTRL + SHIFT + U",
                "keys disable CTRL + SHIFT + U",
                "keys add CTRL + ALT + K Architects Overlay: Palette ein-/ausblenden arch-overlay toggle",
            ],
            "positionale Args, exakte Reihenfolge"
        );
        // Backup existiert mit Datums-Endung.
        assert!(
            store
                .parent()
                .unwrap()
                .read_dir()
                .unwrap()
                .flatten()
                .any(|e| e
                    .file_name()
                    .to_string_lossy()
                    .starts_with("omasettings.json.bak-"))
        );

        // No-op-Drift: kein CLI-Aufruf, kein Backup-Schreiben nötig.
        let before = std::fs::read_to_string(&calls).unwrap();
        let noop = sync_combo(&paths, "Ctrl+Shift+O", CLICK_COMMAND, CLICK_DESCRIPTION).unwrap();
        assert!(!noop.changed);
        assert_eq!(std::fs::read_to_string(&calls).unwrap(), before);
    }

    #[cfg(unix)]
    #[test]
    fn verification_failure_is_reported_not_swallowed() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = tmp.path();
        std::fs::create_dir_all(cfg).unwrap();
        let bin = cfg.join("omasettings");
        std::fs::write(&bin, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&bin, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        let store = cfg.join("omasettings.json");
        std::fs::write(&store, real_store()).unwrap();
        // Fake hyprctl: Bind zeigt weiter die ALTE Combo → verification fails.
        let hyprctl = cfg.join("hyprctl");
        std::fs::write(
            &hyprctl,
            r#"#!/bin/sh
echo '[{"modmask": 5, "key": "U", "description": "Architects Overlay: Palette ein-/ausblenden", "dispatcher": "__lua", "arg": "15"}]'
"#,
        )
        .unwrap();
        std::fs::set_permissions(
            &hyprctl,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();

        let paths = OmarchyPaths {
            bin,
            store,
            bindings_lua: cfg.join("bindings.lua"),
            hyprctl,
            lua: PathBuf::new(),
        };
        let err = sync_combo(&paths, "Ctrl+Alt+K", TOGGLE_COMMAND, TOGGLE_DESCRIPTION).unwrap_err();
        // T2: Fehler als key-JSON — Key + Params-Struktur prüfen.
        let v: serde_json::Value = serde_json::from_str(&err).unwrap();
        assert_eq!(v["error_key"], "sync.err_mismatch", "got {err}");
        assert!(v["params"]["want_key"].is_string() && v["params"]["want_mask"].is_u64());
    }

    // Drift-Sync bricht NICHT ab, wenn ein Slot verifizierungs-seitig fällt:
    // der andere Slot wird trotzdem geschrieben (live-Beobachtung t_1adbc017:
    // Toggle-Slot verifizierte nicht, Click-through blieb halbvoll im Store).
    #[cfg(unix)]
    #[test]
    fn startup_drift_continues_after_slot_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = tmp.path();
        std::fs::create_dir_all(cfg).unwrap();
        let bin = cfg.join("omasettings");
        let calls = cfg.join("calls.log");
        std::fs::write(
            &bin,
            format!("#!/bin/sh\necho \"$@\" >> {}\nexit 0\n", calls.display()),
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        let store = cfg.join("omasettings.json");
        std::fs::write(&store, real_store()).unwrap();
        // Fake hyprctl: Toggle zeigt weiter die ALTE Combo (Fehler),
        // Click-through zeigt die neue ALT+SPACE-Zeile (Erfolg).
        let hyprctl = cfg.join("hyprctl");
        std::fs::write(
            &hyprctl,
            r#"#!/bin/sh
echo '[{"modmask": 5, "key": "U", "description": "Architects Overlay: Palette ein-/ausblenden", "dispatcher": "__lua", "arg": "15"}, {"modmask": 8, "key": "Space", "description": "Architects Overlay: Click-through umschalten", "dispatcher": "__lua", "arg": "16"}]'
"#,
        )
        .unwrap();
        std::fs::set_permissions(
            &hyprctl,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        let paths = OmarchyPaths {
            bin,
            store,
            bindings_lua: cfg.join("bindings.lua"),
            hyprctl,
            lua: PathBuf::new(),
        };
        // Drift: toggle -> Ctrl+Alt+K (Verifikation fällt), click -> Alt+Space (gelingt).
        let outcomes = startup_sync_drift(&paths, "Ctrl+Alt+K", "Alt+Space").unwrap();
        assert_eq!(outcomes.len(), 2, "beide Slots berichten");
        let notes: String = outcomes
            .iter()
            .map(|o| o.note.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            notes.contains("arch-overlay toggle fehlgeschlagen"),
            "Slot-1-Fehler als Note: {notes}"
        );
        assert!(
            notes.contains("arch-overlay clickthrough"),
            "Slot-2-Note vorhanden: {notes}"
        );
        assert!(
            notes.contains("ALT + SPACE") && !notes.contains("clickthrough fehlgeschlagen"),
            "Slot 2 erfolgreich verifiziert: {notes}"
        );
        let log = std::fs::read_to_string(&calls).unwrap();
        // Slot 1 ops UND Slot 2 ops wurden beide über die CLI ausgeführt.
        assert!(
            log.contains("keys remove CTRL + SHIFT + U"),
            "slot1 ops: {log}"
        );
        assert!(log.contains("keys add CTRL + ALT + K"), "slot1 add: {log}");
        assert!(
            log.contains("keys remove CTRL + SHIFT + O"),
            "slot2 ops: {log}"
        );
        assert!(
            log.contains("keys add ALT + SPACE Architects Overlay: Click-through umschalten arch-overlay clickthrough"),
            "slot2 add: {log}"
        );
    }
}

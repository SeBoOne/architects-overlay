//! # game_detection
//!
//! Detects whether Star Citizen is running, so the overlay can show/hide itself.
//! Linux: scan `/proc` for the SC process (may run under Proton/LUG). Windows:
//! `tasklist` / `sysinfo` for StarCitizen.exe. Stub — real detection lands with
//! the Overlay-Fenster task.

/// True while the Star Citizen process is running.
pub fn is_game_running() -> bool {
    // TODO(game-detection): /proc scan on Linux, tasklist on Windows.
    false
}
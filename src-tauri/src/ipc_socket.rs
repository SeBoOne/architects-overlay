//! Local Unix-socket control interface.
//!
//! Purpose: under Wayland the `global-shortcut` plugin (X11-based) cannot
//! provide reliable global hotkeys, and compositors may restrict window
//! attributes set from within the app. The socket gives the desktop session a
//! first-class, composable way to drive the overlay instead — a Hyprland
//! keybind can simply exec a client command:
//!
//! ```ini
//! # ~/.config/hypr/hyprland.conf
//! bind = CTRL SHIFT, U, exec, arch-overlay toggle
//! bind = CTRL SHIFT, O, exec, arch-overlay clickthrough
//! ```
//!
//! Protocol: one command per line, a single reply line, then the connection
//! closes. Commands: `toggle` (visibility), `show`, `hide`, `clickthrough`,
//! `state`. Replies: `OK <state>` or `ERR <reason>`.
//!
//! The socket lives in `$XDG_RUNTIME_DIR` (per-session tmpfs, never on disk)
//! with mode 0700, so it is a purely local, same-user control plane.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::OnceLock;
use std::sync::atomic::Ordering;

use tauri::{AppHandle, Manager};

use crate::overlay::{self, OverlayState, set_click_through, toggle_visible};

/// Socket file name inside the session runtime dir.
const SOCKET_FILE: &str = "architects-overlay.sock";

/// Resolved socket path so the bundled client binary can find the app.
static SOCKET_PATH: OnceLock<std::path::PathBuf> = OnceLock::new();

/// Public socket path for the bundled CLI wrapper.
pub fn socket_path() -> Option<&'static std::path::PathBuf> {
    SOCKET_PATH.get()
}

/// Bind the control socket and spawn the accept loop. Fails soft: a socket
/// problem must never prevent the app from starting.
pub fn init(app: &AppHandle) {
    match bind_socket() {
        Ok((path, listener)) => {
            let _ = SOCKET_PATH.set(path);
            let handle = app.clone();
            std::thread::spawn(move || accept_loop(handle, listener));
        }
        Err(err) => {
            log::warn!("control socket unavailable ({err}) — Wayland keybinds via socket disabled");
        }
    }
}

/// Current overlay state as a one-line reply payload.
fn state_line(app: &AppHandle) -> String {
    match overlay::snapshot(app) {
        Some(s) => format!(
            "visible={} game_running={} click_through={}",
            s.visible, s.game_running, s.click_through
        ),
        None => "no state".into(),
    }
}

/// Execute one command line against the app.
fn handle_command(app: &AppHandle, cmd: &str) -> String {
    let cmd = cmd.trim();
    match cmd {
        "toggle" => {
            toggle_visible(app);
            format!("OK {}", state_line(app))
        }
        "show" | "hide" => {
            let Some(st) = app.try_state::<OverlayState>() else {
                return "ERR no state".into();
            };
            if !st.game_running.load(Ordering::SeqCst) {
                return "ERR game not running".into();
            }
            let want = cmd == "show";
            if st.user_visible.load(Ordering::SeqCst) != want {
                // Reuse the hotkey path for identical semantics.
                toggle_visible(app);
            }
            format!("OK {}", state_line(app))
        }
        "clickthrough" => {
            let next = match app.try_state::<OverlayState>() {
                Some(st) => !st.click_through.load(Ordering::SeqCst),
                None => true,
            };
            let applied = set_click_through(app, next);
            format!("OK click_through={applied}")
        }
        "state" => format!("OK {}", state_line(app)),
        other => format!("ERR unknown command {other:?}"),
    }
}

/// Bind the listening socket in the session runtime dir. A stale socket from
/// a previous crash is unlinked before binding.
fn bind_socket() -> Result<(std::path::PathBuf, UnixListener), String> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .ok_or("XDG_RUNTIME_DIR not set")?;
    let path = dir.join(SOCKET_FILE);
    let _ = std::fs::remove_file(&path);
    let listener =
        UnixListener::bind(&path).map_err(|e| format!("bind {}: {e}", path.display()))?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("chmod: {e}"))?;
    Ok((path, listener))
}

fn accept_loop(app: AppHandle, listener: UnixListener) {
    for stream in listener.incoming().flatten() {
        let reply = serve(&app, stream);
        if let Some(msg) = reply {
            log::debug!("socket command: {msg}");
        }
    }
}

/// Read one command line from the connection, execute it, write the reply.
fn serve(app: &AppHandle, mut stream: UnixStream) -> Option<String> {
    let mut line = String::new();
    BufReader::new(&stream)
        .read_line(&mut line)
        .ok()
        .filter(|n| *n > 0)?;
    let reply = handle_command(app, &line);
    let ok = writeln!(stream, "{reply}").is_ok();
    if ok { Some(reply) } else { None }
}

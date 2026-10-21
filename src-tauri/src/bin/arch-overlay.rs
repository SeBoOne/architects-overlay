//! Tiny CLI client for the running overlay's control socket.
//!
//! Usage: `arch-overlay <command>` — commands: toggle | show | hide |
//! clickthrough | state. Intended for desktop-session keybinds, primarily on
//! Wayland where global shortcuts are unreliable (see docs/WAYLAND.md).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

fn socket_path() -> Option<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(|dir| PathBuf::from(dir).join("architects-overlay.sock"))
}

fn main() {
    let cmd = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: arch-overlay <toggle|show|hide|clickthrough|state>");
        std::process::exit(2);
    });

    let path = match socket_path() {
        Some(p) => p,
        None => {
            eprintln!("XDG_RUNTIME_DIR not set — is this a desktop session?");
            std::process::exit(1);
        }
    };

    let mut stream = match UnixStream::connect(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot reach overlay socket {}: {e}", path.display());
            eprintln!("is the overlay app running?");
            std::process::exit(1);
        }
    };

    if writeln!(stream, "{cmd}").is_err() {
        eprintln!("failed to send command");
        std::process::exit(1);
    }

    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).ok();
    println!("{}", line.trim_end());
    if !line.starts_with("OK") {
        std::process::exit(1);
    }
}

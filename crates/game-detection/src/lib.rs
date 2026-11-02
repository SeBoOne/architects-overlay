//! # game_detection
//!
//! Detects whether Star Citizen is running, so the overlay can show/hide itself.
//!
//! - Linux: scans `/proc/<pid>/cmdline` for known Star Citizen tokens. Because
//!   SC often runs under Proton/LUG (Wine), we match substrings of the command
//!   line rather than a single native binary name — the Windows game process
//!   surfaces as a wine process whose `cmdline` still names `StarCitizen.exe`
//!   (or the RSI launcher / Spectre).
//! - Windows: native `Toolhelp32` process snapshot (no child process), with
//!   `tasklist` as fallback. The shell-out was the primary path before
//!   2026-11-02; application-control policies (AppLocker/WDAC/SmartScreen)
//!   that block spawning `tasklist.exe` blinded it silently — the overlay
//!   then behaved like "game not running" (hotkey dead, auto-hide).
//! - macOS: shells out to `ps -ax -o command=` and matches the same known
//!   tokens. There is no native macOS Star Citizen client; the branch exists
//!   so the overlay builds/behaves cleanly on Apple silicon and would still
//!   spot an SC process running under a compatibility layer (CrossOver etc.)
//!   whose command line names the Windows executables. Clean no-op match:
//!   without such a process the poll simply reports "not running".
//!
//! No telemetry: results are used locally only to show/hide the overlay window.

use std::fs::{read_dir, read_to_string};
use std::path::Path;
use std::process::Command;

/// Executable basenames that identify Star Citizen processes (uppercase; the
/// matcher case-folds the candidate command line up before comparing). Requiring
/// the trailing `.EXE` deliberately excludes directory / config-file references
/// (e.g. a game-path folder or `Star Citizen.xml`) — only actual executables
/// count. Covers the GameClient binary (StarCitizen.exe), the launcher and the
/// in-game Spectre browser, including how those appear under Proton/LUG (wine
/// keeps the Windows path in its command line).
const KNOWN_TOKENS: [&str; 6] = [
    "STARCITIZEN.EXE",
    "STARCITIZEN_LAUNCHER.EXE",
    "RSI LAUNCHER.EXE",
    "RSILAUNCHER.EXE",
    "RSI_SPECTRE.EXE",
    "RSI_SPECTRESYS.EXE",
];

/// Spawn a command without a visible console window on Windows.
///
/// GUI (subsystem:windows) binaries that shell out to console tools
/// (`tasklist`, `ps`, …) otherwise make Windows allocate a new console per
/// spawn — visible as a terminal flash on every game-detection poll. Applying
/// `CREATE_NO_WINDOW` suppresses that allocation. No-op on non-Windows.
pub fn hide_console(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = cmd;
    }
}

/// A candidate process to inspect.
pub struct ProcEntry {
    /// The process's decoded command line (argv joined on NUL).
    pub cmdline: String,
}

/// Is Star Citizen running, per the host platform?
pub fn is_game_running() -> bool {
    let procs = if cfg!(target_os = "windows") {
        windows_procs()
    } else if cfg!(target_os = "macos") {
        macos_procs()
    } else {
        linux_procs()
    };
    scan_candidates(procs.as_slice())
}

/// Scan a list of candidate processes for any Star Citizen token.
///
/// Split out so the /proc and tasklist backends share one matcher and so unit
/// tests can feed synthetic process lists.
pub fn scan_candidates(procs: &[ProcEntry]) -> bool {
    for proc in procs {
        if is_star_citizen(&proc.cmdline) {
            return true;
        }
    }
    false
}

/// Does this command line reference a Star Citizen process?
fn is_star_citizen(cmdline: &str) -> bool {
    let upper = cmdline.to_uppercase();
    for token in KNOWN_TOKENS {
        if upper.contains(token) {
            return true;
        }
    }
    false
}

/// Linux: enumerate `/proc/<pid>/cmdline` files.
fn linux_procs() -> Vec<ProcEntry> {
    let mut out: Vec<ProcEntry> = Vec::new();
    // `/proc` is always present on Linux; hard-code the mount point.
    if let Ok(iter) = read_dir(Path::new("/proc")) {
        for entry in iter.flatten() {
            let path = entry.path();
            // Only numeric PIDs are processes; protocol entries (self, sys,
            // net, …) are skipped.
            let stem = path.file_stem();
            if let Some(pid) = stem {
                if !pid
                    .as_encoded_bytes()
                    .iter()
                    .all(|b| *b >= b'0' && *b <= b'9')
                {
                    continue;
                }
            } else {
                continue;
            }
            // Command line may contain NUL separators / invalid UTF-8 on
            // read failure; both cases are treated as "no match".
            if let Ok(cmdline) = read_to_string(path.join("cmdline")) {
                out.push(ProcEntry { cmdline });
            }
        }
    }
    out
}

/// Windows: native Toolhelp32-Snapshot zuerst, `tasklist` als Fallback.
///
/// Der Toolhelp-Weg spawned KEINEN Kindprozess — Anwendungssteuerungs-
/// richtlinien (AppLocker/WDAC), die das Starten von `tasklist.exe` aus
/// unserem GUI-Prozess blocken, können die Erkennung nicht mehr blind
/// schlagen (Nutzer-Bug 2026-11-02: Hotkey tot + Auto-Hide, obwohl SC lief).
/// Compiled on every platform (so the Linux dev host type-checks it) but only
/// *executed* on Windows, where it is selected from `is_game_running` via
/// `cfg!(target_os = "windows")`.
fn windows_procs() -> Vec<ProcEntry> {
    if cfg!(test) {
        return Vec::new();
    }
    match toolhelp_procs() {
        Some(procs) if !procs.is_empty() => procs,
        // Snapshot leer/fehlgeschlagen → alte tasklist-Route als Rückfalle.
        _ => tasklist_procs(),
    }
}

#[cfg(windows)]
/// Vollständige Prozessliste per Toolhelp32 (exe-Namen, wie tasklist sie zeigt).
fn toolhelp_procs() -> Option<Vec<ProcEntry>> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut out: Vec<ProcEntry> = Vec::new();
        let mut ok = Process32FirstW(snapshot, &mut entry);
        while ok != 0 {
            // szExeFile ist ein fixed [u16; 260]-Buffer, NUL-terminiert.
            let end = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
            out.push(ProcEntry { cmdline: name });
            ok = Process32NextW(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
        Some(out)
    }
}

#[cfg(not(windows))]
/// Kein Toolhelp ohne Windows — der Dispatch auf Nicht-Windows-Plattformen
/// ruft diese Funktion nie; der Stub hält das Crate plattformneutral kompilierbar.
fn toolhelp_procs() -> Option<Vec<ProcEntry>> {
    None
}

#[cfg(windows)]
/// Fallback: `tasklist`-Kindprozess (wie vor 2026-11-02 primär).
fn tasklist_procs() -> Vec<ProcEntry> {
    let mut out: Vec<ProcEntry> = Vec::new();
    // tasklist /FO CSV /NH → `"IMAGE.EXE","PID","SESS","NAME","MEM",...`
    let mut cmd = Command::new("tasklist");
    cmd.arg("/FO").arg("CSV").arg("/NH");
    hide_console(&mut cmd);
    if let Ok(output) = cmd.output() {
        // Match against the raw line: the CSV row repeats the image name
        // verbatim, so substring matching suffices and avoids CSV parsing.
        if let Ok(stdout) = String::from_utf8(output.stdout) {
            for line in stdout.split('\n') {
                let line = line.trim_end_matches('\r').trim();
                if !line.is_empty() {
                    out.push(ProcEntry {
                        cmdline: line.to_string(),
                    });
                }
            }
        }
    }
    out
}

#[cfg(not(windows))]
/// tasklist-Kindprozess gibt es ohne Windows nicht — Stub für den
/// plattformneutralen Dispatch in `windows_procs`.
fn tasklist_procs() -> Vec<ProcEntry> {
    Vec::new()
}

/// macOS: run `ps -ax -o command=` and collect the raw command lines.
///
/// Compiled on every platform (so the Linux dev host type-checks it) but only
/// *executed* on macOS, where it is selected from `is_game_running` via
/// `cfg!(target_os = "macos")`. There is no native Star Citizen client for
/// macOS; this branch exists so the build stays clean on Apple silicon and a
/// compatibility-layer SC process (CrossOver etc.) is still spotted — same
/// substring matcher as the other backends.
fn macos_procs() -> Vec<ProcEntry> {
    let mut out: Vec<ProcEntry> = Vec::new();
    if cfg!(test) {
        return out;
    }
    // `ps -ax -o command=` → one command line per process, no header (the
    // trailing `=` suppresses the "COMMAND" header row).
    if let Ok(output) = Command::new("ps").args(["-ax", "-o", "command="]).output()
        && let Ok(stdout) = String::from_utf8(output.stdout)
    {
        for line in stdout.split('\n') {
            let line = line.trim();
            if !line.is_empty() {
                out.push(ProcEntry {
                    cmdline: line.to_string(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(cmdline: &str) -> ProcEntry {
        ProcEntry {
            cmdline: cmdline.to_string(),
        }
    }

    #[test]
    fn matches_macos_ps_style_command_line() {
        // `ps -ax -o command=` output style — the same tokens matched by the
        // /proc and tasklist backends, so the macOS poller shares one matcher.
        let lines = [
            "  PID TT  STAT      TIME COMMAND",
            "  123 ??  Ss    0:00.01 /usr/libexec/logd",
            "  456 ??  S      2:30.12 /Applications/StarCitizen.app/Contents/MacOS/StarCitizen.exe",
            "  789 ??  S      0:01.02 launchd",
        ];
        let procs: Vec<ProcEntry> = lines.iter().map(|l| proc(l)).collect();
        assert!(scan_candidates(&procs));
    }

    #[test]
    fn matches_star_citizen_win_exe() {
        assert!(scan_candidates(&[
            proc("StarCitizen.exe"),
            proc("explorer.exe")
        ]));
    }

    #[test]
    fn matches_wine_cmdline_with_full_path() {
        // Typical under Proton/LUG: wine keeps the Windows path in argv.
        let cmd = r"D:\Games\RobertsSpaceIndustries\StarCitizen.exe --something";
        assert!(scan_candidates(&[proc(cmd)]));
    }

    #[test]
    fn matches_rsi_launcher() {
        assert!(scan_candidates(&[
            proc("RSI Launcher.exe"),
            proc("cmd.exe")
        ]));
    }

    #[test]
    fn matches_in_case_insensitive_forms() {
        assert!(scan_candidates(&[proc("starcitizen.exe")]));
    }

    #[test]
    fn rejects_unrelated_processes() {
        assert!(!scan_candidates(&[
            proc("explorer.exe"),
            proc("systemd"),
            proc("Hyprland")
        ]));
    }

    #[test]
    fn empty_list_is_false() {
        assert!(!scan_candidates(&[]));
    }

    /// End-to-end check of the real `/proc` scanner on Linux: spawn a child
    /// whose argv names StarCitizen (as a wine/Proton game process would) and
    /// assert the scan flips from false → true while it runs and back to false
    /// after it exits. Runs in the Linux CI job too.
    #[cfg(target_os = "linux")]
    #[test]
    fn proc_scan_detects_spawned_star_citizen() {
        use std::time::Duration;

        // Environment guard: if the real game (or any StarCitizen-named
        // process, e.g. launcher/game under Wine) is already running, the
        // "flip off after the test child exits" assertion can never pass.
        // Skip instead of failing — the scanner is behaving correctly.
        if is_game_running() {
            eprintln!(
                "SKIP: a real StarCitizen process is running — e2e flip-off assertion not testable"
            );
            return;
        }

        // `exec -a` sets argv[0], so the child's /proc/<pid>/cmdline reads
        // "StarCitizen.exe". Lifecycle is bounded (sleep 4) so a failure can
        // never leak a lingering process.
        let spawned = std::process::Command::new("bash")
            .arg("-c")
            .arg("exec -a StarCitizen.exe sleep 4")
            .spawn()
            .ok();

        if let Some(mut child) = spawned {
            // Detection should flip on quickly after spawn.
            let mut seen_true = false;
            for _ in 0..60 {
                if is_game_running() {
                    seen_true = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            assert!(
                seen_true,
                "expected /proc scan to detect the spawned SC process"
            );

            // And flip off once the child (bounded to 4s) has exited.
            let mut seen_false = false;
            for _ in 0..120 {
                if !is_game_running() {
                    seen_false = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            assert!(
                seen_false,
                "expected /proc scan to stop detecting after SC exited"
            );
            let _ = child.wait();
        } else {
            panic!("failed to spawn the test child process");
        }
    }
}

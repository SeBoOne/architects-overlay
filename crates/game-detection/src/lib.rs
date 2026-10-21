//! # game_detection
//!
//! Detects whether Star Citizen is running, so the overlay can show/hide itself.
//!
//! - Linux: scans `/proc/<pid>/cmdline` for known Star Citizen tokens. Because
//!   SC often runs under Proton/LUG (Wine), we match substrings of the command
//!   line rather than a single native binary name — the Windows game process
//!   surfaces as a wine process whose `cmdline` still names `StarCitizen.exe`
//!   (or the RSI launcher / Spectre).
//! - Windows: shells out to `tasklist` and matches known image names.
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

/// A candidate process to inspect.
pub struct ProcEntry {
    /// The process's decoded command line (argv joined on NUL).
    pub cmdline: String,
}

/// Is Star Citizen running, per the host platform?
pub fn is_game_running() -> bool {
    let procs = if cfg!(target_os = "windows") {
        windows_procs()
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

/// Windows: run `tasklist` and collect the image names.
///
/// Compiled on every platform (so the Linux dev host type-checks it) but only
/// *executed* on Windows, where it is selected from `is_game_running` via
/// `cfg!(target_os = "windows")`.
/// TODO(ci): a Windows CI test for the tasklist path — the `#[cfg(test)]`
/// guard below keeps the function inert on Linux; a `#[cfg(target_os =
/// "windows")]` test spawning a renamed copy of an exe (mirroring
/// `proc_scan_detects_spawned_star_citizen`) can be added once a Windows
/// runner exercises the CI workflow.
fn windows_procs() -> Vec<ProcEntry> {
    let mut out: Vec<ProcEntry> = Vec::new();
    // tasklist /FO CSV /NH → `"IMAGE.EXE","PID","SESS","NAME","MEM",...`
    if cfg!(test) {
        return out;
    }
    if let Ok(output) = Command::new("tasklist")
        .arg("/FO")
        .arg("CSV")
        .arg("/NH")
        .output()
    {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(cmdline: &str) -> ProcEntry {
        ProcEntry {
            cmdline: cmdline.to_string(),
        }
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

//! Diagnose-Befehl für Game-Detection auf Windows — gibt den rohen
//! tasklist-Output + Erkennungs-Ergebnis zurück. Kann per IPC
//! `game_detection_diag` aufgerufen werden (Devtools-Konsole).
use crate::game_detection::{is_game_running, scan_candidates, ProcEntry};

#[tauri::command]
pub async fn game_detection_diag() -> serde_json::Value {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let mut cmd = Command::new("tasklist");
        cmd.arg("/FO").arg("CSV").arg("/NH");
        // CREATE_NO_WINDOW wie in game_detection.rs
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut result = serde_json::json!({
            "platform": "windows",
            "is_game_running": is_game_running(),
        });
        if let Ok(output) = cmd.output() {
            if let Ok(stdout) = String::from_utf8(output.stdout) {
                let lines: Vec<&str> = stdout.split('\n').map(|l| l.trim_end_matches('\r')).collect();
                result["tasklist_raw_lines"] = serde_json::json!(lines);
                let procs: Vec<ProcEntry> = lines.iter()
                    .filter(|l| !l.is_empty())
                    .map(|l| ProcEntry { cmdline: l.to_string() })
                    .collect();
                result["candidates_count"] = serde_json::json!(procs.len());
                result["scan_result"] = serde_json::json!(scan_candidates(&procs));
                // Zeige alle Prozesse, die einen Token enthalten (case-insensitive)
                let tokens = ["STARCITIZEN.EXE","STARCITIZEN_LAUNCHER.EXE","RSI LAUNCHER.EXE","RSILAUNCHER.EXE","RSI_SPECTRE.EXE","RSI_SPECTRESYS.EXE"];
                let matches: Vec<String> = procs.iter()
                    .filter(|p| tokens.iter().any(|t| p.cmdline.to_uppercase().contains(t)))
                    .map(|p| p.cmdline.clone())
                    .collect();
                result["matching_processes"] = serde_json::json!(matches);
            }
        }
        result
    }
    #[cfg(not(target_os = "windows"))]
    {
        serde_json::json!({ "platform": "non-windows", "note": "only windows implemented" })
    }
}

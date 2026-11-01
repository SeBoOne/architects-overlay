//! Streaming-Download mit Größenlimit + Tempfile (Karte Umsetzung 6).
//!
//! Fehlerpfade: keine Netzverbindung / HTTP-Fehler / halber Download →
//! die temporäre Datei wird verworfen, die alte Version bleibt intakt.
//! Geschrieben wird in eine `.part`-Datei neben dem Ziel; erst nach
//! vollständigem Download (Größenlimit enforced) wird benannt — der
//! Aufrufer (integrity::verify_or_fail) verifiziert SHA256, BEVOR die
//! Datei installiert wird.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::{Result, UpdaterError};

/// Hartes Download-Größenlimit (500 MiB): Schutz vor halben/Riesen-Downloads
/// (Karte Umsetzung 6). Reale Release-Assets liegen bei ~10–25 MiB.
pub const MAX_DOWNLOAD_BYTES: u64 = 500 * 1024 * 1024;

/// Streaming-Download nach `dir/<asset_name>` (über eine `.part`-Datei,
/// atomares Rename am Ende). `max_bytes` hartes Limit: überschreitet der
/// Body das Limit, bricht der Download ab — kein endloser/halber Download.
pub async fn download_asset(url: &str, dir: &Path, max_bytes: u64) -> Result<PathBuf> {
    if !url.starts_with("https://") {
        return Err(UpdaterError::InsecureUrl(url.to_owned()));
    }
    let file_name = url
        .rsplit('/')
        .next()
        .unwrap_or("update.bin")
        .split('?')
        .next()
        .unwrap_or("update.bin")
        .to_owned();
    if file_name.is_empty() {
        return Err(UpdaterError::Malformed(format!(
            "Asset-URL ohne Dateinamen: {url}"
        )));
    }
    let target = dir.join(&file_name);
    let part = dir.join(format!("{file_name}.part"));

    let client = reqwest::Client::builder()
        .user_agent(concat!(
            "architects-overlay-updater/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|e| UpdaterError::Network(e.to_string()))?;
    let mut resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| UpdaterError::Network(e.to_string()))?;
    let status = resp.status();
    if status.as_u16() == 429 {
        let _ = std::fs::remove_file(&part);
        return Err(UpdaterError::Network(
            "update.reason_rate_limit_retry".into(),
        ));
    }
    if !status.is_success() {
        let _ = std::fs::remove_file(&part);
        return Err(UpdaterError::Network(format!("HTTP {status}")));
    }

    // Content-Length-Frühcheck: ein zu großer Body bricht VOR dem Download ab.
    if let Some(len) = resp.content_length()
        && len > max_bytes
    {
        return Err(UpdaterError::TooLarge {
            limit_bytes: max_bytes,
        });
    }

    let mut file = std::fs::File::create(&part)?;
    let mut written: u64 = 0;
    while let Some(chunk) = resp.chunk().await.map_err(|e| {
        // Halber Download → part-Datei verwerfen (Umsetzung 6).
        let _ = std::fs::remove_file(&part);
        UpdaterError::Network(format!("Download abgebrochen: {e}"))
    })? {
        written += chunk.len() as u64;
        if written > max_bytes {
            let _ = std::fs::remove_file(&part);
            return Err(UpdaterError::TooLarge {
                limit_bytes: max_bytes,
            });
        }
        file.write_all(&chunk).map_err(|e| {
            let _ = std::fs::remove_file(&part);
            UpdaterError::Io(e.to_string())
        })?;
    }
    file.flush().map_err(|e| {
        let _ = std::fs::remove_file(&part);
        UpdaterError::Io(e.to_string())
    })?;
    drop(file);

    // Vorhandene Ziel-Datei (z. B. von einem früheren, fehlgeschlagenen
    // Lauf) überschreiben — das Ziel ist eine reine Temp-/Update-Datei.
    std::fs::rename(&part, &target).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        UpdaterError::Io(e.to_string())
    })?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_https() {
        let dir = tempfile::tempdir().unwrap();
        let err = tokio_download("http://example.com/x.bin", dir.path()).unwrap_err();
        assert!(matches!(err, UpdaterError::InsecureUrl(_)));
    }

    fn tokio_download(url: &str, dir: &Path) -> Result<PathBuf> {
        // Kleiner Test-Helper: ein eigener Runtime für den Einzeltest.
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(download_asset(url, dir, MAX_DOWNLOAD_BYTES))
    }
}

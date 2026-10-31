//! SHA256-Integrität (Karte Umsetzung 5): kein Trust-on-Download ohne Check.
//!
//! Quellen der Prüfsummen, in dieser Reihenfolge:
//! 1. Sidecar-Asset `SHA256SUMS` im Release (CI lädt es an v*-Tags hoch —
//!    Standardformat `"<hex>  <dateiname>"`, wie von `sha256sum -c` erzeugt)
//! 2. Release-Body (Fallback: `"<hex>  <dateiname>"`-Zeilen direkt im Text)
//!
//! Fehlt BEIDES für das Ziel-Asset → bewusste Ablehnung des Downloads
//! (`UpdaterError::NoChecksum`), nie „prüfungslos installieren“.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

use super::{Result, UpdaterError};

/// Name des Sidecar-Checksums-Assets, das der CI-Workflow (Linux- UND
/// Windows-Job) an die Release hängt.
pub const CHECKSUMS_ASSET: &str = "SHA256SUMS";

/// SHA256 einer Datei als Kleinbuchstaben-Hexstring.
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Parse `<hex>  <name>`-Zeilen (Standard-`sha256sum`-Ausgabe; auch ein
/// Leerzeichen zwischen den Feldern wird akzeptiert). `*`-Präfix beim Namen
/// (Binary-Modus) wird ignoriert. Kleinbuchstaben-Hex, 64 Zeichen.
pub fn parse_checksums(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(2, char::is_whitespace);
        let Some(hash) = parts.next() else { continue };
        let Some(rest) = parts.next() else { continue };
        let hash = hash.trim();
        if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let name = rest.trim().trim_start_matches('*').trim();
        if name.is_empty() {
            continue;
        }
        map.insert(name.to_owned(), hash.to_lowercase());
    }
    map
}

/// Prüfsumme für `asset_name` aus einer der Quellen holen.
/// Quellen: Sidecar-`SHA256SUMS`-Text (falls vorhanden), Release-Body.
pub fn find_checksum(
    asset_name: &str,
    checksums_asset: Option<&str>,
    release_body: &str,
) -> Option<String> {
    if let Some(text) = checksums_asset
        && let Some(h) = parse_checksums(text).get(asset_name)
    {
        return Some(h.clone());
    }
    parse_checksums(release_body).get(asset_name).cloned()
}

/// Downloaded-Datei verifizieren: `None`-Checksum (keine Quelle gefunden)
/// → Ablehnung; Mismatch → Ablehnung mit erwartet/tatsächlich. Die temporäre
/// Download-Datei wird bei Misserfolg verworfen (Umsetzung 6).
pub fn verify_or_fail(path: &Path, checksum: Option<&str>, asset_name: &str) -> Result<()> {
    let Some(expected) = checksum else {
        return Err(UpdaterError::NoChecksum(asset_name.to_owned()));
    };
    let actual = sha256_file(path)?;
    if !actual.eq_ignore_ascii_case(expected) {
        let _ = std::fs::remove_file(path);
        return Err(UpdaterError::ChecksumMismatch {
            expected: expected.to_lowercase(),
            actual,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vector() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.txt");
        std::fs::File::create(&p)
            .unwrap()
            .write_all(b"hello world")
            .unwrap();
        let h = sha256_file(&p).unwrap();
        assert_eq!(
            h,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn parses_checksums_lines() {
        let text = "# SHA256SUMS\n\
                    1111111111111111111111111111111111111111111111111111111111111111  architects-overlay_0.5.14_x64-setup.exe\n\
                    2222222222222222222222222222222222222222222222222222222222222222 *architects-overlay_0.5.14_amd64.AppImage\n\
                    short-line nope\n";
        let map = parse_checksums(text);
        assert_eq!(map.len(), 2);
        assert_eq!(
            map.get("architects-overlay_0.5.14_x64-setup.exe").unwrap(),
            "1111111111111111111111111111111111111111111111111111111111111111"
        );
        assert!(map.contains_key("architects-overlay_0.5.14_amd64.AppImage"));
    }

    #[test]
    fn find_checksum_prefers_sidecar_then_body() {
        let body = "9999999999999999999999999999999999999999999999999999999999999999  setup.exe";
        assert_eq!(
            find_checksum("setup.exe", None, body).unwrap(),
            "9999999999999999999999999999999999999999999999999999999999999999"
        );
        let sidecar = "aaaa111111111111111111111111111111111111111111111111111111111111  setup.exe";
        assert_eq!(
            find_checksum("setup.exe", Some(sidecar), body).unwrap(),
            "aaaa111111111111111111111111111111111111111111111111111111111111"
        );
        assert!(find_checksum("missing.exe", Some(sidecar), body).is_none());
    }
}

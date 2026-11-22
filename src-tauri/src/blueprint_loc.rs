//! M9-Q3 (Sebo): Übersetzungs-Korrektur für Blueprint-Erhalt aus Spiel-Logs.
//!
//! Nutzer können eine modifizierte `global.ini` geladen haben
//! (`<LIVE>/data/Localization/<lang>/global.ini` — Sprache aus `g_language`
//! in `user.cfg`/`USER.cfg`, Default `english`). Dann stimmen Log-Namen
//! (`Received Blueprint: …`) nicht mit den Strata-Katalog-Namen überein.
//! Auflösung: Wert (übersetzter Name) → Key in der Nutzer-`global.ini`,
//! dann Key → englischer Wert in der ORIGINALEN `global.ini` aus der
//! `Data.p4k` — und nochmal gegen den Katalog matchen.
//!
//! Die originale `global.ini` wird mit einem Rust-Port von `p4k_reader.py`
//! (Projekt „Star Citizen Localization Merger") aus der `Data.p4k`
//! extrahiert und unter `app_local_data_dir/blueprint-loc/` gecacht
//! (Meta = p4k-Größe + mtime — neue Spielversion = neue Extraktion).

use std::collections::HashMap;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

/// Default-Sprache, wenn `g_language` fehlt oder ausgekommentiert ist.
pub const DEFAULT_LANG: &str = "english";

// ---------------------------------------------------------------------------
// Sprache + Pfade
// ---------------------------------------------------------------------------

/// `g_language` aus `user.cfg`/`USER.cfg` im LIVE-Ordner lesen. Auch
/// auskommentierte Zeilen (`; g_language = english`) zählen als Hinweis —
/// ein auskommentierter Wert beschreibt trotzdem die aktive Sprache.
/// Rückgabe: Sprach-Ordnername (lowercase, Default `english`).
pub fn read_game_language(live_dir: &Path) -> String {
    for name in ["user.cfg", "USER.cfg"] {
        let path = live_dir.join(name);
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        for line in content.lines() {
            let line = line.trim();
            // Leading `;` Kommentar ignorieren — der Wert dahinter beschreibt
            // trotzdem die aktive Sprache (Nutzer-Beobachtung M9-Q3).
            let line = line.trim_start_matches(';').trim();
            if let Some(rest) = line.strip_prefix("g_language") {
                let rest = rest.trim();
                let Some(rest) = rest.strip_prefix('=') else {
                    continue;
                };
                let value = rest.trim();
                // Wert bis zum nächsten Whitespace/Kommentar — z. B.
                // `english` oder `english (INT)`.
                let token: String = value
                    .split(|c: char| c.is_whitespace() || c == ';' || c == '#')
                    .next()
                    .unwrap_or("")
                    .trim_matches('"')
                    .to_lowercase();
                if !token.is_empty() {
                    return token;
                }
            }
        }
    }
    DEFAULT_LANG.to_owned()
}

/// Kandidaten-Pfade zur Nutzer-`global.ini` (Windows ist case-insensitiv,
/// Linux nicht — daher mehrere Schreibweisen).
fn user_ini_candidates(live_dir: &Path, lang: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for data_dir in ["data", "Data"] {
        for loc_dir in ["Localization", "localization"] {
            out.push(
                live_dir
                    .join(data_dir)
                    .join(loc_dir)
                    .join(lang)
                    .join("global.ini"),
            );
        }
    }
    out
}

/// Nutzer-`global.ini` finden (Sprache aus user.cfg, Default english).
/// `None` = keine modifizierte Lokalisierung aktiv (dann ist kein Extra-
/// Schritt nötig).
pub fn find_user_global_ini(live_dir: &Path, lang: &str) -> Option<PathBuf> {
    for cand in user_ini_candidates(live_dir, lang) {
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

/// `Data.p4k` im LIVE-Ordner finden (case-insensitive — Linux-Wine-Prefix).
fn find_data_p4k(live_dir: &Path) -> Option<PathBuf> {
    let mut cand = live_dir.join("Data.p4k");
    if cand.is_file() {
        return Some(cand);
    }
    cand = live_dir.join("data.p4k");
    if cand.is_file() {
        return Some(cand);
    }
    // Letzter Ausweg: readdir (Dateisystem kann abweichen).
    if let Ok(rd) = std::fs::read_dir(live_dir) {
        for e in rd.filter_map(|e| e.ok()) {
            let name = e.file_name().to_string_lossy().to_lowercase();
            if name == "data.p4k" {
                return Some(e.path());
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// INI-Parsing
// ---------------------------------------------------------------------------

/// `key=value`-Paare aus einer global.ini. Kommentarzeilen (`;`/`#`) und
/// Sektions-Header (`[…]`) werden übersprungen. Der erste `=` trennt.
pub fn parse_ini_pairs(path: &Path) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Ok(content) = std::fs::read_to_string(path) else {
        return out;
    };
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with(';')
            || line.starts_with('#')
            || line.starts_with('[')
        {
            continue;
        }
        if let Some(eq) = line.find('=') {
            let key = line[..eq].trim();
            let value = line[eq + 1..].trim();
            if !key.is_empty() && !value.is_empty() {
                out.insert(key.to_lowercase(), value.to_owned());
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// P4K-Reader (Rust-Port von p4k_reader.py, Localization-Merger-Projekt)
// ---------------------------------------------------------------------------

/// P4K = ZIP64 mit nicht-standard Preamble in der Central Directory.
/// Compression 100 = zstandard. Verschlüsselte Einträge: AES-256-CBC mit
/// fixem CIG-Key, Null-IV.
struct P4KEntry {
    filename: String,
    compress_type: u16,
    compressed_size: u64,
    file_size: u64,
    header_offset: u64,
    extra: Vec<u8>,
    is_encrypted: bool,
}

const ZIP_ZSTD: u16 = 100;
const ZIP_STORED: u16 = 0;
const ZIP_DEFLATED: u16 = 8;
const CD_SIG: [u8; 4] = *b"PK\x01\x02";
const EOCD_SIG: [u8; 4] = *b"PK\x05\x06";
const EOCD64_LOC_SIG: [u8; 4] = *b"PK\x06\x07";
const EOCD64_SIG: [u8; 4] = *b"PK\x06\x06";
const LFH_SIG: [u8; 4] = *b"PK\x03\x04";

struct P4KFile {
    path: PathBuf,
    entries: HashMap<String, usize>, // lowercase path → index
    list: Vec<P4KEntry>,
}

fn le_u16(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}
fn le_u32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}
fn le_u64(b: &[u8], off: usize) -> u64 {
    let mut arr = [0u8; 8];
    arr.copy_from_slice(&b[off..off + 8]);
    u64::from_le_bytes(arr)
}

/// ZIP64-Extended-Info aus dem Extra-Field auflösen (Tag 0x0001).
fn resolve_zip64(entry: &mut P4KEntry) {
    let extra = entry.extra.clone();
    let mut off = 0usize;
    while off + 4 < extra.len() {
        let tag = le_u16(&extra, off);
        let size = le_u16(&extra, off + 2) as usize;
        if tag == 0x0001 {
            let vals_start = off + 4;
            let mut idx = 0usize;
            let val = |i: usize| -> u64 { le_u64(&extra, vals_start + i * 8) };
            if entry.file_size == 0xFFFF_FFFF {
                entry.file_size = val(idx);
                idx += 1;
            }
            if entry.compressed_size == 0xFFFF_FFFF {
                entry.compressed_size = val(idx);
                idx += 1;
            }
            if entry.header_offset == 0xFFFF_FFFF {
                entry.header_offset = val(idx);
            }
            return;
        }
        off += 4 + size;
    }
}

impl P4KFile {
    fn open(path: &Path) -> Result<P4KFile, String> {
        let mut fp = std::fs::File::open(path).map_err(|e| format!("p4k open: {e}"))?;
        // EOCD finden: letzte 65.536 + 22 + 76 Bytes scannen (rückwärts).
        let tail_len = (65_536 + 22 + 76) as u64;
        let file_len = fp.metadata().map_err(|e| format!("p4k stat: {e}"))?.len();
        let read_len = tail_len.min(file_len);
        fp.seek(std::io::SeekFrom::End(-(read_len as i64)))
            .map_err(|e| format!("p4k seek: {e}"))?;
        let mut tail = vec![0u8; read_len as usize];
        fp.read_exact(&mut tail)
            .map_err(|e| format!("p4k read: {e}"))?;

        // EOCD (PK\x05\x06) von hinten suchen.
        let mut eocd_pos: Option<usize> = None;
        let mut i = tail.len().saturating_sub(22);
        while i + 22 <= tail.len() {
            if tail[i..i + 4] == EOCD_SIG {
                eocd_pos = Some(i);
                break;
            }
            i = i.saturating_sub(1);
        }
        let Some(eocd) = eocd_pos else {
            return Err("p4k: kein EOCD gefunden".into());
        };
        let mut size_cd = le_u32(&tail, eocd + 12) as u64;
        let mut offset_cd = le_u32(&tail, eocd + 16) as u64;

        // ZIP64: EOCD64-Locator (PK\x06\x07) direkt vor dem EOCD.
        if eocd >= 20 && tail[eocd - 20..eocd - 16] == EOCD64_LOC_SIG {
            let loc = eocd - 20;
            let eocd64_off = le_u64(&tail, loc + 8);
            // EOCD64-Direktiven lesen (PK\x06\x06): size(8) an +8, dann
            // size_cd an +40, offset_cd an +48.
            fp.seek(std::io::SeekFrom::Start(eocd64_off))
                .map_err(|e| format!("p4k seek64: {e}"))?;
            let mut hdr = vec![0u8; 56];
            fp.read_exact(&mut hdr)
                .map_err(|e| format!("p4k read64: {e}"))?;
            if hdr[0..4] == EOCD64_SIG {
                size_cd = le_u64(&hdr, 40);
                offset_cd = le_u64(&hdr, 48);
            }
        }

        // Concat-Offset (P4K-Preamble: Daten vor dem ersten ZIP-Header).
        let concat = (file_len - read_len) as u64 + eocd as u64 - size_cd - offset_cd;

        fp.seek(std::io::SeekFrom::Start(offset_cd + concat))
            .map_err(|e| format!("p4k cd seek: {e}"))?;
        let mut cd_data = vec![0u8; size_cd as usize];
        fp.read_exact(&mut cd_data)
            .map_err(|e| format!("p4k cd read: {e}"))?;

        let mut list: Vec<P4KEntry> = Vec::new();
        let mut entries: HashMap<String, usize> = HashMap::new();
        // P4K-Preamble: vor dem ersten PK\x01\x02 kann Daten liegen —
        // das erste CD-Signal suchen (wie p4k_reader.py).
        let first_pk = cd_data
            .windows(4)
            .position(|w| w == CD_SIG)
            .ok_or_else(|| "p4k: keine Central-Directory-Einträge".to_owned())?;
        let mut pos = first_pk;
        while pos + 46 <= cd_data.len() && cd_data[pos..pos + 4] == CD_SIG {
            let fn_len = le_u16(&cd_data, pos + 28) as usize;
            let extra_len = le_u16(&cd_data, pos + 30) as usize;
            let comment_len = le_u16(&cd_data, pos + 32) as usize;
            let entry_size = 46 + fn_len + extra_len + comment_len;
            if pos + entry_size > cd_data.len() {
                break;
            }
            let fn_raw = &cd_data[pos + 46..pos + 46 + fn_len];
            let flags = le_u16(&cd_data, pos + 8);
            let filename = if flags & 0x800 != 0 {
                String::from_utf8_lossy(fn_raw).into_owned()
            } else {
                fn_raw.iter().map(|&b| b as char).collect::<String>()
            };
            let mut entry = P4KEntry {
                filename,
                compress_type: le_u16(&cd_data, pos + 10),
                compressed_size: le_u32(&cd_data, pos + 20) as u64,
                file_size: le_u32(&cd_data, pos + 24) as u64,
                header_offset: le_u32(&cd_data, pos + 42) as u64,
                extra: cd_data[pos + 46 + fn_len..pos + 46 + fn_len + extra_len].to_vec(),
                is_encrypted: false,
            };
            // CIG-Encryption-Marker: Byte 168 im Extra-Field > 0.
            entry.is_encrypted = entry.extra.len() >= 169 && entry.extra[168] > 0x00;
            resolve_zip64(&mut entry);
            let key = entry.filename.replace('\\', "/").to_lowercase();
            entries.insert(key, list.len());
            list.push(entry);
            pos += entry_size;
        }
        if list.is_empty() {
            return Err("p4k: keine Central-Directory-Einträge".into());
        }
        Ok(P4KFile {
            path: path.to_path_buf(),
            entries,
            list,
        })
    }

    fn find(&self, pattern: &str) -> Option<usize> {
        let key = pattern.replace('\\', "/").to_lowercase();
        self.entries.get(&key).copied()
    }

    fn read(&self, idx: usize) -> Result<Vec<u8>, String> {
        let entry = &self.list[idx];
        let mut fp = std::fs::File::open(&self.path).map_err(|e| format!("p4k open: {e}"))?;
        fp.seek(std::io::SeekFrom::Start(entry.header_offset))
            .map_err(|e| format!("p4k lfh seek: {e}"))?;
        let mut lfh = vec![0u8; 30];
        fp.read_exact(&mut lfh)
            .map_err(|e| format!("p4k lfh: {e}"))?;
        if lfh[0..4] != LFH_SIG {
            return Err("p4k: kein Local-Header".into());
        }
        let local_fn_len = le_u16(&lfh, 26) as u64;
        let local_extra_len = le_u16(&lfh, 28) as u64;
        let data_start = entry.header_offset + 30 + local_fn_len + local_extra_len;
        fp.seek(std::io::SeekFrom::Start(data_start))
            .map_err(|e| format!("p4k data seek: {e}"))?;
        let mut compressed = vec![0u8; entry.compressed_size as usize];
        fp.read_exact(&mut compressed)
            .map_err(|e| format!("p4k data read: {e}"))?;

        if entry.is_encrypted {
            compressed = decrypt_cig(&compressed)?;
        }
        match entry.compress_type {
            ZIP_STORED => Ok(compressed),
            ZIP_DEFLATED => {
                let mut out = Vec::with_capacity(entry.file_size as usize);
                let mut dec = flate2::read::DeflateDecoder::new(&compressed[..]);
                dec.read_to_end(&mut out)
                    .map_err(|e| format!("p4k inflate: {e}"))?;
                Ok(out)
            }
            ZIP_ZSTD => zstd::bulk::decompress(&compressed, entry.file_size as usize + 4096)
                .map_err(|e| format!("p4k zstd: {e}")),
            other => Err(format!("p4k: Kompression {other} nicht unterstützt")),
        }
    }
}

/// AES-256-CBC mit dem fixen CIG-Key + Null-IV (Port von `_decrypt`).
const CIG_KEY: [u8; 32] = [
    0x5e, 0x7a, 0x20, 0x02, 0x30, 0x2e, 0xeb, 0x1a, 0x3b, 0xb6, 0x17, 0xc3, 0x0f, 0xde, 0x1e, 0x47,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

fn decrypt_cig(data: &[u8]) -> Result<Vec<u8>, String> {
    // NOTE: Der Python-Reader nutzt nur die ersten 16 Bytes des Key-Literals
    // als AES-128-Key (AES.new(key, …) mit 16-Byte-Schlüssel = AES-128).
    use aes::Aes128;
    use aes::cipher::{BlockDecrypt, KeyInit};
    let mut key = [0u8; 16];
    key.copy_from_slice(&CIG_KEY[..16]);
    let cipher = Aes128::new(&key.into());
    if data.len() % 16 != 0 {
        return Err("p4k: verschlüsselte Daten keine Blocklänge".into());
    }
    let mut prev = [0u8; 16];
    let mut out = Vec::with_capacity(data.len());
    for block in data.chunks(16) {
        let mut buf = [0u8; 16];
        buf.copy_from_slice(block);
        cipher.decrypt_block(&mut buf.into());
        for i in 0..16 {
            out.push(buf[i] ^ prev[i]);
        }
        prev.copy_from_slice(block);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Original-Global.ini: Cache + Extraktion
// ---------------------------------------------------------------------------

const P4K_LOCALIZATION_ENTRY: &str = "Data/Localization/english/global.ini";

/// Cache-Pfad für die originale global.ini (neben cache.sqlite3).
fn loc_cache_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    app.path()
        .app_local_data_dir()
        .ok()
        .map(|d| d.join("blueprint-loc"))
}

/// Originale (unmodifizierte) global.ini aus der Data.p4k extrahieren und
/// cachen. Cache-Invalidierung: p4k-Größe oder mtime anders (neue Version).
/// Rückgabe: Pfad zur gecachten Datei, oder `None` wenn die p4k fehlt.
pub fn ensure_original_global_ini(
    app: &tauri::AppHandle,
    live_dir: &Path,
) -> Result<Option<PathBuf>, String> {
    let Some(p4k) = find_data_p4k(live_dir) else {
        return Ok(None);
    };
    let meta = std::fs::metadata(&p4k).map_err(|e| format!("p4k stat: {e}"))?;
    let stamp = format!(
        "{}:{}",
        meta.len(),
        meta.modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0)
    );
    let Some(cache_dir) = loc_cache_dir(app) else {
        return Ok(None);
    };
    std::fs::create_dir_all(&cache_dir).map_err(|e| format!("cache dir: {e}"))?;
    let ini_path = cache_dir.join("global.ini");
    let meta_path = cache_dir.join("global.ini.meta");
    if ini_path.is_file() {
        if let Ok(saved) = std::fs::read_to_string(&meta_path) {
            if saved.trim() == stamp {
                return Ok(Some(ini_path));
            }
        }
    }
    let archive = P4KFile::open(&p4k)?;
    let idx = archive
        .find(P4K_LOCALIZATION_ENTRY)
        .ok_or_else(|| "p4k: Data/Localization/english/global.ini nicht gefunden".to_owned())?;
    let data = archive.read(idx)?;
    std::fs::write(&ini_path, &data).map_err(|e| format!("cache write: {e}"))?;
    std::fs::write(&meta_path, &stamp).map_err(|e| format!("cache meta write: {e}"))?;
    Ok(Some(ini_path))
}

// ---------------------------------------------------------------------------
// Namens-Auflösung: übersetzter Name → Key → englischer Name
// ---------------------------------------------------------------------------

/// Auflösungstabelle: übersetzter Name (lowercase) → englischer Name.
/// Aufbau: Nutzer-`global.ini` (value→key) + originale `global.ini`
/// (key→value). `None`-Rückgabe = keine modifizierte Lokalisierung aktiv
/// bzw. Extraktion fehlgeschlagen (dann bleibt der Log-Name unverändert).
pub fn build_translation_map(
    app: &tauri::AppHandle,
    live_dir: &Path,
) -> Option<HashMap<String, String>> {
    let lang = read_game_language(live_dir);
    let user_ini = find_user_global_ini(live_dir, &lang)?;
    let user_pairs = parse_ini_pairs(&user_ini);
    if user_pairs.is_empty() {
        return None;
    }
    let original_path = ensure_original_global_ini(app, live_dir).ok()??;
    let original_pairs = parse_ini_pairs(&original_path);
    if original_pairs.is_empty() {
        return None;
    }
    // value→key aus der Nutzer-Datei, dann key→value aus der Original-Datei.
    let mut by_value: HashMap<String, String> = HashMap::new();
    for (key, value) in &user_pairs {
        by_value
            .entry(value.to_lowercase())
            .or_insert_with(|| key.clone());
    }
    let mut map: HashMap<String, String> = HashMap::new();
    for (value_lc, key) in by_value {
        if let Some(original) = original_pairs.get(&key) {
            map.insert(value_lc, original.clone());
        }
    }
    Some(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_from_commented_line() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("user.cfg"),
            "; g_language = english\nsys_width=1920\n",
        )
        .unwrap();
        assert_eq!(read_game_language(dir.path()), "english");
    }

    #[test]
    fn language_active_line() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("user.cfg"), "g_language = german\n").unwrap();
        assert_eq!(read_game_language(dir.path()), "german");
    }

    #[test]
    fn language_default_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_game_language(dir.path()), "english");
    }

    #[test]
    fn user_ini_found_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        let ini = dir.path().join("data/Localization/english");
        std::fs::create_dir_all(&ini).unwrap();
        std::fs::write(ini.join("global.ini"), "a=b\n").unwrap();
        assert!(find_user_global_ini(dir.path(), "english").is_some());
    }

    #[test]
    fn user_ini_missing_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_user_global_ini(dir.path(), "english").is_none());
    }

    #[test]
    fn ini_pairs_skip_comments_and_sections() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("global.ini");
        std::fs::write(
            &p,
            "; comment\n[section]\nkey1=value one\nkey2=value two\n#hash\n",
        )
        .unwrap();
        let m = parse_ini_pairs(&p);
        assert_eq!(m.get("key1").unwrap(), "value one");
        assert_eq!(m.get("key2").unwrap(), "value two");
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn zip64_resolve_updates_sizes() {
        let mut extra = vec![0u8; 168];
        extra.extend_from_slice(&0x0001u16.to_le_bytes());
        extra.extend_from_slice(&24u16.to_le_bytes());
        extra.extend_from_slice(&123u64.to_le_bytes());
        extra.extend_from_slice(&456u64.to_le_bytes());
        extra.extend_from_slice(&789u64.to_le_bytes());
        let mut e = P4KEntry {
            filename: "x".into(),
            compress_type: 0,
            compressed_size: 0xFFFF_FFFF,
            file_size: 0xFFFF_FFFF,
            header_offset: 0xFFFF_FFFF,
            extra,
            is_encrypted: false,
        };
        resolve_zip64(&mut e);
        assert_eq!(e.file_size, 123);
        assert_eq!(e.compressed_size, 456);
        assert_eq!(e.header_offset, 789);
    }

    #[test]
    fn cig_decrypt_roundtrip() {
        use aes::Aes128;
        use aes::cipher::{BlockEncrypt, KeyInit};
        let mut key = [0u8; 16];
        key.copy_from_slice(&CIG_KEY[..16]);
        let cipher = Aes128::new(&key.into());
        let plaintext = b"0123456789abcdefSFLKJDHFLSKDJFH!"; // 32 bytes
        let mut encrypted = Vec::new();
        let mut prev = [0u8; 16];
        for block in plaintext.chunks(16) {
            let mut buf = [0u8; 16];
            buf.copy_from_slice(block);
            for i in 0..16 {
                buf[i] ^= prev[i];
            }
            cipher.encrypt_block(&mut buf.into());
            encrypted.extend_from_slice(&buf);
            prev = buf;
        }
        let dec = decrypt_cig(&encrypted).unwrap();
        assert_eq!(dec, plaintext.to_vec());
    }

    /// Manueller Realitäts-Check gegen die echte Installation (nur lokal):
    /// `cargo test -p architects-overlay real_installation -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_installation_end_to_end() {
        let live = PathBuf::from(
            "/home/sebo/Games/star-citizen/drive_c/Program Files/Roberts Space Industries/StarCitizen/LIVE",
        );
        let lang = read_game_language(&live);
        eprintln!("lang: {lang}");
        assert_eq!(lang, "english");
        let user_ini = find_user_global_ini(&live, &lang).expect("user global.ini");
        eprintln!("user ini: {}", user_ini.display());
        let p4k = find_data_p4k(&live).expect("Data.p4k");
        eprintln!("p4k: {}", p4k.display());
        let mut archive = P4KFile::open(&p4k).expect("open p4k");
        let idx = archive
            .find(P4K_LOCALIZATION_ENTRY)
            .expect("original entry in p4k");
        let data = archive.read(idx).expect("read original global.ini");
        eprintln!("original size: {} bytes", data.len());
        let cache = std::env::temp_dir().join("bp-loc-e2e");
        std::fs::create_dir_all(&cache).unwrap();
        let orig_path = cache.join("global.ini");
        std::fs::write(&orig_path, &data).unwrap();
        let user_pairs = parse_ini_pairs(&user_ini);
        let orig_pairs = parse_ini_pairs(&orig_path);
        eprintln!(
            "user pairs: {}, original pairs: {}",
            user_pairs.len(),
            orig_pairs.len()
        );
        // Sebo-Beispiel: "Ind/3/A FullSpec-Max" → Key → Original-Wert.
        // (parse_ini_pairs lowercased die Keys.)
        let key = "item_name_radr_chco_s03_fullspecmax";
        let user_val = user_pairs.get(key).expect("user key");
        let orig_val = orig_pairs.get(key).expect("original key");
        eprintln!("user value: {user_val:?} -> original value: {orig_val:?}");
        // value→key (user) + key→value (original) = Übersetzungs-Paar.
        let mut by_value = HashMap::new();
        for (k, v) in &user_pairs {
            by_value
                .entry(v.to_lowercase())
                .or_insert_with(|| k.clone());
        }
        let resolved = by_value
            .get(&user_val.to_lowercase())
            .and_then(|k| orig_pairs.get(k));
        eprintln!("resolved: {:?}", resolved);
        assert_eq!(resolved.map(|s| s.as_str()), Some(orig_val.as_str()));
    }
}

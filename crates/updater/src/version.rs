//! Semver-Vergleich inkl. Pre-release-Suffixe (Karte Umsetzung 1).
//!
//! Bewusst ohne `semver`-Crate: die Releases tragen Tags wie `v0.5.13` oder
//! `v0.6.0-rc.1`; ein kleiner, gut getesteter Comparator reicht und hält die
//! Dependency-Liste minimal (Workspace-Konvention: kleine, gerichtete Crates).
//!
//! Regeln:
//! - führendes `v` wird ignoriert
//! - Kern: drei numerische Segmente `MAJOR.MINOR.PATCH`; fehlende Segmente
//!   zählen 0 (`v0.5` == `v0.5.0`)
//! - nach dem Kern folgt optional ein Pre-release-Suffix (`-rc.1`, `-beta`).
//!   Eine Version MIT Suffix ist KLEINER als dieselbe Version ohne Suffix
//!   (semver: 1.0.0-rc.1 < 1.0.0). Identische Kerner werden also nur dann
//!   als „neuer“ gewertet, wenn die Suffixe differieren.
//! - Suffixe werden segmentweise verglichen: numerische Segmente numerisch,
//!   sonst lexikalisch; weniger Segmente zählt als kleiner (1.0.0-rc < 1.0.0-rc.1).

/// `Some` nur bei gültigem Kern; `(major, minor, patch, Some(suffix))`.
pub fn parse_version(raw: &str) -> Option<(u64, u64, u64, Option<String>)> {
    let s = raw.trim().trim_start_matches('v');
    let (core, suffix) = match s.split_once('-') {
        Some((c, r)) => (c, Some(r.to_owned())),
        None => (s, None),
    };
    // Build-Metadaten (+...) sind für den Vergleich irrelevant — abschneiden.
    let core = core.split('+').next().unwrap_or(core);
    let mut parts = [0u64; 3];
    let mut seen = 0;
    for (i, seg) in core.split('.').enumerate() {
        if i >= 3 {
            return None;
        }
        if seg.is_empty() || !seg.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        parts[i] = seg.parse::<u64>().ok()?;
        seen = i + 1;
    }
    if seen == 0 {
        return None;
    }
    Some((parts[0], parts[1], parts[2], suffix))
}

fn cmp_suffix(a: Option<&str>, b: Option<&str>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (None, None) => Ordering::Equal,
        // Kein Suffix = Release > Suffix (semver).
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => {
            // Segmentweise: numerische Segmente numerisch, sonst lexikalisch.
            let xs: Vec<&str> = x.split('.').collect();
            let ys: Vec<&str> = y.split('.').collect();
            for (px, py) in xs.iter().zip(ys.iter()) {
                let ox = px.parse::<u64>().ok();
                let oy = py.parse::<u64>().ok();
                let ord = match (ox, oy) {
                    (Some(nx), Some(ny)) => nx.cmp(&ny),
                    // Zahl < Nicht-Zahl (semver-Konvention).
                    (Some(_), None) => Ordering::Less,
                    (None, Some(_)) => Ordering::Greater,
                    (None, None) => px.cmp(py),
                };
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            xs.len().cmp(&ys.len())
        }
    }
}

fn cmp_versions(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (Some(pa), Some(pb)) = (parse_version(a), parse_version(b)) else {
        return Ordering::Equal; // unlesbar — keine Behauptung
    };
    let core = (pa.0, pa.1, pa.2).cmp(&(pb.0, pb.1, pb.2));
    if core != Ordering::Equal {
        return core;
    }
    cmp_suffix(pa.3.as_deref(), pb.3.as_deref())
}

/// `true` nur wenn `candidate` STRIKT neuer als `current` ist (semver,
/// Pre-release-Suffixe korrekt behandelt). Unlesbare Tags werden als
/// „nicht neuer“ behandelt — der Check zeigt dann einfach kein Update.
pub fn is_newer(current: &str, candidate: &str) -> bool {
    cmp_versions(current, candidate) == std::cmp::Ordering::Less
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tags() {
        assert_eq!(parse_version("v0.5.13"), Some((0, 5, 13, None)));
        assert_eq!(parse_version("0.5.13"), Some((0, 5, 13, None)));
        assert_eq!(
            parse_version("v0.6.0-rc.1"),
            Some((0, 6, 0, Some("rc.1".into())))
        );
        assert_eq!(parse_version("v0.5"), Some((0, 5, 0, None)));
        assert_eq!(parse_version("garbage"), None);
        assert_eq!(parse_version("v0.5.13+build.42"), Some((0, 5, 13, None)));
    }

    #[test]
    fn newer_core() {
        assert!(is_newer("0.5.13", "v0.5.14"));
        assert!(is_newer("0.5.13", "v1.0.0"));
        assert!(!is_newer("0.5.13", "v0.5.13"));
        assert!(!is_newer("0.5.13", "v0.5.12"));
        assert!(!is_newer("0.5.13", "garbage"));
        assert!(
            is_newer("0.5.13", "v0.6.0-rc.1"),
            "höherer Kern mit Pre-release ist trotzdem neuer"
        );
        assert!(
            !is_newer("0.6.0", "v0.6.0-rc.1"),
            "Pre-release ist kein Update für dieselbe Release-Version"
        );
    }

    #[test]
    fn prerelease_ordering() {
        assert!(is_newer("0.6.0-rc.1", "v0.6.0"));
        assert!(is_newer("0.6.0-rc.1", "v0.6.0-rc.2"));
        assert!(is_newer("0.6.0-rc", "v0.6.0-rc.1"));
        assert!(!is_newer("0.6.0", "v0.6.0-rc.9"));
        assert!(
            is_newer("0.6.0-alpha", "v0.6.0-beta"),
            "lexikalischer Fallback"
        );
        assert!(
            is_newer("0.6.0-rc.1", "v0.6.0-rc.1+build"),
            "Build-Metadaten egal"
        );
    }
}

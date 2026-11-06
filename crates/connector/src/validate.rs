//! Strenger Validator (docs/research/connector-system.md §2/§3):
//! Schema-Check via Deserialisierung, https-only, keine file://-Pfade,
//! Größenlimit. Zusätzliche Abweichungen sind **Warnungen** — die UI bietet
//! die bewusste „Warnung ignorieren“-Option (Verantwortung des Nutzers);
//! **Errors** blockieren Import immer.

use crate::flow::{AutocompleteRule, CondOp, Condition, Flow, Screen, ScreenType};
use crate::manifest::{
    AuthType, ConnectorManifest, FieldType, FormField, HttpMethod, Placement, PlacementKind, View,
    ViewType,
};
use serde_json::Value;

/// 1 MiB — Manifeste sind deklarativ, mehr braucht niemand.
pub const MAX_MANIFEST_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, PartialEq)]
pub enum Violation {
    /// Blockiert Import (auch mit „Warnung ignorieren“).
    Error(String),
    /// Nur mit bewusstem „Warnung ignorieren“ importierbar.
    Warning(String),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Report {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl Report {
    pub fn error(&mut self, msg: impl Into<String>) {
        self.errors.push(msg.into());
    }
    pub fn warn(&mut self, msg: impl Into<String>) {
        self.warnings.push(msg.into());
    }
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

impl From<Vec<Violation>> for Report {
    fn from(v: Vec<Violation>) -> Self {
        let mut r = Report::default();
        for item in v {
            match item {
                Violation::Error(m) => r.errors.push(m),
                Violation::Warning(m) => r.warnings.push(m),
            }
        }
        r
    }
}

/// Voller Durchlauf: Größenlimit → JSON-Schema (Deserialisierung) →
/// strukturelle Regeln. `bytes` = rohe Manifest-Datei.
pub fn validate_manifest(bytes: &[u8]) -> Report {
    let mut report = Report::default();

    if bytes.len() > MAX_MANIFEST_BYTES {
        report.error(format!(
            "manifest exceeds size limit of {} bytes ({} bytes)",
            MAX_MANIFEST_BYTES,
            bytes.len()
        ));
        return report;
    }

    let manifest: ConnectorManifest = match serde_json::from_slice(bytes) {
        Ok(m) => m,
        Err(e) => {
            report.error(format!("schema validation failed: {e}"));
            return report;
        }
    };

    check_structure(&manifest, &mut report);
    report
}

fn check_structure(m: &ConnectorManifest, report: &mut Report) {
    // ID: eindeutig stabil, ASCII
    let id_ok = !m.id.is_empty()
        && m.id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !id_ok {
        report.error(format!(
            "id '{}' must be non-empty ASCII [A-Za-z0-9_-]",
            m.id
        ));
    }

    // base_url: strikt https://
    let base = &m.base_url;
    if !base.starts_with("https://") {
        report.error(format!("base_url must be https:// (got '{base}')"));
    }
    if base.contains("file://")
        || base.contains("localhost.invalid") && !base.starts_with("https://")
    {
        report.error(format!("file:// or non-https path in base_url: '{base}'"));
    }
    for c in [' ', '\t', '\n'] {
        if base.contains(c) {
            report.warn(format!("base_url contains whitespace: '{base}'"));
            break;
        }
    }

    // Auth-Deklaration konsistent
    if let Some(auth) = &m.auth {
        match auth.auth_type {
            AuthType::Header => {
                if auth
                    .header
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or("")
                    .is_empty()
                {
                    report.error("auth.type=header requires non-empty 'header' name");
                }
                if auth
                    .value_source
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or("")
                    .is_empty()
                {
                    report.error("auth.type=header requires 'value_source' (settings key)");
                }
            }
            AuthType::None => {
                if auth.header.is_some() || auth.value_source.is_some() {
                    report.warn("auth.type=none but header/value_source present");
                }
            }
        }
    }

    check_placement(&m.placement, report);

    // Views: eindeutige IDs, Endpunkte https-relativ, form-Regeln
    let mut seen = std::collections::BTreeSet::new();
    for view in &m.views {
        if !seen.insert(view.id.as_str()) {
            report.error(format!("duplicate view id '{}'", view.id));
        }
        check_view(view, report);
    }

    if m.views.is_empty() && m.flows.iter().flatten().all(|f| f.screens.is_empty()) {
        report.warn("manifest declares no views");
    }
    // Flows (SF1): eindeutige IDs, gültige Screen-Referenzen, Typ-Regeln.
    check_flows(m, report);
    // Settings-Felder (Karte A / Spec 5.1): generische Plugin-Einstellungsgruppe.
    check_settings_fields(m, report);
    // Sessionscan (Karte B / Spec 5.3): „Meine …“-Landliste.
    check_scan(m, report);
    // Ersteller-Cockpit v1 (Spec 15): optionaler Create-Pfad — gleiche
    // Pfad-Strenge wie alle Endpunkte (base_url-relativ, keine Platzhalter:
    // create braucht keine Parameter, Body kommt aus dem Formular).
    if let Some(ce) = m.create_endpoint.as_deref() {
        if ce.trim().is_empty() || !ce.starts_with('/') {
            report.error("create_endpoint must be a path starting with '/'".to_owned());
        }
        if ce.contains("file://") || ce.starts_with("http") {
            report.error("create_endpoint must stay base_url-relative".to_owned());
        }
        if !extract_params(ce).is_empty() {
            report.error("create_endpoint must not contain placeholders".to_owned());
        }
    }
}

/// Regeln für `settings_fields` (SCLM v2 Karte A): Key-Format wie Auth-
/// `value_source` (ASCII-Stamm für die Settings-Speicherung), eindeutige Keys,
/// keine Kollision mit der Auth-Quelle. Label-de/en werden bereits durch das
/// Schema erzwungen (LocalizedText ist Pflicht) — leere Strings hier nachgeprüft.
fn check_settings_fields(m: &ConnectorManifest, report: &mut Report) {
    let auth_source = m
        .auth
        .as_ref()
        .and_then(|a| a.value_source.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let mut seen = std::collections::BTreeSet::new();
    for f in &m.settings_fields {
        let key = f.key.trim();
        if key.is_empty() {
            report.error("settings_fields: field key empty");
            continue;
        }
        let key_ok = key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !key_ok {
            report.error(format!(
                "settings_fields: key '{}' must be ASCII [A-Za-z0-9_-]",
                f.key
            ));
        }
        if !seen.insert(key) {
            report.error(format!("settings_fields: duplicate key '{}'", key));
        }
        if f.label.de.trim().is_empty() {
            report.error(format!("settings_fields: key '{}': label.de is empty", key));
        }
        if f.label.en.trim().is_empty() {
            report.error(format!("settings_fields: key '{}': label.en is empty", key));
        }
        if auth_source == Some(key) {
            report.error(format!(
                "settings_fields: key '{}' collides with auth.value_source",
                key
            ));
        }
    }
}

/// Regeln für `scan` (SCLM v2 Karte B / Spec 5.3): generischer Sessionscan.
/// Der Core ist generisch — geprüft wird nur Wohlgeformtheit: ID-Format,
/// Endpunkt-Regeln (relativ, Liste ohne Platzhalter, Detail genau `{code}`),
/// Header-Templates nur gegen `{code}`, Match-Key MUSS in settings_fields
/// deklariert sein (sonst gibt es nichts zu vergleichen), Kappen-Grenzen.
fn check_scan(m: &ConnectorManifest, report: &mut Report) {
    let Some(s) = &m.scan else { return };
    let tag = format!("scan '{}'", s.id);
    let id_ok = !s.id.trim().is_empty()
        && s.id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !id_ok {
        report.error(format!(
            "scan: id '{}' must be non-empty ASCII [A-Za-z0-9_-]",
            s.id
        ));
    }
    if m.views.iter().any(|v| v.id == s.id) {
        report.error(format!("{tag}: collides with view id"));
    }
    if s.title.de.trim().is_empty() || s.title.en.trim().is_empty() {
        report.error(format!("{tag}: title.de/en must not be empty"));
    }
    for (what, target) in [
        ("list_endpoint", s.list_endpoint.as_str()),
        ("detail_endpoint", s.detail_endpoint.as_str()),
    ] {
        if target.trim().is_empty() || !target.starts_with('/') {
            report.error(format!("{tag}: {what} must be a path starting with '/'"));
        }
        if target.contains("file://") || target.starts_with("http") {
            report.error(format!("{tag}: {what} must stay base_url-relative"));
        }
    }
    // Listen-Endpunkt hat keine Parameter; Detail-Endpunkt genau `{code}`.
    if !extract_params(&s.list_endpoint).is_empty() {
        report.error(format!(
            "{tag}: list_endpoint must not contain placeholders"
        ));
    }
    let detail_params = extract_params(&s.detail_endpoint);
    if detail_params != vec!["code".to_owned()] {
        report.error(format!(
            "{tag}: detail_endpoint must contain exactly one placeholder {{code}} (got {:?})",
            detail_params
        ));
    }
    // Ersteller-Arm (optional): gleicher Maßstab wie detail_endpoint.
    if let Some(extra) = s.creator_detail_endpoint.as_deref() {
        if extra.trim().is_empty() || !extra.starts_with('/') {
            report.error(format!(
                "{tag}: creator_detail_endpoint must be a path starting with '/'"
            ));
        }
        if extra.contains("file://") || extra.starts_with("http") {
            report.error(format!(
                "{tag}: creator_detail_endpoint must stay base_url-relative"
            ));
        }
        let extra_params = extract_params(extra);
        if extra_params != vec!["code".to_owned()] {
            report.error(format!(
                "{tag}: creator_detail_endpoint must contain exactly one placeholder {{code}} (got {:?})",
                extra_params
            ));
        }
    }
    for (name, template) in s.headers.iter().flatten() {
        if name.trim().is_empty() {
            report.error(format!("{tag}: empty header name"));
        }
        for param in extract_params(template) {
            if param != "code" {
                report.error(format!(
                    "{tag}: header template '{{{param}}}' is not bound — only {{code}} is available"
                ));
            }
        }
    }
    for (what, val) in [
        ("done_field", &s.done_field),
        ("code_field", &s.code_field),
        ("id_field", &s.id_field),
        ("name_field", &s.name_field),
        ("match_path", &s.match_path),
    ] {
        if val.trim().is_empty() {
            report.error(format!("{tag}: {what} is empty"));
        }
    }
    let key = s.match_setting_key.trim();
    let key_ok = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !key_ok {
        report.error(format!(
            "{tag}: match_setting_key '{}' must be ASCII [A-Za-z0-9_-]",
            s.match_setting_key
        ));
    } else if !m.settings_fields.iter().any(|f| f.key == key) {
        report.error(format!(
            "{tag}: match_setting_key '{}' is not declared in settings_fields",
            s.match_setting_key
        ));
    }
    if s.done_cap == 0 {
        report.error(format!("{tag}: done_cap must be >= 1"));
    } else if s.done_cap > 100 {
        report.error(format!(
            "{tag}: done_cap > 100 — scan would flood detail fetches"
        ));
    }
    if s.concurrency == 0 {
        report.error(format!("{tag}: concurrency must be >= 1"));
    } else if s.concurrency > 16 {
        report.warn(format!(
            "{tag}: concurrency {} clamped to 16 at runtime",
            s.concurrency
        ));
    }
    check_poll_bounds(report, &tag, s.poll_seconds);
}

/// poll_seconds-Deckel (Härtung t_3cfc7842 Finding 1): untere Grenze war
/// schon gewarnt, obere fehlte — ein Remote-gesteuertes Manifest konnte
/// poll_seconds = u64::MAX setzen und damit den Poll-Cache praktisch
/// unbegrenzt machen (TTL nie ablaufbar, nur force half). Warnung ab einem
/// Tag, Fehler ab ~10^7 s (115 Tage — kein plausibles Poll-Intervall).
/// Der Runtime-Deckel (poll_ttl_ms = saturating_mul) fängt zusätzlich
/// bereits gespeicherte Manifeste aus älteren Ständen ab.
fn check_poll_bounds(report: &mut Report, tag: &str, poll_seconds: Option<u64>) {
    const POLL_WARN_SECONDS: u64 = 86_400;
    const POLL_MAX_SECONDS: u64 = 10_000_000;
    if let Some(p) = poll_seconds {
        if p == 0 {
            report.warn(format!("{tag}: poll_seconds=0 — floody polling"));
        } else if p > POLL_MAX_SECONDS {
            report.error(format!(
                "{tag}: poll_seconds {p} > {POLL_MAX_SECONDS} — Poll-Cache wäre praktisch unbegrenzt (remote-gesteuerte Manifeste)"
            ));
        } else if p > POLL_WARN_SECONDS {
            report.warn(format!(
                "{tag}: poll_seconds {p} > {POLL_WARN_SECONDS} (1 Tag) — sehr träge Poll-Liste"
            ));
        }
    }
}

/// Flow-Regeln (SF1): IDs eindeutig, Entry existiert, Screen-Referenzen
/// (Transitions/Branches) zeigen nur innerhalb des eigenen Flows auf
/// existierende Screens, Typ-Spezifika (wait braucht exit, form braucht
/// form-Block, sonst Endpunkte), Autocomplete-Regeln wohlgeformt.
fn check_flows(m: &ConnectorManifest, report: &mut Report) {
    let mut flow_ids = std::collections::BTreeSet::new();
    // SF3: Screen-IDs über Flows hinweg beobachten — Duplikate sind mit
    // `<flow>:<screen>`-Qualifikation im Renderer erlaubt, aber der
    // unqualifizierte Fallback (erster Treffer) macht sie zur Falle für
    // externe Manifest-Autoren → Warnung.
    let mut screen_owners: std::collections::BTreeMap<&str, &str> =
        std::collections::BTreeMap::new();
    for flow in m.flows.iter().flatten() {
        if !seen_insert(&mut flow_ids, &flow.id) {
            report.error(format!("duplicate flow id '{}'", flow.id));
            continue;
        }
        for s in &flow.screens {
            match screen_owners.insert(s.id.as_str(), flow.id.as_str()) {
                Some(prev) if prev != flow.id.as_str() => report.warn(format!(
                    "screen id '{}' reused across flows ('{}' and '{}') — renderer resolves qualified '<flow>:<screen>'",
                    s.id, prev, flow.id
                )),
                _ => {}
            }
        }
        check_flow(flow, m, report);
    }
}

fn seen_insert<'a>(set: &mut std::collections::BTreeSet<&'a str>, id: &'a str) -> bool {
    if id.trim().is_empty() {
        return true; // empty-ID-Fehler wird am Screen selbst gemeldet
    }
    set.insert(id)
}

fn check_flow(flow: &Flow, m: &ConnectorManifest, report: &mut Report) {
    let fid = &flow.id;
    if flow.screens.is_empty() {
        report.error(format!("flow '{fid}': no screens"));
        return;
    }
    let ids: std::collections::BTreeSet<&str> =
        flow.screens.iter().map(|s| s.id.as_str()).collect();
    // Flow-Zustands-Keys (SF2): Pins aller Screens — Formular-Templates
    // dürfen darauf verweisen (Selektionen/Codes fließen screen-übergreifend).
    let pin_keys: std::collections::BTreeSet<&str> = flow
        .screens
        .iter()
        .flat_map(|s| s.pins.iter().flatten())
        .map(|p| p.as_str())
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    for s in &flow.screens {
        if !seen_insert(&mut seen, &s.id) {
            report.error(format!("flow '{fid}': duplicate screen id '{}'", s.id));
        }
        check_screen(fid, s, &ids, &pin_keys, report);
    }
    if flow.entry.trim().is_empty() {
        report.error(format!("flow '{fid}': empty entry"));
    } else if !ids.contains(flow.entry.as_str()) {
        report.error(format!(
            "flow '{fid}': entry '{}' is not a screen",
            flow.entry
        ));
    }
    // Ersteller-Cockpit v1 (Spec 15): flows[].tabs = 1–6 permanente
    // Sub-Reiter {id, screen, label{de,en}}. Kern prüft nur Wohlgeformtheit:
    // eindeutige ASCII-IDs, bekannte Screens, nicht leer beschriftet.
    if let Some(tabs) = &flow.tabs {
        if tabs.is_empty() || tabs.len() > 6 {
            report.error(format!(
                "flow '{fid}': tabs must have 1-6 entries (got {})",
                tabs.len()
            ));
        }
        let mut tab_ids = std::collections::BTreeSet::new();
        for t in tabs {
            if !ascii_key(&t.id) {
                report.error(format!(
                    "flow '{fid}': tab id '{}' must be ASCII [A-Za-z0-9_-]",
                    t.id
                ));
            } else if !tab_ids.insert(t.id.as_str()) {
                report.error(format!("flow '{fid}': duplicate tab id '{}'", t.id));
            }
            check_screen_id_ref(fid, "tab screen", &t.screen, &ids, report);
            if t.label.de.trim().is_empty() || t.label.en.trim().is_empty() {
                report.error(format!("flow '{fid}': tab '{}' has empty label", t.id));
            }
        }
    }
    // SCLM v2 (Spec 5.4): optionaler Code-Einstieg-Block `open`.
    if let Some(o) = &flow.open {
        check_flow_open(fid, o, &ids, m, report);
    }
}

fn ascii_key(key: &str) -> bool {
    !key.trim().is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Regeln für `flow.open` (SCLM v2 / Spec 5.4): generischer Code-Einstieg —
/// Endpunkt-Regeln, sid-Mapping-Pflicht (join-POST braucht die Session-ID),
/// Match-Key muss deklariert sein (sonst gibt es keinen Benutzernamen),
/// Start-Screens müssen existieren. Der Kern prüft Wohlgeformtheit, keinen
/// Connector-Sprech.
fn check_flow_open(
    flow_id: &str,
    o: &crate::flow::FlowOpen,
    ids: &std::collections::BTreeSet<&str>,
    m: &ConnectorManifest,
    report: &mut Report,
) {
    let tag = format!("flow '{flow_id}' open");
    if !ascii_key(&o.param) {
        report.error(format!(
            "{tag}: param '{}' must be ASCII [A-Za-z0-9_-]",
            o.param
        ));
    }
    if !o.session_endpoint.starts_with('/') || o.session_endpoint.contains("file://") {
        report.error(format!(
            "{tag}: session_endpoint must be a '/'-relative path"
        ));
    }
    let ep_params = extract_params(&o.session_endpoint);
    if ep_params != vec![o.param.clone()] {
        report.error(format!(
            "{tag}: session_endpoint must contain exactly one placeholder {{{}}} (got {:?})",
            o.param, ep_params
        ));
    }
    if !o.mapping.contains_key("sid") {
        report.error(format!(
            "{tag}: mapping must contain a 'sid' key — the join endpoint needs {{sid}}"
        ));
    }
    if o.done_path.trim().is_empty() {
        report.error(format!("{tag}: done_path is empty"));
    }
    // Ersteller-Cockpit v1: Einstiegs-Semantik — die Match-/Join-Felder
    // müssen ALLE vorhanden oder ALLE fehlend sein: Teilnehmer-Einstieg
    // (Auto-Join, Code pinnt als join_code) vs. Admin-Einstieg (Ersteller-
    // Code pinnt als admin_code, Auto-Join entfällt). Halbpaare sind Fehler.
    let join_fields = [
        o.match_path.as_ref(),
        o.id_path.as_ref(),
        o.match_setting_key.as_ref(),
        o.join_endpoint.as_ref(),
    ];
    let any_join = join_fields.iter().any(Option::is_some);
    let all_join = join_fields.iter().all(Option::is_some);
    if any_join && !all_join {
        report.error(format!(
            "{tag}: participant join fields (match_path, id_path, match_setting_key, join_endpoint) must all be present or all absent (admin entry)"
        ));
    }
    if all_join {
        for (what, val) in [
            ("match_path", o.match_path.as_deref().unwrap_or("")),
            ("id_path", o.id_path.as_deref().unwrap_or("")),
        ] {
            if val.trim().is_empty() {
                report.error(format!("{tag}: {what} is empty"));
            }
        }
        let key = o.match_setting_key.as_deref().unwrap_or("").trim();
        if !ascii_key(key) {
            report.error(format!(
                "{tag}: match_setting_key '{key}' must be ASCII [A-Za-z0-9_-]"
            ));
        } else if !m.settings_fields.iter().any(|f| f.key == key) {
            report.error(format!(
                "{tag}: match_setting_key '{key}' is not declared in settings_fields"
            ));
        }
        let je = o.join_endpoint.as_deref().unwrap_or("");
        if !je.starts_with('/') || je.contains("file://") {
            report.error(format!("{tag}: join_endpoint must be a '/'-relative path"));
        }
    }
    // Admin-Feld-Regeln (Ersteller-Cockpit v1):
    // - Teilnehmer-Einstieg: admin_path + admin_flow nur als Paar (Route
    //   zum Cockpit); eins ohne das andere ist eine halbe Regel.
    // - Admin-Einstieg: admin_flow ist unsinnig (der Flow IST das Cockpit),
    //   admin_path ist Pflicht-Guardrail — ohne Admin-Flag-Prüfung im
    //   Session-GET wäre jeder gültige join_code ein Fake-Ersteller-Code.
    // - admin_flow-Ziel: existierender Flow mit open-Block in Admin-
    //   Semantik (eigener admin_path), nicht der eigene Flow.
    match (o.admin_path.as_deref(), o.admin_flow.as_deref()) {
        (Some(p), Some(target)) => {
            if p.trim().is_empty() {
                report.error(format!("{tag}: admin_path is empty"));
            }
            if !all_join {
                report.error(format!(
                    "{tag}: admin_flow is only valid on a participant entry (all join fields) — this flow is an admin entry"
                ));
            } else if target == flow_id {
                report.error(format!("{tag}: admin_flow must not point to own flow"));
            } else {
                match m.flows.iter().flatten().find(|f| f.id == target) {
                    None => report.error(format!(
                        "{tag}: admin_flow references unknown flow '{target}'"
                    )),
                    Some(t) => match &t.open {
                        None => report.error(format!(
                            "{tag}: admin_flow target '{target}' has no open block"
                        )),
                        Some(to) => {
                            if to.join_endpoint.is_some() || to.match_setting_key.is_some() {
                                report.error(format!(
                                    "{tag}: admin_flow target '{target}' must be an admin entry (no participant join fields)"
                                ));
                            }
                            if to.admin_path.as_deref().unwrap_or("").trim().is_empty() {
                                report.error(format!(
                                    "{tag}: admin_flow target '{target}' requires its own admin_path guardrail"
                                ));
                            }
                        }
                    },
                }
            }
        }
        (Some(p), None) => {
            if p.trim().is_empty() {
                report.error(format!("{tag}: admin_path is empty"));
            }
            if all_join {
                report.error(format!(
                    "{tag}: admin_path on a participant entry requires admin_flow (route target)"
                ));
            }
        }
        (None, Some(target)) => {
            report.error(format!(
                "{tag}: admin_flow '{target}' requires admin_path (flag path in the session response)"
            ));
        }
        (None, None) => {}
    }
    if !any_join && o.admin_path.as_deref().unwrap_or("").trim().is_empty() {
        report.error(format!(
            "{tag}: admin entry (no join fields) requires admin_path — the session response must carry an admin flag"
        ));
    }
    // Erlaubte Platzhalter im open-Block: Einstiegs-Param, sid, deklarierte
    // Settings-Keys (username-Platzhalter im join_body).
    let setting_keys: std::collections::BTreeSet<&str> =
        m.settings_fields.iter().map(|f| f.key.as_str()).collect();
    let allowed = |p: &str| p == o.param.as_str() || p == "sid" || setting_keys.contains(p);
    let check_templates: Vec<(&str, Option<&std::collections::BTreeMap<String, String>>)> = vec![
        ("headers", o.headers.as_ref()),
        ("join_headers", o.join_headers.as_ref()),
    ];
    if let Some(tpl) = &o.join_body {
        let mut texts = Vec::new();
        collect_json_strings(tpl, &mut texts);
        let joined = texts.join(" ");
        for p in extract_params(&joined) {
            if !allowed(&p) {
                report.error(format!(
                    "{tag}: join_body placeholder '{{{p}}}' is unbound (allowed: {{{}}}, {{sid}}, settings keys)",
                    o.param
                ));
            }
        }
    }
    for (what, headers) in check_templates {
        if let Some(map) = headers {
            for (name, template) in map {
                if name.trim().is_empty() {
                    report.error(format!("{tag}: {what} has empty header name"));
                }
                for p in extract_params(template) {
                    if !allowed(&p) {
                        report.error(format!(
                            "{tag}: {what} template '{{{p}}}' is unbound (allowed: {{{}}}, {{sid}}, settings keys)",
                            o.param
                        ));
                    }
                }
            }
        }
    }
    for (what, target) in [
        ("open_screen", &o.open_screen),
        ("done_screen", &o.done_screen),
    ] {
        if target.trim().is_empty() {
            report.error(format!("{tag}: {what} is empty"));
        } else if !ids.contains(target.as_str()) {
            report.error(format!(
                "{tag}: {what} references unknown screen '{target}'"
            ));
        }
    }
    // D-A (Resume-Logik): progress-Felder sind optional, aber gekoppelt —
    // progress_screen ohne progress_path wäre eine Zielangabe ohne Auslöser.
    if let Some(pp) = &o.progress_path
        && pp.trim().is_empty()
    {
        report.error(format!("{tag}: progress_path is empty"));
    }
    if let Some(ps) = &o.progress_screen {
        if ps.trim().is_empty() {
            report.error(format!("{tag}: progress_screen is empty"));
        } else if !ids.contains(ps.as_str()) {
            report.error(format!(
                "{tag}: progress_screen references unknown screen '{ps}'"
            ));
        }
        if o.progress_path.as_deref().unwrap_or("").trim().is_empty() {
            report.error(format!(
                "{tag}: progress_screen requires a non-empty progress_path"
            ));
        }
    }
    // D-A (Selbst-Wünsche-Injektion): alle Pfade müssen gefüllt sein —
    // leere Strings wären ein semantisch kaputter Schalter.
    if let Some(sw) = &o.self_wishes {
        for (what, val) in [
            ("self_wishes.wishes_path", &sw.wishes_path),
            ("self_wishes.collection", &sw.collection),
            ("self_wishes.id_path", &sw.id_path),
            ("self_wishes.name_path", &sw.name_path),
        ] {
            if val.trim().is_empty() {
                report.error(format!("{tag}: {what} is empty"));
            }
        }
    }
}

/// Alle Strings in einem JSON-Wert einsammeln (join_body-Platzhalterprüfung).
fn collect_json_strings(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Array(items) => {
            for item in items {
                collect_json_strings(item, out);
            }
        }
        serde_json::Value::Object(map) => {
            for val in map.values() {
                collect_json_strings(val, out);
            }
        }
        _ => {}
    }
}

fn check_screen_id_ref(
    flow_id: &str,
    what: &str,
    target: &str,
    ids: &std::collections::BTreeSet<&str>,
    report: &mut Report,
) {
    if target.trim().is_empty() {
        report.error(format!(
            "flow '{flow_id}': {what} references empty screen id"
        ));
    } else if !ids.contains(target) {
        report.error(format!(
            "flow '{flow_id}': {what} references unknown screen '{target}'"
        ));
    }
}

fn check_condition_refs(
    flow_id: &str,
    ctx: &str,
    c: &Condition,
    ids: &std::collections::BTreeSet<&str>,
    report: &mut Report,
) {
    if c.on.trim().is_empty() {
        report.error(format!(
            "flow '{flow_id}': {ctx} condition has empty 'on' path"
        ));
    }
    if c.op == CondOp::Equals || c.op == CondOp::NotEquals {
        if c.value.is_none() {
            report.error(format!(
                "flow '{flow_id}': {ctx} condition op requires a 'value'"
            ));
        }
    } else if c.value.is_some() {
        report.warn(format!(
            "flow '{flow_id}': {ctx} condition op ignores 'value'"
        ));
    }
    if let Some(t) = &c.then {
        check_screen_id_ref(flow_id, &format!("{ctx} 'then'"), t, ids, report);
    }
    if let Some(t) = &c.otherwise {
        check_screen_id_ref(flow_id, &format!("{ctx} 'otherwise'"), t, ids, report);
    }
}

fn check_screen(
    flow_id: &str,
    s: &Screen,
    ids: &std::collections::BTreeSet<&str>,
    pin_keys: &std::collections::BTreeSet<&str>,
    report: &mut Report,
) {
    let sid = &s.id;
    if sid.trim().is_empty() {
        report.error(format!("flow '{flow_id}': screen with empty id"));
    }
    // Endpunkt-Regeln: form braucht keinen Endpunkt, wait POLLT seinen
    // Endpunkt (raffle_done-Exit), alle anderen brauchen einen.
    let target = s.endpoint.as_deref().unwrap_or("");
    if s.screen_type != ScreenType::Form {
        if target.trim().is_empty() {
            if s.screen_type == ScreenType::Wait {
                report.warn(format!(
                    "flow '{flow_id}': wait screen '{sid}' without endpoint — nothing to poll"
                ));
            } else {
                report.error(format!(
                    "flow '{flow_id}': screen '{sid}' has empty endpoint"
                ));
            }
        } else {
            check_endpoint_target(
                flow_id,
                sid,
                target,
                s.params.as_deref().unwrap_or(&[]),
                report,
            );
        }
    } else if !target.trim().is_empty() {
        report.warn(format!(
            "flow '{flow_id}': screen '{sid}' ignores 'endpoint' (type {:?})",
            s.screen_type
        ));
    }
    // Header-Templates: Platzhalter gegen `params` prüfen.
    let param_keys: std::collections::BTreeSet<&str> =
        s.params.iter().flatten().map(|p| p.key.as_str()).collect();
    for (name, template) in s.headers.iter().flatten() {
        if name.trim().is_empty() {
            report.error(format!(
                "flow '{flow_id}': screen '{sid}' has empty header name"
            ));
        }
        for param in extract_params(template) {
            if !param_keys.contains(param.as_str()) {
                report.error(format!(
                    "flow '{flow_id}': screen '{sid}': header template '{{{param}}}' has no declared param"
                ));
            }
        }
    }
    for p in s.params.iter().flatten() {
        if p.key.trim().is_empty() {
            report.error(format!("flow '{flow_id}': screen '{sid}': param key empty"));
        }
    }
    if s.mapping.is_empty() && s.screen_type != ScreenType::Form {
        report.warn(format!("flow '{flow_id}': screen '{sid}' without mapping"));
    }
    // SCLM v2 (Spec 5.2): Screen-Antwort → Session-Keystore. Spalten sind
    // fest (Teil der Keystore-Tabelle), Werte sind Flow-Zustands-Keys.
    for (col, state_key) in s.keystore_fields.iter().flatten() {
        if !matches!(
            col.as_str(),
            "join_code" | "admin_code" | "name" | "participant_id" | "pcode" | "result_id"
        ) {
            report.error(format!(
                "flow '{flow_id}': screen '{sid}': keystore_fields column '{col}' unknown (allowed: join_code, admin_code, name, participant_id, pcode, result_id)"
            ));
        }
        if state_key.trim().is_empty() {
            report.error(format!(
                "flow '{flow_id}': screen '{sid}': keystore_fields value for '{col}' is empty"
            ));
        }
    }
    check_poll_bounds(
        report,
        &format!("flow '{flow_id}': screen '{sid}'"),
        s.poll_seconds,
    );
    if let Some(c) = &s.when {
        check_condition_refs(flow_id, "display", c, ids, report);
        if c.then.is_some() || c.otherwise.is_some() {
            report.warn(format!(
                "flow '{flow_id}': screen '{sid}': display rule ignores then/otherwise"
            ));
        }
    }
    for (what, target) in [
        ("on_submit", s.on_submit.as_deref()),
        ("on_select", s.on_select.as_deref()),
    ] {
        if let Some(t) = target {
            check_screen_id_ref(flow_id, what, t, ids, report);
        }
    }
    match s.screen_type {
        ScreenType::Form => match &s.form {
            Some(form) => check_form(
                &format!("flow '{flow_id}' screen '{sid}'"),
                form,
                pin_keys,
                report,
            ),
            None => report.error(format!(
                "flow '{flow_id}': screen '{sid}' (type form): missing 'form' declaration"
            )),
        },
        ScreenType::Wait => match &s.exit {
            Some(exit) => {
                if exit.then.as_deref().map(str::trim).unwrap_or("").is_empty() {
                    report.error(format!(
                        "flow '{flow_id}': wait screen '{sid}' exit needs 'then' target"
                    ));
                } else {
                    check_condition_refs(flow_id, "wait exit", exit, ids, report);
                }
                if s.poll_seconds.is_none() {
                    report.warn(format!(
                        "flow '{flow_id}': wait screen '{sid}' without poll_seconds — no polling"
                    ));
                }
            }
            None => report.error(format!(
                "flow '{flow_id}': wait screen '{sid}' missing 'exit' condition"
            )),
        },
        ScreenType::CodeEntry | ScreenType::List | ScreenType::Detail | ScreenType::Results => {}
    }
    for (i, ac) in s.autocomplete.iter().flatten().enumerate() {
        check_autocomplete(flow_id, sid, i, ac, report);
    }
    // SF2: Multi-Select nur auf Listen; Confirm braucht eine Write-Action.
    if let Some(multi) = &s.multi {
        if s.screen_type != ScreenType::List {
            report.error(format!(
                "flow '{flow_id}': screen '{sid}': 'multi' is only valid on type list"
            ));
        }
        let has_write = s.actions.iter().flatten().any(|a| !a.is_navigation());
        if !has_write {
            report.error(format!(
                "flow '{flow_id}': screen '{sid}': multi list needs an action with method (confirm)"
            ));
        }
        if multi
            .max_path
            .as_deref()
            .is_some_and(|mp| mp.trim().is_empty())
        {
            report.error(format!(
                "flow '{flow_id}': screen '{sid}': multi.max_path is empty"
            ));
        }
        // U-BUILD (docs/SCLM-FLOW-V2 §26): select_if ist die generische
        // Waehlbarkeits-Regel pro Zeile (ersetzt das for_sale-Gating) —
        // Bedingung wie enabled_if: then/otherwise ist hier sinnlos (Error).
        // `on` MUSS ein Mapping-Key des Screens sein: die Regel wird gegen
        // {Flow-Zustand, ...Zeile} ausgewertet, und nur gemappte Zeilen-Keys
        // garantieren, dass die Zeile das Feld traegt. select_if ohne
        // Mapping-Key ist eine tote Deklaration (Error).
        if let Some(c) = &multi.select_if {
            check_condition_refs(
                flow_id,
                &format!("screen '{sid}' multi select_if"),
                c,
                ids,
                report,
            );
            if c.then.is_some() || c.otherwise.is_some() {
                report.error(format!(
                    "flow '{flow_id}': screen '{sid}': multi select_if must not use then/otherwise"
                ));
            }
            if !s.mapping.contains_key(c.on.as_str()) {
                report.error(format!(
                    "flow '{flow_id}': screen '{sid}': multi select_if 'on' '{}' is not a mapping key of the screen",
                    c.on
                ));
            }
        }
        // select_hint ohne de/en-Text rendert keinen Hinweis (Error wie bei
        // options-Eintraegen — die Deklaration waere tot).
        if let Some(h) = &multi.select_hint
            && (h.de.trim().is_empty() || h.en.trim().is_empty())
        {
            report.error(format!(
                "flow '{flow_id}': screen '{sid}': multi select_hint without de/en text"
            ));
        }
    }
    // SF2: Actions — eindeutige IDs, method/path-Paarigkeit, goto-Referenzen,
    // Header-Templates gegen die deklarierten Action-Params.
    let mut action_ids = std::collections::BTreeSet::new();
    for a in s.actions.iter().flatten() {
        if !seen_insert(&mut action_ids, a.id.as_str()) {
            report.error(format!(
                "flow '{flow_id}': screen '{sid}': duplicate action id '{}'",
                a.id
            ));
        }
        if a.id.trim().is_empty() {
            report.error(format!(
                "flow '{flow_id}': screen '{sid}': action with empty id"
            ));
        }
        match (
            a.method,
            a.path.as_deref().map(str::trim).filter(|p| !p.is_empty()),
        ) {
            (None, None) if a.goto.is_none() => report.error(format!(
                "flow '{flow_id}': screen '{sid}': action '{}' needs method+path or goto",
                a.id
            )),
            (Some(_), None) => report.error(format!(
                "flow '{flow_id}': screen '{sid}': action '{}' with method needs 'path'",
                a.id
            )),
            (None, Some(_)) if a.goto.is_none() => report.error(format!(
                "flow '{flow_id}': screen '{sid}': action '{}' with path needs 'method'",
                a.id
            )),
            _ => {}
        }
        if let Some(g) = a.goto.as_deref() {
            // goto NACH erfolgreichem Write ist legitim (Transition nach
            // Submit, z. B. wishes → wait) — keine Warnung.
            check_screen_id_ref(flow_id, &format!("action '{}'", a.id), g, ids, report);
        }
        let aparams: std::collections::BTreeSet<&str> =
            a.params.iter().flatten().map(|p| p.key.as_str()).collect();
        for (name, template) in a.headers.iter().flatten() {
            if name.trim().is_empty() {
                report.error(format!(
                    "flow '{flow_id}': screen '{sid}': action '{}' has empty header name",
                    a.id
                ));
            }
            for param in extract_params(template) {
                if !aparams.contains(param.as_str()) {
                    report.error(format!(
                        "flow '{flow_id}': screen '{sid}': action '{}': header template '{{{param}}}' has no declared param",
                        a.id
                    ));
                }
            }
        }
        for p in a.params.iter().flatten() {
            if p.key.trim().is_empty() {
                report.error(format!(
                    "flow '{flow_id}': screen '{sid}': action '{}': param key empty",
                    a.id
                ));
            }
        }
        // SF2-R1 D3: Action-Eingabefelder — nicht-leere, gegen `params`
        // eindeutige Keys (Binding-Kollision wäre unaufgelösbar).
        let mut field_keys = std::collections::BTreeSet::new();
        for f in a.fields.iter().flatten() {
            if f.key.trim().is_empty() {
                report.error(format!(
                    "flow '{flow_id}': screen '{sid}': action '{}': field key empty",
                    a.id
                ));
                continue;
            }
            if aparams.contains(f.key.as_str()) || !field_keys.insert(f.key.as_str()) {
                report.error(format!(
                    "flow '{flow_id}': screen '{sid}': action '{}': duplicate field/param key '{}'",
                    a.id, f.key
                ));
            }
            // Ersteller-Cockpit v1 (Karte F, Spec 15): Action-Felder dürfen
            // Auswahlfelder sein (`type: "select"` + `options`) — z. B. der
            // max_wishes-Select 1–6 auf c_overview. select ohne Optionen ist
            // unrendierbar (Error), Optionen ohne select wären tote Deklaration
            // (Error). options-Einträge müssen beide Sprachen tragen.
            let options = f.options.as_deref().unwrap_or(&[]);
            if f.field_type == Some(FieldType::Select) {
                if options.is_empty() {
                    report.error(format!(
                        "flow '{flow_id}': screen '{sid}': action '{}': select field '{}' has no options",
                        a.id, f.key
                    ));
                }
                for o in options {
                    if o.de.trim().is_empty() || o.en.trim().is_empty() {
                        report.error(format!(
                            "flow '{flow_id}': screen '{sid}': action '{}': select field '{}' has an option without de/en text",
                            a.id, f.key
                        ));
                    }
                }
            } else if !options.is_empty() {
                report.error(format!(
                    "flow '{flow_id}': screen '{sid}': action '{}': field '{}' has options without type 'select'",
                    a.id, f.key
                ));
            }
        }
        // SCLM v2: enabled_if ist Bedingung wie when/exit — Referenzen müssen
        // existieren, then/otherwise ist hier sinnlos (Error, kein Warnpfad:
        // die Regel steuert nur die Button-Aktivierung).
        if let Some(c) = &a.enabled_if {
            check_condition_refs(
                flow_id,
                &format!("action '{}' enabled_if", a.id),
                c,
                ids,
                report,
            );
            if c.then.is_some() || c.otherwise.is_some() {
                report.error(format!(
                    "flow '{flow_id}': screen '{sid}': action '{}': enabled_if must not use then/otherwise",
                    a.id
                ));
            }
        }
    }
}

fn check_endpoint_target(
    flow_id: &str,
    sid: &str,
    target: &str,
    params: &[crate::manifest::Param],
    report: &mut Report,
) {
    if target.contains("file://") {
        report.error(format!(
            "flow '{flow_id}': screen '{sid}': file:// not allowed in endpoint"
        ));
    }
    if target.starts_with("http://") {
        report.error(format!(
            "flow '{flow_id}': screen '{sid}': http:// not allowed (base_url is https)"
        ));
    }
    if target.starts_with("https://") {
        report.warn(format!(
            "flow '{flow_id}': screen '{sid}': absolute endpoint overrides base_url — verify deliberately"
        ));
    }
    let param_keys: std::collections::BTreeSet<&str> =
        params.iter().map(|p| p.key.as_str()).collect();
    for param in extract_params(target) {
        if !param_keys.contains(param.as_str()) {
            report.error(format!(
                "flow '{flow_id}': screen '{sid}': endpoint placeholder '{{{param}}}' is not declared in 'params'"
            ));
        }
    }
}

fn check_autocomplete(
    flow_id: &str,
    sid: &str,
    idx: usize,
    ac: &AutocompleteRule,
    report: &mut Report,
) {
    let ctx = format!("flow '{flow_id}' screen '{sid}' autocomplete[{idx}]");
    if ac.field.trim().is_empty() {
        report.error(format!("{ctx}: empty 'field'"));
    }
    let ep = &ac.endpoint;
    if ep.trim().is_empty() {
        report.error(format!("{ctx}: empty 'endpoint'"));
    } else {
        check_endpoint_target(
            flow_id,
            sid,
            ep,
            ac.params.as_deref().unwrap_or(&[]),
            report,
        );
    }
    for p in ac.params.iter().flatten() {
        if p.key.trim().is_empty() {
            report.error(format!("{ctx}: param key empty"));
        }
    }
    if ac.fill.is_empty() {
        report.warn(format!("{ctx}: no 'fill' mapping — no auto-fill"));
    }
    for (field, path) in &ac.fill {
        if field.trim().is_empty() {
            report.error(format!("{ctx}: empty fill field"));
        } else if path.trim().is_empty() {
            // Leerer Pfad ist DEKLARATIV GÜLTIG (SF3): „Treffer selbst" —
            // Such-Endpoints mit reinen String-Listen (SCLM Teilnehmer-/
            // Waren-Suche) füllen so das Eingabefeld direkt. dot_path/
            // autofill (Rust + TS) resolven "" auf das Item selbst. Bewusst
            // KEINE Warnung: gültiges Idiom.
        }
    }
}

/// Placement-Regeln (Sebo-Korrektur 2026-11-02): `own` braucht ein Label,
/// `replace`/`embed` brauchen ein Ziel. `hotkey_slot` ist nur bei `own`
/// sinnvoll (Warnung sonst). Das Ziel selbst verifiziert C2 gegen die
/// bekannten Sektionen — hier reicht die Strukturprüfung (generischer Core).
fn check_placement(p: &Placement, report: &mut Report) {
    match p.kind {
        PlacementKind::Own => {
            if p.label.is_none() {
                report.error("placement.kind=own requires a 'label' (de/en)");
            }
            if p.target.is_some() {
                report.warn("placement.kind=own ignores 'target'");
            }
        }
        PlacementKind::Replace | PlacementKind::Embed => {
            let target = p.target.as_deref().map(str::trim).unwrap_or("");
            if target.is_empty() {
                report.error(format!(
                    "placement.kind={} requires a non-empty 'target' section",
                    if p.kind == PlacementKind::Replace {
                        "replace"
                    } else {
                        "embed"
                    }
                ));
            }
            if p.label.is_some() {
                report.warn(format!(
                    "placement.kind={} ignores 'label'",
                    if p.kind == PlacementKind::Replace {
                        "replace"
                    } else {
                        "embed"
                    }
                ));
            }
            if p.hotkey_slot {
                report.warn("hotkey_slot only applies to placement.kind=own");
            }
        }
    }
}

fn check_view(v: &View, report: &mut Report) {
    let _ = ViewType::Calendar;
    // Formular-Views adressieren über form.path statt endpoint.
    let target: &str = if v.view_type == ViewType::Form {
        v.form.as_ref().map(|f| f.path.as_str()).unwrap_or("")
    } else {
        v.endpoint.as_str()
    };
    if v.view_type != ViewType::Form && target.trim().is_empty() {
        report.error(format!("view '{}': empty endpoint", v.id));
        return;
    }
    // Endpunkte sind relativ zu base_url; absolute URLs und file:// sind Error.
    if target.contains("file://") {
        report.error(format!("view '{}': file:// not allowed in endpoint", v.id));
    }
    if target.starts_with("http://") {
        report.error(format!(
            "view '{}': http:// not allowed (base_url is https)",
            v.id
        ));
    }
    if target.starts_with("https://") {
        report.warn(format!(
            "view '{}': absolute endpoint overrides base_url — verify deliberately",
            v.id
        ));
    }
    check_poll_bounds(report, &format!("view '{}'", v.id), v.poll_seconds);
    // Endpoint-Platzhalter: jeder `{param}` muss deklariert sein (Error —
    // sonst bricht der Fetch zur Laufzeit).
    let param_keys: std::collections::BTreeSet<&str> =
        v.params.iter().flatten().map(|p| p.key.as_str()).collect();
    if v.view_type != ViewType::Form {
        for param in extract_params(&v.endpoint) {
            if !param_keys.contains(param.as_str()) {
                report.error(format!(
                    "view '{}': endpoint placeholder '{{{param}}}' is not declared in 'params'",
                    v.id
                ));
            }
        }
        for p in v.params.iter().flatten() {
            if p.key.trim().is_empty() {
                report.error(format!("view '{}': param key empty", v.id));
            }
        }
        // Header-Templates: Platzhalter ebenfalls gegen `params` prüfen.
        for (name, template) in v.headers.iter().flatten() {
            if name.trim().is_empty() {
                report.error(format!("view '{}': empty header name", v.id));
            }
            for param in extract_params(template) {
                if !param_keys.contains(param.as_str()) {
                    report.error(format!(
                        "view '{}': header template '{{{param}}}' is not declared in 'params'",
                        v.id
                    ));
                }
            }
        }
    }
    if v.view_type == ViewType::Form {
        match &v.form {
            Some(form) => check_form(&v.id, form, &Default::default(), report),
            None => report.error(format!(
                "view '{}' (type form): missing 'form' declaration",
                v.id
            )),
        }
    } else if v.form.is_some() {
        report.warn(format!(
            "view '{}': 'form' declaration ignored for type != form",
            v.id
        ));
    }

    // Nicht-Form-Views ohne Mapping sind verdächtig (nur Kalender-/Detail-Views
    // brauchen zwingend eins — Warnung genügt generisch).
    if v.view_type != ViewType::Form && v.mapping.is_empty() {
        report.warn(format!(
            "view '{}': type {:?} without mapping",
            v.id, v.view_type
        ));
    }
}

fn check_form(
    view_id: &str,
    form: &crate::manifest::FormView,
    extra_keys: &std::collections::BTreeSet<&str>,
    report: &mut Report,
) {
    if form.fields.is_empty() {
        report.error(format!("view '{view_id}': form has no fields"));
    }
    if form.path.trim().is_empty() {
        report.error(format!("view '{view_id}': form path empty"));
    }
    // {param}-Platzhalter im Pfad müssen Felder sein (Flow-Screens: auch
    // Flow-Zustands-Keys/Pins gelten — Selektionen fließen in den Body).
    let mut allowed: std::collections::BTreeSet<&str> =
        form.fields.iter().map(|f| f.key.as_str()).collect();
    allowed.extend(extra_keys.iter().copied());
    for param in extract_params(&form.path) {
        if !allowed.contains(param.as_str()) {
            report.error(format!(
                "view '{view_id}': path placeholder '{{{param}}}' has no matching field"
            ));
        }
    }
    // body_template: rekurriert nach {param}-Strings scannen (Warnung statt
    // Error — Templates können Strings enthalten, die keine Platzhalter sind)
    if let HttpMethod::Get = form.method {
        report.warn(format!(
            "view '{view_id}': form with GET method — intended for read-only forms"
        ));
    }
    check_template(&form.body_template, view_id, &allowed, report);
    for field in &form.fields {
        check_field(view_id, field, report);
    }
}

fn extract_params(path: &str) -> Vec<String> {
    let mut params = Vec::new();
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        if let Some(end) = after.find('}') {
            params.push(after[..end].to_string());
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    params
}

fn check_field(view_id: &str, f: &FormField, report: &mut Report) {
    if f.key.trim().is_empty() {
        report.error(format!("view '{view_id}': field key empty"));
    }
    let _ = FieldType::Text;
    if f.field_type == FieldType::Select && f.options.as_deref().unwrap_or(&[]).is_empty() {
        report.error(format!(
            "view '{view_id}': select field '{}' has no options",
            f.key
        ));
    }
    if f.header
        .as_deref()
        .map(str::trim)
        .is_some_and(|h| h.is_empty())
    {
        report.error(format!(
            "view '{view_id}': field '{}' declares an empty 'header'",
            f.key
        ));
    }
    if f.default_source
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| s.is_empty())
    {
        report.error(format!(
            "view '{view_id}': field '{}' declares an empty 'default_source'",
            f.key
        ));
    }
    // U-BUILD (docs/SCLM-FLOW-V2 §26): deklarierte Feldgrenzen. max_length
    // nur im Bereich 1..=2000 (0 = tote Deklaration, >2000 = kein Manifest
    // braucht so breite Textfelder); min/max nur als Paar sinnvoll — min > max
    // haelt jede Koerpergroesse. Manifeste ohne diese Felder validieren wie
    // bisher (None = kein Verhalten, Bestandsmanifeste byte-identisch).
    if let Some(ml) = f.max_length
        && !(1..=2000).contains(&ml)
    {
        report.error(format!(
            "view '{view_id}': field '{}': max_length {ml} outside 1..=2000",
            f.key
        ));
    }
    if let (Some(lo), Some(hi)) = (f.min, f.max)
        && lo > hi
    {
        report.error(format!(
            "view '{view_id}': field '{}': min {lo} greater than max {hi}",
            f.key
        ));
    }
}

/// Template-Platzhalter prüfen: Strings im Template dürfen {param} nutzen.
fn check_template(
    v: &Value,
    view_id: &str,
    field_keys: &std::collections::BTreeSet<&str>,
    report: &mut Report,
) {
    match v {
        Value::String(s) => {
            for param in extract_params(s) {
                if !field_keys.contains(param.as_str()) {
                    report.error(format!(
                        "view '{view_id}': template placeholder '{{{param}}}' has no matching field"
                    ));
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                check_template(item, view_id, field_keys, report);
            }
        }
        Value::Object(map) => {
            for (_, val) in map {
                check_template(val, view_id, field_keys, report);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"{
        "manifest_version": 1,
        "id": "sclm",
        "name": {"de": "SCLM", "en": "SCLM"},
        "base_url": "https://sclm.site",
        "auth": {"type": "header", "header": "X-SCLM-Code", "value_source": "connector.sclm.code"},
        "placement": {"kind": "own", "label": {"de": "SCLM", "en": "SCLM"}, "hotkey_slot": true},
        "views": [
            {
                "id": "sessions", "type": "table",
                "title": {"de": "Verlosungen", "en": "Raffles"},
                "endpoint": "/api/sessions",
                "mapping": {"name": "data[].name"},
                "poll_seconds": 300
            },
            {
                "id": "join", "type": "form",
                "title": {"de": "Beitreten", "en": "Join"},
                "endpoint": "",
                "form": {
                    "fields": [
                        {"key": "code", "label": {"de": "Code", "en": "Code"}, "type": "text", "required": true},
                        {"key": "name", "label": {"de": "Name", "en": "Name"}, "type": "text", "required": true}
                    ],
                    "method": "post",
                    "path": "/api/session/{code}/participants",
                    "body_template": {"name": "{name}"}
                }
            }
        ]
    }"#;

    #[test]
    fn valid_manifest_passes() {
        let report = validate_manifest(VALID.as_bytes());
        assert!(report.is_ok(), "{report:?}");
        assert!(report.warnings.is_empty(), "{report:?}");
    }

    #[test]
    fn rejects_http_and_file() {
        let http = VALID.replace("https://sclm.site", "http://sclm.site");
        assert!(!validate_manifest(http.as_bytes()).is_ok());
        let file = VALID.replace("https://sclm.site", "file:///etc");
        assert!(!validate_manifest(file.as_bytes()).is_ok());
    }

    #[test]
    fn rejects_oversize() {
        let junk = vec![b'a'; MAX_MANIFEST_BYTES + 1];
        let r = validate_manifest(&junk);
        assert!(!r.is_ok());
        assert!(r.errors[0].contains("size limit"));
    }

    #[test]
    fn rejects_schema_drift() {
        let bad = r#"{"manifest_version": 1, "id": "x", "typo_field": true}"#;
        let r = validate_manifest(bad.as_bytes());
        assert!(!r.is_ok());
        assert!(r.errors[0].contains("schema validation failed"));
    }

    #[test]
    fn header_auth_requires_header_and_source() {
        // Leere value_source → Error (kein trailing-comma-Artefakt).
        let no_source = VALID.replace("connector.sclm.code", "");
        let r = validate_manifest(no_source.as_bytes());
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.contains("value_source")));
    }

    #[test]
    fn form_view_requires_form_block_and_matching_params() {
        // type=form ohne form-Block: Inline-Manifest statt String-Replace.
        let m = concat!(
            r#"{"manifest_version":1,"id":"x","name":{"de":"x","en":"x"},"#,
            r#""base_url":"https://x.example","placement":{"kind":"own","label":{"de":"x","en":"x"}},"#,
            r#""views":[{"id":"f","type":"form","title":{"de":"f","en":"f"},"endpoint":""}]}"#
        );
        let r = validate_manifest(m.as_bytes());
        assert!(!r.is_ok(), "{r:?}");
        assert!(r.errors.iter().any(|e| e.contains("missing 'form'")));

        let bad_param = VALID.replace(
            "/api/session/{code}/participants",
            "/api/session/{kode}/participants",
        );
        let r = validate_manifest(bad_param.as_bytes());
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.contains("path placeholder")));
    }

    #[test]
    fn warnings_can_be_ignored() {
        // absolute endpoint = Warnung, importierbar mit „Warnung ignorieren“
        let abs = VALID.replace("/api/sessions", "https://cdn.example.com/api/sessions");
        let r = validate_manifest(abs.as_bytes());
        assert!(r.is_ok());
        assert!(r.warnings.iter().any(|w| w.contains("absolute endpoint")));
    }

    #[test]
    fn placement_rules() {
        // own ohne Label = Error
        let no_label = VALID.replace(r#""label": {"de": "SCLM", "en": "SCLM"}, "#, "");
        let r = validate_manifest(no_label.as_bytes());
        assert!(!r.is_ok(), "{r:?}");
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("own requires a 'label'"))
        );

        // replace ohne target = Error; mit label = Warnung
        let replace = VALID.replace(
            r#""placement": {"kind": "own", "label": {"de": "SCLM", "en": "SCLM"}, "hotkey_slot": true}"#,
            r#""placement": {"kind": "replace", "target": "", "label": {"de": "x", "en": "x"}, "hotkey_slot": true}"#,
        );
        let r = validate_manifest(replace.as_bytes());
        assert!(!r.is_ok(), "{r:?}");
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("replace requires a non-empty 'target'"))
        );

        let replace_ok = VALID.replace(
            r#""placement": {"kind": "own", "label": {"de": "SCLM", "en": "SCLM"}, "hotkey_slot": true}"#,
            r#""placement": {"kind": "embed", "target": "inventory"}"#,
        );
        let r = validate_manifest(replace_ok.as_bytes());
        assert!(r.is_ok(), "{r:?}");

        // embed mit hotkey_slot = Warnung (ignorierbar)
        let embed_warn = VALID.replace(
            r#""placement": {"kind": "own", "label": {"de": "SCLM", "en": "SCLM"}, "hotkey_slot": true}"#,
            r#""placement": {"kind": "embed", "target": "inventory", "hotkey_slot": true}"#,
        );
        let r = validate_manifest(embed_warn.as_bytes());
        assert!(r.is_ok(), "{r:?}");
        assert!(r.warnings.iter().any(|w| w.contains("hotkey_slot")));

        // placement fehlt komplett = Schema-Error (Pflichtfeld)
        let missing = VALID.replace(r#""placement": {"kind": "own", "label": {"de": "SCLM", "en": "SCLM"}, "hotkey_slot": true},"#, "");
        assert!(!validate_manifest(missing.as_bytes()).is_ok());
    }

    #[test]
    fn fixture_sclm_manifest_validates() {
        // Das kanonische Manifest-Test-Asset src-tauri/tests/fixtures/sclm-manifest.json
        // (eingecheckter Snapshot von Omars lokalem examples/sclm.json) muss sauber
        // durch den Validator laufen — genau so, wie ein Nutzer es importieren würde.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../src-tauri/tests/fixtures/sclm-manifest.json");
        let text = std::fs::read_to_string(&path).expect("fixtures/sclm-manifest.json exists");
        let r = validate_manifest(text.as_bytes());
        assert!(r.is_ok(), "{r:?}");
        assert!(r.warnings.is_empty(), "{r:?}");
    }

    #[test]
    fn duplicate_view_ids_error() {
        let dup = VALID.replace(r#""id": "join""#, r#""id": "sessions""#);
        let r = validate_manifest(dup.as_bytes());
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.contains("duplicate view id")));
    }

    #[test]
    fn endpoint_placeholder_requires_declared_param() {
        // Detail-View mit {code} aber ohne params → Error.
        let base = VALID.replace(
            r#""endpoint": "/api/sessions""#,
            r#""endpoint": "/api/session/{code}", "headers": {"X-SCLM-Code": "{code}"}"#,
        );
        let r = validate_manifest(base.as_bytes());
        assert!(!r.is_ok(), "{r:?}");
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("endpoint placeholder") && e.contains("{code}")),
            "{r:?}"
        );

        // Mit deklariertem param ist beides sauber.
        let declared = base.replace(
            r#""mapping": {"name": "data[].name"},"#,
            r#""mapping": {"name": "data[].name"}, "params": [{"key": "code", "label": {"de": "Code", "en": "Code"}}],"#,
        );
        let r = validate_manifest(declared.as_bytes());
        assert!(r.is_ok(), "{r:?}");

        // Header-Template mit undeklariertem Platzhalter → Error.
        let bad_header = declared.replace(
            r#""headers": {"X-SCLM-Code": "{code}"}"#,
            r#""headers": {"X-SCLM-Code": "{kode}"}"#,
        );
        let r = validate_manifest(bad_header.as_bytes());
        assert!(
            r.errors.iter().any(|e| e.contains("header template")),
            "{r:?}"
        );
    }

    // ---- SF1: Flow-Validierung ------------------------------------------------

    /// Vollständiges, selbstständiges Flow-Manifest (kein VALID-Replace —
    /// die alten View-Objekte würden sonst als invalid Flow-Objekte in der
    /// flows-Liste landen und den Schema-Check killen, bevor die Flow-Regeln
    /// überhaupt greifen).
    fn flow_json(entry: &str, screens: &str) -> String {
        format!(
            r#"{{"id": "participant", "title": {{"de": "T", "en": "T"}}, "entry": "{entry}", "screens": [{screens}]}}"#
        )
    }

    fn manifest_with_flow(flow: &str) -> String {
        format!(
            r#"{{"manifest_version": 1, "id": "sclm", "name": {{"de": "SCLM", "en": "SCLM"}}, "base_url": "https://sclm.site", "placement": {{"kind": "own", "label": {{"de": "SCLM", "en": "SCLM"}}, "hotkey_slot": true}}, "views": [], "flows": [{flow}]}}"#
        )
    }

    #[test]
    fn flow_screen_unknown_transition_target_errors() {
        // on_submit zeigt auf nicht existierenden Screen → Error
        let screens = r#"{"id": "a", "type": "code_entry", "title": {"de": "a", "en": "a"}, "endpoint": "/api/session/{code}", "params": [{"key": "code", "label": {"de": "c", "en": "c"}}], "on_submit": "ghost"}"#;
        let m = manifest_with_flow(&flow_json("a", screens));
        let r = validate_manifest(m.as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("on_submit") && e.contains("ghost")),
            "{r:?}"
        );
    }

    #[test]
    fn flow_entry_must_exist() {
        let screens = r#"{"id": "a", "type": "code_entry", "title": {"de": "a", "en": "a"}, "endpoint": "/api/session/{code}", "params": [{"key": "code", "label": {"de": "c", "en": "c"}}]}"#;
        let m = manifest_with_flow(&flow_json("nope", screens));
        let r = validate_manifest(m.as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("entry") && e.contains("not a screen")),
            "{r:?}"
        );
    }

    #[test]
    fn wait_screen_requires_exit_condition() {
        let screens = r#"{"id": "w", "type": "wait", "title": {"de": "w", "en": "w"}}"#;
        let m = manifest_with_flow(&flow_json("w", screens));
        let r = validate_manifest(m.as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("wait screen") && e.contains("missing 'exit'")),
            "{r:?}"
        );
    }

    #[test]
    fn wait_screen_exit_resolves_to_known_screen() {
        // exit.then zeigt auf unbekannten Screen → Error; korrekt → sauber
        let bad = r#"{"id": "w", "type": "wait", "title": {"de": "w", "en": "w"}, "poll_seconds": 10, "exit": {"on": "raffle_done", "op": "equals", "value": true, "then": "ghost"}}"#;
        let r = validate_manifest(manifest_with_flow(&flow_json("w", bad)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("exit 'then'") && e.contains("ghost")),
            "{r:?}"
        );

        let ok = r#"{"id": "w", "type": "wait", "title": {"de": "w", "en": "w"}, "endpoint": "/api/wait", "poll_seconds": 10, "exit": {"on": "raffle_done", "op": "equals", "value": true, "then": "res"}, "mapping": {"raffle_done": "raffle_done"}},
            {"id": "res", "type": "results", "title": {"de": "r", "en": "r"}, "endpoint": "/api/res", "mapping": {"n": "n"}}"#;
        let r = validate_manifest(manifest_with_flow(&flow_json("w", ok)).as_bytes());
        assert!(r.is_ok(), "{r:?}");
        assert!(r.warnings.is_empty(), "{r:?}");
    }

    #[test]
    fn flow_form_screen_needs_form_block() {
        let screens = r#"{"id": "f", "type": "form", "title": {"de": "f", "en": "f"}}"#;
        let m = manifest_with_flow(&flow_json("f", screens));
        let r = validate_manifest(m.as_bytes());
        assert!(
            r.errors.iter().any(|e| e.contains("missing 'form'")),
            "{r:?}"
        );
    }

    #[test]
    fn flow_endpoint_rules_like_views() {
        // http:// → Error; {param} ohne Deklaration → Error
        let bad = r#"{"id": "a", "type": "list", "title": {"de": "a", "en": "a"}, "endpoint": "http://evil"}"#;
        let r = validate_manifest(manifest_with_flow(&flow_json("a", bad)).as_bytes());
        assert!(
            r.errors.iter().any(|e| e.contains("http:// not allowed")),
            "{r:?}"
        );

        let badparam = r#"{"id": "a", "type": "list", "title": {"de": "a", "en": "a"}, "endpoint": "/api/session/{code}"}"#;
        let r = validate_manifest(manifest_with_flow(&flow_json("a", badparam)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("endpoint placeholder") && e.contains("{code}")),
            "{r:?}"
        );
    }

    #[test]
    fn flow_duplicate_ids_error() {
        let screens = r#"{"id": "a", "type": "list", "title": {"de": "a", "en": "a"}, "endpoint": "/api/a", "mapping": {"n": "n"}},
            {"id": "a", "type": "list", "title": {"de": "a", "en": "a"}, "endpoint": "/api/a", "mapping": {"n": "n"}}"#;
        let m = manifest_with_flow(&flow_json("a", screens));
        let r = validate_manifest(m.as_bytes());
        assert!(
            r.errors.iter().any(|e| e.contains("duplicate screen id")),
            "{r:?}"
        );

        let dup = manifest_with_flow(&format!(
            "{}, {}",
            flow_json(
                "a",
                r#"{"id": "a", "type": "list", "title": {"de": "a", "en": "a"}, "endpoint": "/api/a", "mapping": {"n": "n"}}"#
            ),
            flow_json(
                "a",
                r#"{"id": "a2", "type": "list", "title": {"de": "a", "en": "a"}, "endpoint": "/api/a", "mapping": {"n": "n"}}"#
            )
        ));
        // Achtung: manifest_with_flow ergänzt nur EIN Flow-Objekt; hier
        // simulieren wir zwei Flows mit identischer ID durch String-Ersetzen.
        let dup = dup.replace(
            r#""flows": [{"#,
            r#""flows": [{"id": "x-flow", "title": {"de": "F", "en": "F"}, "entry": "a", "screens": [{"id": "a", "type": "list", "title": {"de": "a", "en": "a"}, "endpoint": "/api/a", "mapping": {"n": "n"}}]}, {"#,
        );
        let r = validate_manifest(dup.as_bytes());
        assert!(
            r.errors.iter().any(|e| e.contains("duplicate flow id")),
            "{r:?}"
        );
    }

    // ---- U-BUILD (docs/SCLM-FLOW-V2 §26): select_if-Gating + Feldgrenzen ----

    /// list-Screen mit multi + Write-Action (select_if-Unterlage).
    const SEL_BASE: &str = r#"{"id": "w", "type": "list", "title": {"de": "w", "en": "w"}, "endpoint": "/api/w", "mapping": {"n": "n", "for_sale": "for_sale"}, "multi": {"max_path": "max_wishes"}, "actions": [{"id": "confirm", "title": {"de": "c", "en": "c"}, "method": "post", "path": "/api/w", "goto": "done"}]},
            {"id": "done", "type": "results", "title": {"de": "r", "en": "r"}, "endpoint": "/api/r", "mapping": {"n": "n"}}"#;

    #[test]
    fn flow_select_if_checked() {
        // select_if + select_hint auf einem Mapping-Key: valide.
        let ok = SEL_BASE.replace(
            r#""multi": {"max_path": "max_wishes"}"#,
            r#""multi": {"max_path": "max_wishes", "select_if": {"on": "for_sale", "op": "not_equals", "value": true}, "select_hint": {"de": "im Verkauf", "en": "for sale"}}"#,
        );
        let r = validate_manifest(manifest_with_flow(&flow_json("w", &ok)).as_bytes());
        assert!(r.is_ok(), "{r:?}");

        // select_if aendert die Action-Pflicht nicht (bestehende Regel greift).

        // select_if OHNE Mapping-Key des Screens -> Error (tote Deklaration:
        // die Zeile traegt das Feld nie).
        let bad_key = ok.replace(r#""on": "for_sale""#, r#""on": "ghost_field""#);
        let r = validate_manifest(manifest_with_flow(&flow_json("w", &bad_key)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("select_if 'on' 'ghost_field' is not a mapping key")),
            "{r:?}"
        );

        // then/otherwise ist wie bei enabled_if sinnlos (keine Branch-Regel) -> Error.
        let bad_then = ok.replace(r#""value": true}"#, r#""value": true, "then": "done"}"#);
        let r = validate_manifest(manifest_with_flow(&flow_json("w", &bad_then)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("select_if must not use")),
            "{r:?}"
        );

        // equals/not_equals ohne value -> Error (gleiche Condition-Regel wie enabled_if).
        let bad_value = ok.replace(
            r#""op": "not_equals", "value": true"#,
            r#""op": "not_equals""#,
        );
        let r = validate_manifest(manifest_with_flow(&flow_json("w", &bad_value)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("condition op requires a 'value'")),
            "{r:?}"
        );

        // select_hint ohne de/en-Text -> Error (Hinweis waere unrendbar).
        let bad_hint = ok.replace(
            r#""select_hint": {"de": "im Verkauf", "en": "for sale"}"#,
            r#""select_hint": {"de": " ", "en": "for sale"}"#,
        );
        let r = validate_manifest(manifest_with_flow(&flow_json("w", &bad_hint)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("select_hint without de/en")),
            "{r:?}"
        );

        // exists-Op ohne value ist ok (Op-Satz wie enabled_if).
        let exists_ok = ok.replace(r#""op": "not_equals", "value": true"#, r#""op": "exists""#);
        let r = validate_manifest(manifest_with_flow(&flow_json("w", &exists_ok)).as_bytes());
        assert!(r.is_ok(), "{r:?}");
    }

    #[test]
    fn field_bounds_checked() {
        // Flow-Form-Screen (check_form/check_field gilt fuer Views UND Flows).
        let base = r#"{"id": "add", "type": "form", "title": {"de": "a", "en": "a"}, "form": {"fields": [{"key": "name", "label": {"de": "n", "en": "n"}, "type": "text", "required": true}, {"key": "qty", "label": {"de": "q", "en": "q"}, "type": "number"}], "method": "post", "path": "/api/add", "body_template": {"name": "{name}", "qty": "{qty}"}}}"#;

        // Gueltige Grenzen (Fixture-Aequivalent name 120 / qty 1..10000) -> durch.
        let ok = base
            .replace(
                r#""type": "text", "required": true"#,
                r#""type": "text", "required": true, "max_length": 120"#,
            )
            .replace(
                r#""type": "number""#,
                r#""type": "number", "min": 1, "max": 10000"#,
            );
        let r = validate_manifest(manifest_with_flow(&flow_json("add", &ok)).as_bytes());
        assert!(r.is_ok(), "{r:?}");

        // Obergrenze 2000 ist gueltig.
        let max_ok = base.replace(
            r#""type": "text", "required": true"#,
            r#""type": "text", "required": true, "max_length": 2000"#,
        );
        let r = validate_manifest(manifest_with_flow(&flow_json("add", &max_ok)).as_bytes());
        assert!(r.is_ok(), "{r:?}");

        // max_length 0 -> Error (tote Deklaration).
        let zero = base.replace(
            r#""type": "text", "required": true"#,
            r#""type": "text", "required": true, "max_length": 0"#,
        );
        let r = validate_manifest(manifest_with_flow(&flow_json("add", &zero)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("max_length 0 outside 1..=2000")),
            "{r:?}"
        );

        // max_length 5000 -> Error (uebererlaubter Bereich).
        let huge = base.replace(
            r#""type": "text", "required": true"#,
            r#""type": "text", "required": true, "max_length": 5000"#,
        );
        let r = validate_manifest(manifest_with_flow(&flow_json("add", &huge)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("max_length 5000 outside 1..=2000")),
            "{r:?}"
        );

        // min > max -> Error.
        let swapped = base.replace(
            r#""type": "number""#,
            r#""type": "number", "min": 10000, "max": 1"#,
        );
        let r = validate_manifest(manifest_with_flow(&flow_json("add", &swapped)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("min 10000 greater than max 1")),
            "{r:?}"
        );

        // Manifeste OHNE die neuen Felder validieren wie bisher (base selbst).
        let plain = validate_manifest(manifest_with_flow(&flow_json("add", base)).as_bytes());
        assert!(plain.is_ok(), "{plain:?}");
    }

    #[test]
    fn flow_autocomplete_rules_checked() {
        // Auto-Fill-Mapping: endpoint + fill-Mapping, {param}-Prüfung
        let ok = r#"{"id": "wishes", "type": "form", "title": {"de": "w", "en": "w"}, "form": {"fields": [{"key": "sid", "label": {"de": "s", "en": "s"}, "type": "text", "required": true}, {"key": "name", "label": {"de": "n", "en": "n"}, "type": "text"}, {"key": "size", "label": {"de": "s", "en": "s"}, "type": "text"}], "method": "post", "path": "/api/session/{sid}/wishes", "body_template": {"name": "{name}"}}, "autocomplete": [{"field": "name", "endpoint": "/api/items/search", "query_param": "q", "fill": {"size": "size"}}]}"#;
        let r = validate_manifest(manifest_with_flow(&flow_json("wishes", ok)).as_bytes());
        assert!(r.is_ok(), "{r:?}");

        let bad = ok.replace("/api/items/search", "/api/items/{q}/search");
        let r = validate_manifest(manifest_with_flow(&flow_json("wishes", &bad)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("{q}") && e.contains("endpoint placeholder")),
            "{r:?}"
        );

        // leeres fill-Mapping = Warnung (kein Auto-Fill)
        let empty_fill = ok.replace(r#""fill": {"size": "size"}"#, r#""fill": {}"#);
        let r = validate_manifest(manifest_with_flow(&flow_json("wishes", &empty_fill)).as_bytes());
        assert!(
            r.warnings.iter().any(|w| w.contains("no 'fill' mapping")),
            "{r:?}"
        );
    }

    #[test]
    fn flow_multi_and_actions_checked() {
        // multi auf Nicht-Liste → Error
        let bad = r#"{"id": "d", "type": "detail", "title": {"de": "d", "en": "d"}, "endpoint": "/api/d", "mapping": {"n": "n"}, "multi": {}}"#;
        let r = validate_manifest(manifest_with_flow(&flow_json("d", bad)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("'multi' is only valid on type list")),
            "{r:?}"
        );

        // multi Liste ohne Write-Action → Error; mit Action + goto ok
        let list_no_action = r#"{"id": "w", "type": "list", "title": {"de": "w", "en": "w"}, "endpoint": "/api/w", "mapping": {"n": "n"}, "multi": {"max_path": "max_wishes"}}"#;
        let r = validate_manifest(manifest_with_flow(&flow_json("w", list_no_action)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("multi list needs an action with method")),
            "{r:?}"
        );

        let ok = r#"{"id": "w", "type": "list", "title": {"de": "w", "en": "w"}, "endpoint": "/api/w", "mapping": {"n": "n"}, "multi": {"max_path": "max_wishes"}, "actions": [{"id": "confirm", "title": {"de": "c", "en": "c"}, "method": "post", "path": "/api/w", "goto": "done"}, {"id": "nav", "title": {"de": "n", "en": "n"}, "goto": "done"}]},
            {"id": "done", "type": "results", "title": {"de": "r", "en": "r"}, "endpoint": "/api/r", "mapping": {"n": "n"}}"#;
        let r = validate_manifest(manifest_with_flow(&flow_json("w", ok)).as_bytes());
        assert!(r.is_ok(), "{r:?}");

        // SF2-R1 D3: Action-Felder — valide pcode-Claim-Deklaration ok,
        // Kollision Feld/Param → Error, leerer Key → Error.
        let fields_ok = ok.replace(
            r#""method": "post", "path": "/api/w", "goto": "done""#,
            r#""method": "post", "path": "/api/w", "fields": [{"key": "pcode", "label": {"de": "p", "en": "p"}}], "goto": "done""#,
        );
        let r = validate_manifest(manifest_with_flow(&flow_json("w", &fields_ok)).as_bytes());
        assert!(r.is_ok(), "{r:?}");

        let fields_dup = ok.replace(
            r#""method": "post", "path": "/api/w", "goto": "done""#,
            r#""method": "post", "path": "/api/w", "fields": [{"key": "pcode", "label": {"de": "p", "en": "p"}}, {"key": "pcode", "label": {"de": "q", "en": "q"}}], "goto": "done""#,
        );
        let r = validate_manifest(manifest_with_flow(&flow_json("w", &fields_dup)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("duplicate field/param key")),
            "{r:?}"
        );

        // goto auf unbekannten Screen → Error
        let bad_goto = ok.replace(r#""goto": "done""#, r#""goto": "ghost""#);
        let r = validate_manifest(manifest_with_flow(&flow_json("w", &bad_goto)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("action 'confirm'") && e.contains("ghost")),
            "{r:?}"
        );

        // method ohne path → Error
        let no_path = ok.replace(
            r#""method": "post", "path": "/api/w""#,
            r#""method": "post""#,
        );
        let r = validate_manifest(manifest_with_flow(&flow_json("w", &no_path)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("action 'confirm' with method needs 'path'")),
            "{r:?}"
        );

        // Header-Template ohne deklarierten Param → Error
        let bad_header = ok.replace(
            r#""method": "post", "path": "/api/w""#,
            r#""method": "post", "path": "/api/w", "headers": {"X-SCLM-Code": "{code}"}"#,
        );
        let r = validate_manifest(manifest_with_flow(&flow_json("w", &bad_header)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("action 'confirm'") && e.contains("{code}")),
            "{r:?}"
        );

        // doppelte Action-ID → Error
        let dup = ok.replace(
            r#"{"id": "nav""#,
            r#"{"id": "confirm", "title": {"de": "x", "en": "x"}, "goto": "done"}, {"id": "nav""#,
        );
        let r = validate_manifest(manifest_with_flow(&flow_json("w", &dup)).as_bytes());
        assert!(
            r.errors.iter().any(|e| e.contains("duplicate action id")),
            "{r:?}"
        );
    }

    #[test]
    fn flow_action_select_fields_checked() {
        // Ersteller-Cockpit v1 (Karte F, Spec 15): c_overview-Vertrag — ein
        // Action-Feld als Select (type select + options 1–6) ist valide und
        // wirft keine Warnung (Karte G rendert daraus das Sofort-Auswahlfeld).
        let select_ok = r#"{"id": "co", "type": "detail", "title": {"de": "co", "en": "co"}, "endpoint": "/api/session/{code}", "params": [{"key": "code", "label": {"de": "c", "en": "c"}}, {"key": "sid", "label": {"de": "s", "en": "s"}}], "mapping": {"sid": "id", "max_wishes": "max_wishes"}, "actions": [{"id": "max_wishes", "title": {"de": "mw", "en": "mw"}, "method": "patch", "path": "/api/session/{sid}/settings", "params": [{"key": "code", "label": {"de": "c", "en": "c"}}, {"key": "sid", "label": {"de": "s", "en": "s"}}], "headers": {"X-SCLM-Code": "{code}"}, "body_template": {"max_wishes": "{max_wishes}"}, "fields": [{"key": "max_wishes", "label": {"de": "Max", "en": "Max"}, "type": "select", "required": true, "options": [{"de": "1", "en": "1"}, {"de": "6", "en": "6"}]}]}]}"#;
        let r = validate_manifest(manifest_with_flow(&flow_json("co", select_ok)).as_bytes());
        assert!(r.is_ok(), "{r:?}");
        assert!(r.warnings.is_empty(), "{r:?}");
        let m = connector_manifest_of(&manifest_with_flow(&flow_json("co", select_ok)));
        let action = &m.flows.as_ref().unwrap()[0].screens[0]
            .actions
            .as_ref()
            .unwrap()[0];
        let field = &action.fields.as_ref().unwrap()[0];
        assert_eq!(field.field_type, Some(FieldType::Select));
        assert_eq!(field.options.as_ref().unwrap().len(), 2);
        assert_eq!(field.options.as_ref().unwrap()[1].de, "6");

        // select ohne options → Error (nichts rendert das Feld).
        let select_no_options = select_ok.replace(
            r#", "options": [{"de": "1", "en": "1"}, {"de": "6", "en": "6"}]"#,
            "",
        );
        let r =
            validate_manifest(manifest_with_flow(&flow_json("co", &select_no_options)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("select field 'max_wishes' has no options")),
            "{r:?}"
        );

        // options ohne select-Typ (explizit text) → Error (tote Deklaration).
        let text_with_options = select_ok.replace(r#""type": "select""#, r#""type": "text""#);
        let r =
            validate_manifest(manifest_with_flow(&flow_json("co", &text_with_options)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("field 'max_wishes' has options without type 'select'")),
            "{r:?}"
        );

        // options ohne deklarierten type (Default text) → ebenfalls Error.
        let implicit_text_with_options = select_ok.replace(r#""type": "select", "#, "");
        let r = validate_manifest(
            manifest_with_flow(&flow_json("co", &implicit_text_with_options)).as_bytes(),
        );
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("field 'max_wishes' has options without type 'select'")),
            "{r:?}"
        );

        // Option ohne en-Text → Error (beide Sprachen sind Pflicht).
        let half_option =
            select_ok.replace(r#"{"de": "6", "en": "6"}"#, r#"{"de": "6", "en": ""}"#);
        let r = validate_manifest(manifest_with_flow(&flow_json("co", &half_option)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("select field 'max_wishes' has an option without de/en text")),
            "{r:?}"
        );

        // number-Feld ohne options ist legitim (Delta-Felder wie loot qty).
        let number_ok = select_ok.replace(r#""type": "select""#, r#""type": "number""#);
        let number_ok = number_ok.replace(
            r#", "options": [{"de": "1", "en": "1"}, {"de": "6", "en": "6"}]"#,
            "",
        );
        let r = validate_manifest(manifest_with_flow(&flow_json("co", &number_ok)).as_bytes());
        assert!(r.is_ok(), "{r:?}");
    }

    #[test]
    fn fixture_sclm_manifest_creator_overview_max_wishes_select() {
        // Review-R1 blocking-Punkt: Spec 15 (Zeile „max_wishes-Select 1–6
        // (PATCH settings, sofort)“) + Karte G Kriterium 4 verlangen die
        // Aktion AUF c_overview (nicht nur die Settings-Form dahinter).
        // Kanonisches Test-Asset: eingechecktes Fixture (Snapshot von Omars
        // gitignoredem examples/sclm.json), damit das Gate in Frisch-Klonen
        // ohne examples/sclm.json grün ist.
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../src-tauri/tests/fixtures/sclm-manifest.json"
        );
        let text = std::fs::read_to_string(path).expect("fixtures/sclm-manifest.json ist lesbar");
        let report = validate_manifest(text.as_bytes());
        assert!(report.errors.is_empty(), "{report:?}");
        let m = crate::manifest::ConnectorManifest::parse(text.as_bytes()).expect("parst");
        let creator = m
            .flows
            .as_ref()
            .unwrap()
            .iter()
            .find(|f| f.id == "creator")
            .expect("creator-Flow");
        let overview = creator
            .screens
            .iter()
            .find(|s| s.id == "c_overview")
            .expect("c_overview-Screen");
        let action = overview
            .actions
            .as_ref()
            .unwrap()
            .iter()
            .find(|a| a.id == "max_wishes")
            .expect("max_wishes-Aktion auf c_overview (Sofort-Select, nicht goto)");
        assert_eq!(action.method, Some(HttpMethod::Patch));
        assert_eq!(
            action.path.as_deref(),
            Some("/api/session/{sid}/settings"),
            "PATCH settings ist der bestehende Settings-Pfad"
        );
        assert_eq!(
            action
                .headers
                .as_ref()
                .and_then(|h| h.get("X-SCLM-Code"))
                .map(String::as_str),
            Some("{code}"),
            "Admin-Code als Header-Template (Creator-Flow pinnt admin_code)"
        );
        assert_eq!(
            action.body_template.as_ref(),
            Some(&Value::Object(
                [(
                    "max_wishes".to_string(),
                    Value::String("{max_wishes}".to_string())
                )]
                .into_iter()
                .collect()
            )),
            "Body nur das geänderte Feld (teilbarer PATCH)"
        );
        let params: Vec<&str> = action
            .params
            .as_ref()
            .unwrap()
            .iter()
            .map(|p| p.key.as_str())
            .collect();
        assert_eq!(params, vec!["code", "sid"]);
        let field = &action.fields.as_ref().expect("Select-Feld deklariert")[0];
        assert_eq!(field.key, "max_wishes");
        assert_eq!(field.field_type, Some(FieldType::Select));
        assert_eq!(field.required, Some(true));
        let options: Vec<&str> = field
            .options
            .as_ref()
            .unwrap()
            .iter()
            .map(|o| o.de.as_str())
            .collect();
        assert_eq!(
            options,
            vec!["1", "2", "3", "4", "5", "6"],
            "Overlay-Grenze 1–6 (API-Limit 1–10 bleibt möglich, Spec 15 Punkt 3)"
        );
        // Fallback-Navigation bleibt erhalten (Spec: Select SOFORT auf der
        // Übersicht, die Settings-Form dahinter darf weiter existieren).
        assert!(
            overview
                .actions
                .as_ref()
                .unwrap()
                .iter()
                .any(|a| a.goto.as_deref() == Some("settings")),
            "goto settings bleibt als Fallback"
        );
        // Der Select ist das ERSTE Feld der Übersicht (Tab-Primat).
        assert_eq!(
            overview.actions.as_ref().unwrap()[0].id,
            "max_wishes",
            "Sofort-Select steht vor goto/Regenerate"
        );
    }

    // ---- SCLM v2 Karte A: settings_fields --------------------------------------

    /// Manifest mit settings_fields (an VALID angehängt, vor `sync`-lose Klammer).
    fn with_settings_fields(base: &str, fields: &str) -> String {
        base.replace(
            r#""views": ["#,
            &format!(r#""settings_fields": [{fields}], "views": ["#),
        )
    }

    #[test]
    fn manifest_without_settings_fields_stays_valid() {
        // Rückwärtskompatibilität: bestehende Manifeste haben keine Felder.
        let r = validate_manifest(VALID.as_bytes());
        assert!(r.is_ok(), "{r:?}");
        assert!(r.warnings.is_empty(), "{r:?}");
        let m = connector_manifest_of(VALID);
        assert!(m.settings_fields.is_empty());
    }

    fn connector_manifest_of(text: &str) -> crate::manifest::ConnectorManifest {
        crate::manifest::ConnectorManifest::parse(text.as_bytes()).expect("valid")
    }

    #[test]
    fn settings_fields_valid_and_persisted_shape() {
        let m = with_settings_fields(
            VALID,
            r#"{"key": "sclm_username", "label": {"de": "SCLM-Benutzername", "en": "SCLM username"}, "type": "text", "required": false, "secret": false}"#,
        );
        let r = validate_manifest(m.as_bytes());
        assert!(r.is_ok(), "{r:?}");
        assert!(r.warnings.is_empty(), "{r:?}");
        let parsed = connector_manifest_of(&m);
        assert_eq!(parsed.settings_fields.len(), 1);
        let f = &parsed.settings_fields[0];
        assert_eq!(f.key, "sclm_username");
        assert_eq!(f.label.de, "SCLM-Benutzername");
        assert!(!f.required);
        assert!(!f.secret);
        // Defaults: required/secret fehlen → false.
        let d = with_settings_fields(
            VALID,
            r#"{"key": "k", "label": {"de": "K", "en": "K"}, "type": "text"}"#,
        );
        assert!(
            validate_manifest(d.as_bytes()).is_ok(),
            "{:?}",
            validate_manifest(d.as_bytes())
        );
        assert!(!connector_manifest_of(&d).settings_fields[0].required);
    }

    #[test]
    fn settings_fields_rejects_missing_or_empty_label() {
        // fehlendes label-Objekt → Schema-Error
        let no_label = with_settings_fields(VALID, r#"{"key": "sclm_username", "type": "text"}"#);
        let r = validate_manifest(no_label.as_bytes());
        assert!(!r.is_ok(), "{r:?}");
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("schema validation failed") && e.contains("label")),
            "{r:?}"
        );

        // fehlendes en (nur de) → Schema-Error
        let no_en = with_settings_fields(
            VALID,
            r#"{"key": "sclm_username", "label": {"de": "Name"}, "type": "text"}"#,
        );
        let r = validate_manifest(no_en.as_bytes());
        assert!(!r.is_ok(), "{r:?}");

        // label present, aber leerer String → struktureller Error
        let empty_en = with_settings_fields(
            VALID,
            r#"{"key": "sclm_username", "label": {"de": "Name", "en": "  "}, "type": "text"}"#,
        );
        let r = validate_manifest(empty_en.as_bytes());
        assert!(
            r.errors.iter().any(|e| e.contains("label.en is empty")),
            "{r:?}"
        );
    }

    #[test]
    fn settings_fields_rejects_bad_key_and_unknown_type() {
        // Key mit Leerzeichen/Umlaut → Error
        let bad_key = with_settings_fields(
            VALID,
            r#"{"key": "sclm nutzer", "label": {"de": "N", "en": "N"}, "type": "text"}"#,
        );
        let r = validate_manifest(bad_key.as_bytes());
        assert!(
            r.errors.iter().any(|e| e.contains("must be ASCII")),
            "{r:?}"
        );

        // doppelter Key → Error
        let dup = with_settings_fields(
            VALID,
            r#"{"key": "k", "label": {"de": "N", "en": "N"}, "type": "text"}, {"key": "k", "label": {"de": "M", "en": "M"}, "type": "text"}"#,
        );
        let r = validate_manifest(dup.as_bytes());
        assert!(
            r.errors.iter().any(|e| e.contains("duplicate key")),
            "{r:?}"
        );

        // Kollision mit auth.value_source → Error (VALID-Auth-Quelle)
        let clash = with_settings_fields(
            VALID,
            r#"{"key": "connector.sclm.code", "label": {"de": "N", "en": "N"}, "type": "text"}"#,
        );
        let r = validate_manifest(clash.as_bytes());
        assert!(
            r.errors.iter().any(|e| e.contains("collides with auth")),
            "{r:?}"
        );

        // type != text (v1 nur text) → Schema-Error
        let number = with_settings_fields(
            VALID,
            r#"{"key": "k", "label": {"de": "N", "en": "N"}, "type": "number"}"#,
        );
        assert!(!validate_manifest(number.as_bytes()).is_ok());

        // unbekanntes Feld im settings_field → Schema-Error (deny_unknown_fields)
        let typo = with_settings_fields(
            VALID,
            r#"{"key": "k", "label": {"de": "N", "en": "N"}, "type": "text", "typo": true}"#,
        );
        assert!(!validate_manifest(typo.as_bytes()).is_ok());
    }

    #[test]
    fn fixture_sclm_manifest_declares_settings_fields() {
        // Das Fixture deklariert den dedizierten SCLM-Benutzernamen
        // (leer startend, KEIN uex-Pre-fill — keine default_source-Whitelist).
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../src-tauri/tests/fixtures/sclm-manifest.json");
        let text = std::fs::read_to_string(&path).expect("fixtures/sclm-manifest.json exists");
        let m = connector_manifest_of(&text);
        assert_eq!(
            m.settings_fields
                .iter()
                .map(|f| f.key.as_str())
                .collect::<Vec<_>>(),
            vec!["sclm_username"]
        );
        assert!(!m.settings_fields[0].secret);
    }

    #[test]
    fn field_header_and_default_source_empty_are_errors() {
        let bad_header = VALID.replace(
            r#"{"key": "name", "label": {"de": "Name", "en": "Name"}, "type": "text", "required": true}"#,
            r#"{"key": "name", "label": {"de": "Name", "en": "Name"}, "type": "text", "required": true, "header": "  "}"#,
        );
        let r = validate_manifest(bad_header.as_bytes());
        assert!(
            r.errors.iter().any(|e| e.contains("empty 'header'")),
            "{r:?}"
        );

        let bad_src = VALID.replace(
            r#"{"key": "name", "label": {"de": "Name", "en": "Name"}, "type": "text", "required": true}"#,
            r#"{"key": "name", "label": {"de": "Name", "en": "Name"}, "type": "text", "default_source": ""}"#,
        );
        let r = validate_manifest(bad_src.as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("empty 'default_source'")),
            "{r:?}"
        );
    }

    // ---- SCLM v2 Karte B (Spec 5.3): scan-Block-Validierung ---------------

    /// VALID + settings_fields(sclm_username) + wohlgeformter scan.
    const SCAN_OK: &str = r#"{
        "manifest_version": 1,
        "id": "sclm",
        "name": {"de": "SCLM", "en": "SCLM"},
        "base_url": "https://sclm.site",
        "auth": {"type": "none"},
        "placement": {"kind": "own", "label": {"de": "SCLM", "en": "SCLM"}, "hotkey_slot": true},
        "settings_fields": [
            {"key": "sclm_username", "label": {"de": "N", "en": "N"}, "type": "text"}
        ],
        "scan": {
            "id": "my_raffles",
            "title": {"de": "Meine Verlosungen", "en": "My Raffles"},
            "list_endpoint": "/api/sessions",
            "done_field": "raffle_done",
            "code_field": "join_code",
            "id_field": "id",
            "name_field": "name",
            "detail_endpoint": "/api/session/{code}",
            "match_path": "participants[].name",
            "match_setting_key": "sclm_username",
            "done_cap": 20,
            "concurrency": 6,
            "poll_seconds": 300
        },
        "views": []
    }"#;

    fn patched_scan(find: &str, repl: &str) -> Report {
        validate_manifest(SCAN_OK.replace(find, repl).as_bytes())
    }

    #[test]
    fn scan_valid_manifest_passes() {
        let r = validate_manifest(SCAN_OK.as_bytes());
        // views:[] ist bewusst leer (Fokus scan) -> generische view-Warnung ist
        // okay; der scan selbst darf keine Fehler/Warnungen erzeugen.
        assert!(r.is_ok(), "{r:?}");
        assert!(
            !r.errors.iter().any(|e| e.contains("scan"))
                && !r.warnings.iter().any(|w| w.contains("scan")),
            "{r:?}"
        );
    }

    #[test]
    fn scan_requires_declared_match_setting_key() {
        let r = patched_scan(
            r#""match_setting_key": "sclm_username""#,
            r#""match_setting_key": "wild_key""#,
        );
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("not declared in settings_fields")),
            "{r:?}"
        );
    }

    #[test]
    fn scan_detail_endpoint_needs_exactly_code_placeholder() {
        let no_ph = patched_scan(r#""/api/session/{code}""#, r#""/api/session""#);
        assert!(
            no_ph
                .errors
                .iter()
                .any(|e| e.contains("exactly one placeholder {code}")),
            "{no_ph:?}"
        );
        let other_ph = patched_scan(r#""/api/session/{code}""#, r#""/api/session/{sid}""#);
        assert!(
            other_ph
                .errors
                .iter()
                .any(|e| e.contains("exactly one placeholder {code}")),
            "{other_ph:?}"
        );
        // Listen-Endpunkt darf KEINE Platzhalter haben.
        let list_ph = patched_scan(
            r#""list_endpoint": "/api/sessions""#,
            r#""list_endpoint": "/api/sessions?u={code}""#,
        );
        assert!(
            list_ph
                .errors
                .iter()
                .any(|e| e.contains("must not contain placeholders")),
            "{list_ph:?}"
        );
    }

    #[test]
    fn scan_creator_detail_endpoint_is_optional_and_validated() {
        // SCLM 4A (docs SCLM-FLOW-V2 §25): Kandidaten laufen über die sid
        // (öffentlich), die Ersteller-Vereinigung über den Keystore-Admin-Code
        // — dafür ist ein zweiter, optionaler Detail-Endpunkt im scan da.
        let ok = SCAN_OK.replace(
            r#""detail_endpoint": "/api/session/{code}","#,
            r#""detail_endpoint": "/api/session/{code}",
            "creator_detail_endpoint": "/api/session/{code}","#,
        );
        let r = validate_manifest(ok.as_bytes());
        assert!(r.errors.is_empty(), "{r:?}");
        // Fehlt das Feld: altes Verhalten, keine Fehler (Bestandsmanifeste).
        let absent = validate_manifest(SCAN_OK.as_bytes());
        assert!(absent.errors.is_empty(), "{absent:?}");
        // Fremder Platzhalter ist ungebunden -> Error mit Feldname.
        let bad_ph = SCAN_OK.replace(
            r#""detail_endpoint": "/api/session/{code}","#,
            r#""detail_endpoint": "/api/session/{code}",
            "creator_detail_endpoint": "/api/session/{sid}","#,
        );
        let r = validate_manifest(bad_ph.as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("creator_detail_endpoint")
                    && e.contains("exactly one placeholder {code}")),
            "{r:?}"
        );
        // Absolute URLs / fremde Schemata sind verboten (gleiche Regel wie
        // detail_endpoint — der Wert kommt aus dem Manifest, nicht vom Nutzer).
        let bad_url = SCAN_OK.replace(
            r#""detail_endpoint": "/api/session/{code}","#,
            r#""detail_endpoint": "/api/session/{code}",
            "creator_detail_endpoint": "https://evil.example/{code}","#,
        );
        let r = validate_manifest(bad_url.as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("creator_detail_endpoint") && e.contains("base_url-relative")),
            "{r:?}"
        );
    }

    #[test]
    fn scan_header_templates_only_bind_code() {
        // {code}-Header ist okay (keine scan-Fehler):
        let ok = SCAN_OK.replace(
            r#""match_setting_key": "sclm_username","#,
            r#""match_setting_key": "sclm_username", "headers": {"X-Test": "{code}"},"#,
        );
        let r = validate_manifest(ok.as_bytes());
        assert!(r.errors.is_empty(), "{r:?}");
        // Jeder andere Platzhalter ist ungebunden → Error.
        let bad = SCAN_OK.replace(
            r#""match_setting_key": "sclm_username","#,
            r#""match_setting_key": "sclm_username", "headers": {"X-Test": "{was}"},"#,
        );
        let bad = validate_manifest(bad.as_bytes());
        assert!(
            bad.errors
                .iter()
                .any(|e| e.contains("only {code} is available")),
            "{bad:?}"
        );
    }

    #[test]
    fn scan_caps_and_id_collisions_are_errors() {
        let zero = patched_scan(r#""done_cap": 20"#, r#""done_cap": 0"#);
        assert!(
            zero.errors.iter().any(|e| e.contains("done_cap")),
            "{zero:?}"
        );
        let huge = patched_scan(r#""done_cap": 20"#, r#""done_cap": 500"#);
        assert!(huge.errors.iter().any(|e| e.contains("flood")), "{huge:?}");
        let conc0 = patched_scan(r#""concurrency": 6"#, r#""concurrency": 0"#);
        assert!(
            conc0.errors.iter().any(|e| e.contains("concurrency")),
            "{conc0:?}"
        );
        let conc_hi = patched_scan(r#""concurrency": 6"#, r#""concurrency": 40"#);
        assert!(
            conc_hi.warnings.iter().any(|e| e.contains("clamped")),
            "{conc_hi:?}"
        );
        assert!(conc_hi.is_ok(), "{conc_hi:?}");
        // Scan-ID kollidiert mit View-ID (Render-/Cache-Namespace).
        let coll = SCAN_OK.replace(r#""id": "my_raffles""#, r#""id": "sessions""#).replace(
            r#""views": []"#,
            r#""views": [{"id": "sessions", "type": "table", "title": {"de": "T", "en": "T"}, "endpoint": "/api/sessions", "mapping": {"name": "[].name"}}]"#,
        );
        let r = validate_manifest(coll.as_bytes());
        assert!(
            r.errors.iter().any(|e| e.contains("collides with view id")),
            "{r:?}"
        );
    }
    // ---- Härtung t_3cfc7842 Finding 1: poll_seconds-Obergrenzen -------------------
    #[test]
    fn scan_poll_seconds_upper_bounds() {
        // Reviewer-Sonden-Literal (u64::MAX/500) + plausibler Überdehnungsfall.
        let huge = patched_scan(r#""poll_seconds": 300"#, r#""poll_seconds": 10000001"#);
        assert!(
            huge.errors.iter().any(|e| e.contains("poll_seconds")),
            "{huge:?}"
        );
        assert!(!huge.is_ok(), "{huge:?}");
        let max = patched_scan(
            r#""poll_seconds": 300"#,
            &(r#""poll_seconds": "#.to_string() + &(u64::MAX / 500).to_string()),
        );
        assert!(
            max.errors.iter().any(|e| e.contains("poll_seconds")),
            "{max:?}"
        );
        // Warn-Grenze: > 1 Tag, aber unterhalb der Fehlergrenze.
        let warny = patched_scan(r#""poll_seconds": 300"#, r#""poll_seconds": 86401"#);
        assert!(warny.is_ok(), "{warny:?}");
        assert!(
            warny.warnings.iter().any(|w| w.contains("poll_seconds")),
            "{warny:?}"
        );
        // Grenzwerte: exakt 10^7 ist noch erlaubt (aber Warnung > 1 Tag),
        // 10^7+1 kippt in den Fehler; exakt 86400 (1 Tag) ist warnfrei.
        let edge = patched_scan(r#""poll_seconds": 300"#, r#""poll_seconds": 10000000"#);
        assert!(edge.is_ok(), "{edge:?}");
        assert!(
            edge.warnings.iter().any(|w| w.contains("poll_seconds")),
            "{edge:?}"
        );
        let over = patched_scan(r#""poll_seconds": 300"#, r#""poll_seconds": 10000001"#);
        assert!(!over.is_ok(), "{over:?}");
        let day = patched_scan(r#""poll_seconds": 300"#, r#""poll_seconds": 86400"#);
        assert!(
            !day.warnings.iter().any(|w| w.contains("poll_seconds")),
            "{day:?}"
        );
    }

    // ---- SCLM v2 (Spec 5.4): open-Block, enabled_if, keystore_fields --------

    fn manifest_with_open_flow(flow: &str) -> String {
        format!(
            r#"{{"manifest_version": 1, "id": "sclm", "name": {{"de": "SCLM", "en": "SCLM"}}, "base_url": "https://sclm.site", "placement": {{"kind": "own", "label": {{"de": "SCLM", "en": "SCLM"}}, "hotkey_slot": true}}, "views": [], "settings_fields": [{{"key": "sclm_username", "label": {{"de": "Name", "en": "Name"}}, "type": "text", "required": true}}], "flows": [{flow}]}}"#
        )
    }

    const OPEN_OK: &str = r#""open": {
        "param": "code",
        "session_endpoint": "/api/session/{code}",
        "headers": {"X-SCLM-Code": "{code}"},
        "mapping": {"sid": "id", "max_wishes": "settings.max_wishes", "raffle_done": "raffle_done"},
        "match_path": "participants[].name",
        "id_path": "participants[].id",
        "match_setting_key": "sclm_username",
        "join_endpoint": "/api/session/{sid}/participants",
        "join_headers": {"X-SCLM-Code": "{code}"},
        "open_screen": "a",
        "done_screen": "d"
    }"#;

    const FLOW_HEAD: &str =
        r#"{"id": "participant", "title": {"de": "T", "en": "T"}, "entry": "a", "#;

    const SCREENS_OK: &str = r#""screens": [
        {"id": "a", "type": "form", "title": {"de": "a", "en": "a"}, "form": {"fields": [{"key": "sclm_username", "label": {"de": "x", "en": "x"}, "type": "text", "required": true, "default_source": "sclm_username"}], "method": "post", "path": "/api/x", "body_template": {"name": "{sclm_username}"}}},
        {"id": "d", "type": "results", "title": {"de": "d", "en": "d"}, "endpoint": "/api/r/{rid}", "params": [{"key": "rid", "label": {"de": "r", "en": "r"}}]}
    ]}"#;

    fn open_flow(open: &str) -> String {
        FLOW_HEAD.to_string() + open + ", " + SCREENS_OK
    }

    #[test]
    fn flow_open_block_valid_and_defaults_apply() {
        let m = manifest_with_open_flow(&open_flow(OPEN_OK));
        let r = validate_manifest(m.as_bytes());
        assert!(r.is_ok(), "{r:?}");
    }

    #[test]
    fn flow_open_block_rules() {
        // mapping ohne sid → Error (join-POST braucht {sid})
        let no_sid = OPEN_OK.replace(
            r#""mapping": {"sid": "id", "max_wishes": "settings.max_wishes", "raffle_done": "raffle_done"}"#,
            r#""mapping": {"max_wishes": "settings.max_wishes"}"#,
        );
        let r = validate_manifest(manifest_with_open_flow(&open_flow(&no_sid)).as_bytes());
        assert!(r.errors.iter().any(|e| e.contains("'sid' key")), "{r:?}");
        // Match-Key nicht in settings_fields deklariert → Error
        let ghost = OPEN_OK.replace("\"sclm_username\"", "\"sclm_ghost\"");
        let r = validate_manifest(manifest_with_open_flow(&open_flow(&ghost)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("not declared in settings_fields")),
            "{r:?}"
        );
        // done_screen zeigt auf unbekannten Screen → Error
        let ghost_screen = OPEN_OK.replace(r#""done_screen": "d""#, r#""done_screen": "d nope""#);
        let r = validate_manifest(manifest_with_open_flow(&open_flow(&ghost_screen)).as_bytes());
        assert!(
            r.errors.iter().any(|e| e.contains("unknown screen")),
            "{r:?}"
        );
        // session_endpoint mit fremdem Platzhalter → Error
        let wrong_ph = OPEN_OK.replace("/api/session/{code}", "/api/session/{sid}");
        let r = validate_manifest(manifest_with_open_flow(&open_flow(&wrong_ph)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("exactly one placeholder")),
            "{r:?}"
        );
    }

    #[test]
    fn flow_open_progress_and_self_wishes_guardrails() {
        // D-A: progress-Felder + self_wishes sind optional, aber wohlgeformt
        // prüfen (Muster wie flow_open_block_rules).
        // Saubere Variante: progress zeigt auf existierenden Screen, self_wishes
        // als reiner Schalter ({}) mit serde-Defaults.
        let good = OPEN_OK.replace(
            r#""done_screen": "d"#,
            r#""progress_path": "wishes", "progress_screen": "d", "self_wishes": {}, "done_screen": "d"#,
        );
        let r = validate_manifest(manifest_with_open_flow(&open_flow(&good)).as_bytes());
        assert!(r.is_ok(), "{r:?}");
        // progress_screen auf unbekannten Screen → Error
        let ghost = good.replace(r#""progress_screen": "d""#, r#""progress_screen": "nope""#);
        let r = validate_manifest(manifest_with_open_flow(&open_flow(&ghost)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("progress_screen references unknown screen")),
            "{r:?}"
        );
        // progress_screen ohne progress_path → Error (Ziel ohne Auslöser)
        let orphan = good.replace(r#""progress_path": "wishes", "#, "");
        let r = validate_manifest(manifest_with_open_flow(&open_flow(&orphan)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("requires a non-empty progress_path")),
            "{r:?}"
        );
        // progress_path leer → Error
        let empty = good.replace(r#""progress_path": "wishes""#, r#""progress_path": " ""#);
        let r = validate_manifest(manifest_with_open_flow(&open_flow(&empty)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("progress_path is empty")),
            "{r:?}"
        );
        // self_wishes mit leerem collection-String → Error
        let bad_sw = good.replace(
            r#""self_wishes": {}"#,
            r#""self_wishes": {"collection": " "}"#,
        );
        let r = validate_manifest(manifest_with_open_flow(&open_flow(&bad_sw)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("self_wishes.collection is empty")),
            "{r:?}"
        );
    }

    #[test]
    fn view_and_flow_screen_get_poll_bounds_too() {
        // view: Obergrenze greift wie beim scan (Fehler).
        let bad_view = r#""views": [{"id": "sessions", "type": "table", "title": {"de": "T", "en": "T"}, "endpoint": "/api/sessions", "mapping": {"name": "[].name"}, "poll_seconds": 10000001}]"#;
        let v = validate_manifest(SCAN_OK.replace(r#""views": []"#, bad_view).as_bytes());
        assert!(
            v.errors
                .iter()
                .any(|e| e.contains("poll_seconds") && e.contains("sessions")),
            "{v:?}"
        );
        // flow-Warte-Screen: Warn-Grenze (> 1 Tag).
        let flow_json = r#""views": [{"id": "sessions", "type": "table", "title": {"de": "T", "en": "T"}, "endpoint": "/api/sessions", "mapping": {"name": "[].name"}}], "flows": [{"id": "f1", "title": {"de": "F", "en": "F"}, "entry": "code", "screens": [{"id": "code", "type": "wait", "title": {"de": "W", "en": "W"}, "endpoint": "/api/wait", "mapping": {"raffle_done": "raffle_done"}, "poll_seconds": 86401, "exit": {"on": "raffle_done", "op": "equals", "value": true, "then": "code"}}]}]"#;
        let f = validate_manifest(SCAN_OK.replace(r#""views": []"#, flow_json).as_bytes());
        assert!(
            f.warnings
                .iter()
                .any(|w| w.contains("poll_seconds") && w.contains("code")),
            "{f:?}"
        );
        assert!(f.is_ok(), "{f:?}");
    }

    #[test]
    fn flow_open_action_guardrails() {
        // enabled_if mit then/otherwise → Error (nur Aktivierungsregel);
        // unbekannte Keystore-Spalte → Error. Reine String-Konkatenation —
        // JSON-Klammern dürfen nicht durch format!-Interpolation laufen.
        let bad_screens = r#""screens": [
            {"id": "a", "type": "form", "title": {"de": "a", "en": "a"}, "form": {"fields": [{"key": "sclm_username", "label": {"de": "x", "en": "x"}, "type": "text", "required": true, "default_source": "sclm_username"}], "method": "post", "path": "/api/x", "body_template": {"name": "{sclm_username}"}}},
            {"id": "d", "type": "results", "title": {"de": "d", "en": "d"}, "endpoint": "/api/r/{rid}", "params": [{"key": "rid", "label": {"de": "r", "en": "r"}}],
             "keystore_fields": {"bogus_col": "rid"},
             "actions": [{"id": "collect", "title": {"de": "c", "en": "c"}, "method": "patch", "path": "/api/r/{rid}",
                 "enabled_if": {"on": "collected", "op": "not_equals", "value": true, "then": "a"}}]}
        ]}"#;
        let flow = FLOW_HEAD.to_string() + OPEN_OK + ", " + bad_screens;
        let r = validate_manifest(manifest_with_open_flow(&flow).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("enabled_if must not use")),
            "{r:?}"
        );
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("keystore_fields column 'bogus_col'")),
            "{r:?}"
        );
        // saubere Variante: enabled_if ohne then, erlaubte Keystore-Spalte
        let clean = bad_screens
            .replace(r#", "then": "a""#, "")
            .replace(r#""bogus_col": "rid""#, r#""result_id": "rid""#);
        let flow_clean = FLOW_HEAD.to_string() + OPEN_OK + ", " + &clean;
        let r = validate_manifest(manifest_with_open_flow(&flow_clean).as_bytes());
        assert!(r.is_ok(), "{r:?}");
    }

    // ---- Ersteller-Cockpit v1 (Spec 15): tabs + Admin-Einstieg/Route --------

    fn manifest_with_flows(flows: &[&str]) -> String {
        format!(
            r#"{{"manifest_version": 1, "id": "sclm", "name": {{"de": "SCLM", "en": "SCLM"}}, "base_url": "https://sclm.site", "placement": {{"kind": "own", "label": {{"de": "SCLM", "en": "SCLM"}}, "hotkey_slot": true}}, "views": [], "settings_fields": [{{"key": "sclm_username", "label": {{"de": "Name", "en": "Name"}}, "type": "text", "required": true}}], "flows": [{}]}}"#,
            flows.join(", ")
        )
    }

    /// Admin-Einstieg: alle Join-Felder fehlend, dafür admin_path.
    const ADMIN_OPEN_OK: &str = r#""open": {
        "session_endpoint": "/api/session/{code}",
        "headers": {"X-SCLM-Code": "{code}"},
        "mapping": {"sid": "id", "admin_code": "admin_code", "join_code": "join_code"},
        "admin_path": "is_admin",
        "open_screen": "ca",
        "done_screen": "cd"
    }"#;

    const ADMIN_SCREENS_OK: &str = r#""screens": [
        {"id": "ca", "type": "detail", "title": {"de": "ca", "en": "ca"}, "endpoint": "/api/session/{code}", "params": [{"key": "code", "label": {"de": "c", "en": "c"}}]},
        {"id": "cd", "type": "results", "title": {"de": "cd", "en": "cd"}, "endpoint": "/api/x"}
    ]}"#;

    fn admin_flow(open: &str) -> String {
        r#"{"id": "creator", "title": {"de": "C", "en": "C"}, "entry": "ca", "#.to_string()
            + open
            + ", "
            + ADMIN_SCREENS_OK
    }

    #[test]
    fn flow_tabs_validate_ids_screens_and_bounds() {
        let tabs_ok = r#""tabs": [
            {"id": "overview", "screen": "a", "label": {"de": "Übersicht", "en": "Overview"}},
            {"id": "raffle", "screen": "d", "label": {"de": "Verlosung", "en": "Raffle"}}
        ], "#;
        let flow = FLOW_HEAD.to_string() + tabs_ok + OPEN_OK + ", " + SCREENS_OK;
        let r = validate_manifest(manifest_with_open_flow(&flow).as_bytes());
        assert!(r.is_ok(), "{r:?}");
        // Tab zeigt auf unbekannten Screen → Error
        let ghost = flow.replace(r#""screen": "d""#, r#""screen": "nope""#);
        let r = validate_manifest(manifest_with_open_flow(&ghost).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("tab screen references unknown screen")),
            "{r:?}"
        );
        // doppelte Tab-ID → Error
        let dup = flow.replace(r#""id": "raffle""#, r#""id": "overview""#);
        let r = validate_manifest(manifest_with_open_flow(&dup).as_bytes());
        assert!(
            r.errors.iter().any(|e| e.contains("duplicate tab id")),
            "{r:?}"
        );
        // 7 Tabs → Error (1–6)
        let many = r#""tabs": [
            {"id": "t1", "screen": "a", "label": {"de": "1", "en": "1"}},
            {"id": "t2", "screen": "a", "label": {"de": "2", "en": "2"}},
            {"id": "t3", "screen": "a", "label": {"de": "3", "en": "3"}},
            {"id": "t4", "screen": "a", "label": {"de": "4", "en": "4"}},
            {"id": "t5", "screen": "a", "label": {"de": "5", "en": "5"}},
            {"id": "t6", "screen": "a", "label": {"de": "6", "en": "6"}},
            {"id": "t7", "screen": "a", "label": {"de": "7", "en": "7"}}
        ], "#;
        let flow = FLOW_HEAD.to_string() + many + OPEN_OK + ", " + SCREENS_OK;
        let r = validate_manifest(manifest_with_open_flow(&flow).as_bytes());
        assert!(
            r.errors.iter().any(|e| e.contains("tabs must have 1-6")),
            "{r:?}"
        );
        // leere Labels → Error
        let empty_label =
            r#""tabs": [{"id": "overview", "screen": "a", "label": {"de": " ", "en": " "}}], "#;
        let flow = FLOW_HEAD.to_string() + empty_label + OPEN_OK + ", " + SCREENS_OK;
        let r = validate_manifest(manifest_with_open_flow(&flow).as_bytes());
        assert!(r.errors.iter().any(|e| e.contains("empty label")), "{r:?}");
    }

    #[test]
    fn flow_open_join_fields_must_be_complete_pair() {
        // Halbpaar: join_endpoint fehlt, Match-Felder stehen → Error.
        let half = OPEN_OK.replace(r#""join_endpoint": "/api/session/{sid}/participants","#, "");
        let r = validate_manifest(manifest_with_open_flow(&open_flow(&half)).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("must all be present or all absent")),
            "{r:?}"
        );
    }

    #[test]
    fn flow_open_admin_entry_requires_admin_path_guardrail() {
        // Admin-Einstieg komplett (ohne join-Felder, mit admin_path) → ok.
        let r = validate_manifest(manifest_with_flows(&[&admin_flow(ADMIN_OPEN_OK)]).as_bytes());
        assert!(r.is_ok(), "{r:?}");
        // Admin-Einstieg ohne admin_path → Error (Guardrail fehlt).
        let no_guard = ADMIN_OPEN_OK.replace(r#""admin_path": "is_admin","#, "");
        let r = validate_manifest(manifest_with_flows(&[&admin_flow(&no_guard)]).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("admin entry (no join fields) requires admin_path")),
            "{r:?}"
        );
    }

    #[test]
    fn flow_open_admin_flow_target_rules() {
        let route = OPEN_OK.replace(
            r#""open_screen""#,
            r#""admin_path": "is_admin", "admin_flow": "creator", "open_screen""#,
        );
        let routed_flow = open_flow(&route);
        let admin_target = admin_flow(ADMIN_OPEN_OK);
        let flows = [routed_flow.as_str(), admin_target.as_str()];
        // Sauber: Teilnehmer-Flow routed auf Admin-Flow.
        let r = validate_manifest(manifest_with_flows(&flows).as_bytes());
        assert!(r.is_ok(), "{r:?}");
        // unbekannter Ziel-Flow → Error
        let ghost = flows[0].replace(r#""admin_flow": "creator""#, r#""admin_flow": "nope""#);
        let r = validate_manifest(manifest_with_flows(&[&ghost, flows[1]]).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("admin_flow references unknown flow")),
            "{r:?}"
        );
        // Ziel-Flow ohne open-Block → Error
        let no_open = r#"{"id": "creator", "title": {"de": "C", "en": "C"}, "entry": "ca", "#
            .to_string()
            + ADMIN_SCREENS_OK;
        let r = validate_manifest(manifest_with_flows(&[flows[0], &no_open]).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("admin_flow target 'creator' has no open block")),
            "{r:?}"
        );
        // Ziel-Flow mit Join-Feldern (Teilnehmer-Semantik) → Error
        let bad_target = flows[1].replace(
            r#""admin_path": "is_admin""#,
            r#""match_path": "participants[].name", "id_path": "participants[].id", "match_setting_key": "sclm_username", "join_endpoint": "/api/session/{sid}/participants", "admin_path": "is_admin""#,
        );
        let r = validate_manifest(manifest_with_flows(&[flows[0], &bad_target]).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("must be an admin entry")),
            "{r:?}"
        );
        // admin_flow ohne admin_path → Error
        let orphan = flows[0].replace(r#""admin_path": "is_admin", "#, "");
        let r = validate_manifest(manifest_with_flows(&[&orphan, flows[1]]).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("admin_flow 'creator' requires admin_path")),
            "{r:?}"
        );
        // admin_path ohne admin_flow am Teilnehmer-Einstieg → Error
        let half_route = flows[0].replace(r#", "admin_flow": "creator""#, "");
        let r = validate_manifest(manifest_with_flows(&[&half_route, flows[1]]).as_bytes());
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("admin_path on a participant entry requires admin_flow")),
            "{r:?}"
        );
    }
}

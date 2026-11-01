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
}

/// Flow-Regeln (SF1): IDs eindeutig, Entry existiert, Screen-Referenzen
/// (Transitions/Branches) zeigen nur innerhalb des eigenen Flows auf
/// existierende Screens, Typ-Spezifika (wait braucht exit, form braucht
/// form-Block, sonst Endpunkte), Autocomplete-Regeln wohlgeformt.
fn check_flows(m: &ConnectorManifest, report: &mut Report) {
    let mut flow_ids = std::collections::BTreeSet::new();
    for flow in m.flows.iter().flatten() {
        if !seen_insert(&mut flow_ids, &flow.id) {
            report.error(format!("duplicate flow id '{}'", flow.id));
            continue;
        }
        check_flow(flow, report);
    }
}

fn seen_insert<'a>(set: &mut std::collections::BTreeSet<&'a str>, id: &'a str) -> bool {
    if id.trim().is_empty() {
        return true; // empty-ID-Fehler wird am Screen selbst gemeldet
    }
    set.insert(id)
}

fn check_flow(flow: &Flow, report: &mut Report) {
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
    if s.poll_seconds == Some(0) {
        report.warn(format!(
            "flow '{flow_id}': screen '{sid}': poll_seconds=0 — floody polling"
        ));
    }
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
        if let Some(mp) = &multi.max_path {
            if mp.trim().is_empty() {
                report.error(format!(
                    "flow '{flow_id}': screen '{sid}': multi.max_path is empty"
                ));
            }
        }
    }
    // SF2: Actions — eindeutige IDs, method/path-Paarigkeit, goto-Referenzen,
    // Header-Templates gegen die deklarierten Action-Params.
    let mut action_ids = std::collections::BTreeSet::new();
    for a in s.actions.iter().flatten() {
        if !seen_insert(&mut action_ids, &a.id.as_str()) {
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
        if field.trim().is_empty() || path.trim().is_empty() {
            report.error(format!("{ctx}: empty fill field or path"));
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
    if v.poll_seconds == Some(0) {
        report.warn(format!("view '{}': poll_seconds=0 — floody polling", v.id));
    }
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
    fn shipped_sclm_manifest_validates() {
        // Das mitgelieferte SCLM-Pilot-Manifest (C3) muss sauber durch den
        // Validator laufen — genau so, wie ein Nutzer es importieren würde.
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/sclm.json");
        let text = std::fs::read_to_string(&path).expect("examples/sclm.json exists");
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
}

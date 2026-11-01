//! Strenger Validator (docs/research/connector-system.md §2/§3):
//! Schema-Check via Deserialisierung, https-only, keine file://-Pfade,
//! Größenlimit. Zusätzliche Abweichungen sind **Warnungen** — die UI bietet
//! die bewusste „Warnung ignorieren“-Option (Verantwortung des Nutzers);
//! **Errors** blockieren Import immer.

use crate::manifest::{
    Auth, AuthType, ConnectorManifest, FieldType, FormField, HttpMethod, View, ViewType,
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
    if base.contains("file://") || base.contains("localhost.invalid") && !base.starts_with("https://")
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
                if auth.header.as_deref().map(str::trim).unwrap_or("").is_empty() {
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

    // Views: eindeutige IDs, Endpunkte https-relativ, form-Regeln
    let mut seen = std::collections::BTreeSet::new();
    for view in &m.views {
        if !seen.insert(view.id.as_str()) {
            report.error(format!("duplicate view id '{}'", view.id));
        }
        check_view(view, report);
    }

    if m.views.is_empty() {
        report.warn("manifest declares no views".into());
    }
}

fn check_view(v: &View, report: &mut Report) {
    let _ = ViewType::Calendar;
    if v.endpoint.trim().is_empty() {
        report.error(format!("view '{}': empty endpoint", v.id));
        return;
    }
    // Endpunkte sind relativ zu base_url; absolute URLs und file:// sind Error.
    if v.endpoint.contains("file://") {
        report.error(format!("view '{}': file:// not allowed in endpoint", v.id));
    }
    if v.endpoint.starts_with("http://") {
        report.error(format!(
            "view '{}': http:// not allowed (base_url is https)",
            v.id
        ));
    }
    if v.endpoint.starts_with("https://") {
        report.warn(format!(
            "view '{}': absolute endpoint overrides base_url — verify deliberately",
            v.id
        ));
    }
    if let Some(p) = v.poll_seconds {
        if p == 0 {
            report.warn(format!("view '{}': poll_seconds=0 — floody polling", v.id));
        }
    }

    if v.view_type == ViewType::Form {
        match &v.form {
            Some(form) => check_form(&v.id, form, report),
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

fn check_form(view_id: &str, form: &crate::manifest::FormView, report: &mut Report) {
    if form.fields.is_empty() {
        report.error(format!("view '{view_id}': form has no fields"));
    }
    if form.path.trim().is_empty() {
        report.error(format!("view '{view_id}': form path empty"));
    }
    // {param}-Platzhalter im Pfad müssen Felder sein
    let field_keys: std::collections::BTreeSet<&str> =
        form.fields.iter().map(|f| f.key.as_str()).collect();
    for param in extract_params(&form.path) {
        if !field_keys.contains(param.as_str()) {
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
    check_template(&form.body_template, view_id, &field_keys, report);
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
        let no_source = VALID.replace(
            r#""value_source": "connector.sclm.code""#,
            "",
        );
        let r = validate_manifest(no_source.as_bytes());
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.contains("value_source")));
    }

    #[test]
    fn form_view_requires_form_block_and_matching_params() {
        let missing_form = VALID.replace(r#""form": {"#, r#""form_disabled": {"#);
        let r = validate_manifest(missing_form.as_bytes());
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.contains("missing 'form'")));

        let bad_param = VALID.replace("/api/session/{code}/participants", "/api/session/{kode}/participants");
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
    fn duplicate_view_ids_error() {
        let dup = VALID.replace(r#""id": "join""#, r#""id": "sessions""#);
        let r = validate_manifest(dup.as_bytes());
        assert!(!r.is_ok());
        assert!(r.errors.iter().any(|e| e.contains("duplicate view id")));
    }
}

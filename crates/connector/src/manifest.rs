//! Serde-Schema für Connector-Manifeste (v1).
//!
//! Streng per `deny_unknown_fields` — ein Manifest mit Tippfern muss beim
//! Import scheitern, nicht stillschweigend Felder ignorieren.

use serde::Deserialize;
use std::collections::BTreeMap;

/// Lokalisierter Text (de/en, Fallback en im UI).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalizedText {
    pub de: String,
    pub en: String,
}

/// Auth-Deklaration. `header` = Header-Auth (z. B. `X-SCLM-Code`), dessen
/// Wert aus einer Nutzer-Eingabe pro Connector in den Einstellungen kommt
/// (`value_source` = Settings-Key). Session-Codes sind WERTE, keine Auth.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Auth {
    #[serde(rename = "type")]
    pub auth_type: AuthType,
    /// Header-Name (nur bei `header`-Auth, Pflicht).
    pub header: Option<String>,
    /// Settings-Key, unter dem der Nutzer den Wert speichert (Pflicht bei `header`).
    pub value_source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthType {
    Header,
    None,
}

/// View-Typen: table | cards | detail | calendar | form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewType {
    Table,
    Cards,
    Detail,
    Calendar,
    Form,
}

/// Ein View im Manifest.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    pub id: String,
    #[serde(rename = "type")]
    pub view_type: ViewType,
    pub title: LocalizedText,
    /// API-Endpunkt (relativ zu `base_url`).
    pub endpoint: String,
    /// Feld → Dot-Path in der Antwort (`"data[].ship_name"`).
    #[serde(default)]
    pub mapping: BTreeMap<String, String>,
    /// Polling-Intervall (Sekunden). Default 300 — sparsam (Live-API-Regel).
    #[serde(default)]
    pub poll_seconds: Option<u64>,
    /// Nur bei `type: form`: Formular-Deklaration.
    pub form: Option<FormView>,
}

/// Formular-View (durch SCLM getrieben): Eingabefelder + method + path
/// (+ Pfad-Parameter aus der Auswahl) + body_template.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormView {
    /// Pflicht bei `type: form`.
    pub fields: Vec<FormField>,
    pub method: HttpMethod,
    /// Pfad mit `{param}`-Platzhaltern aus den Feldern, z. B. `/api/session/{code}/wishes`.
    pub path: String,
    /// Body-Vorlage; `{param}`-Platzhalter werden ersetzt.
    pub body_template: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HttpMethod {
    Get,
    Post,
    Patch,
    Put,
    Delete,
}

/// Eingabefeld im Formular.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormField {
    /// Schlüssel im Template/Pfad.
    pub key: String,
    pub label: LocalizedText,
    #[serde(rename = "type")]
    pub field_type: FieldType,
    #[serde(default)]
    pub required: bool,
    /// Nur bei `type: select`.
    pub options: Option<Vec<LocalizedText>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Text,
    Number,
    Select,
}

/// L2-Sync (für AAWA später): Master-Richtung + Strategie pro Connector.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sync {
    pub master: SyncMaster,
    pub strategy: SyncStrategy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncMaster {
    Platform,
    Local,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncStrategy {
    Replace,
    Merge,
}

/// Root-Manifest.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorManifest {
    pub manifest_version: u32,
    /// Eindeutige ID (ASCII, `-`/`_` erlaubt).
    pub id: String,
    pub name: LocalizedText,
    /// Muss https:// sein (Validator).
    pub base_url: String,
    pub auth: Option<Auth>,
    pub views: Vec<View>,
    pub sync: Option<Sync>,
}

impl ConnectorManifest {
    /// Deserialisiert aus Bytes (mit Größenlimit-Prüfung seitens Validator).
    pub fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

//! Serde-Schema für Connector-Manifeste (v1).
//!
//! Streng per `deny_unknown_fields` — ein Manifest mit Tippfern muss beim
//! Import scheitern, nicht stillschweigend Felder ignorieren.

use crate::flow::Flow;
use serde::Deserialize;
use std::collections::BTreeMap;

/// Lokalisierter Text (de/en, Fallback en im UI).
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocalizedText {
    pub de: String,
    pub en: String,
}

/// Auth-Deklaration. `header` = Header-Auth (z. B. `X-SCLM-Code`), dessen
/// Wert aus einer Nutzer-Eingabe pro Connector in den Einstellungen kommt
/// (`value_source` = Settings-Key). Session-Codes sind WERTE, keine Auth.
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Auth {
    #[serde(rename = "type")]
    pub auth_type: AuthType,
    /// Header-Name (nur bei `header`-Auth, Pflicht).
    pub header: Option<String>,
    /// Settings-Key, unter dem der Nutzer den Wert speichert (Pflicht bei `header`).
    pub value_source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthType {
    Header,
    None,
}

/// View-Typen: table | cards | detail | calendar | form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewType {
    Table,
    Cards,
    Detail,
    Calendar,
    Form,
}

/// Param-Deklaration für Views mit `{param}`-Platzhaltern im Endpoint
/// (z. B. Session-Detail `GET /api/session/{code}`): Label für die
/// Code-Eingabe im UI.
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Param {
    pub key: String,
    pub label: LocalizedText,
    /// Pflichtfeld (nur für Action-Eingabefelder relevant, SF2-R1 D3;
    /// Default true — gepinnte Werte zählen als erfüllt).
    #[serde(default)]
    pub required: Option<bool>,
}

/// Ein View im Manifest.
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    pub id: String,
    #[serde(rename = "type")]
    pub view_type: ViewType,
    pub title: LocalizedText,
    /// API-Endpunkt (relativ zu `base_url`). Darf `{param}`-Platzhalter
    /// enthalten — jeder muss in `params` deklariert sein.
    pub endpoint: String,
    /// Feld → Dot-Path in der Antwort (`"data[].ship_name"`).
    #[serde(default)]
    pub mapping: BTreeMap<String, String>,
    /// Polling-Intervall (Sekunden). Default 300 — sparsam (Live-API-Regel).
    #[serde(default)]
    pub poll_seconds: Option<u64>,
    /// Nur bei `type: form`: Formular-Deklaration.
    pub form: Option<FormView>,
    /// Platzhalter-Parameter des Endpoints (Pflicht bei `{param}` darin).
    pub params: Option<Vec<Param>>,
    /// Request-Header als `{param}`-Templates (z. B. Session-Code-Auth).
    pub headers: Option<BTreeMap<String, String>>,
}

/// Formular-View (durch SCLM getrieben): Eingabefelder + method + path
/// (+ Pfad-Parameter aus der Auswahl) + body_template.
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HttpMethod {
    Get,
    Post,
    Patch,
    Put,
    Delete,
}

/// Eingabefeld im Formular.
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
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
    /// Optional: zusätzlich als Request-Header senden (Wert des Feldes),
    /// z. B. ein Session-Code als `X-SCLM-Code`.
    pub header: Option<String>,
    /// Optional: Voreinstellung aus einem Settings-Wert (Whitelist),
    /// wenn das Feld leer bleibt — z. B. Standard-Benutzername.
    pub default_source: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Text,
    Number,
    Select,
}

/// Manifest-deklariertes Settings-Feld (SCLM v2 Karte A / Spec 5.1): generische
/// Plugin-Einstellungsgruppe in den Overlay-Einstellungen — der Wert wird pro
/// Connector persistiert (analog der Auth-Feld-Mechanik) und ist im Flow-Kontext
/// als `{key}`-Placeholder verfügbar. v1 unterstützt nur `type: "text"`.
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsField {
    /// Settings-Key (ASCII [A-Za-z0-9_-]), z. B. `sclm_username`.
    pub key: String,
    pub label: LocalizedText,
    /// v1: nur text (serde erzwingt den String "text").
    #[serde(rename = "type")]
    pub field_type: SettingsFieldType,
    #[serde(default)]
    pub required: bool,
    /// true: Wert wie ein Secret behandeln — verlässt das Frontend nie
    /// (nur `_set`-Booles), Maskierung in der UI.
    #[serde(default)]
    pub secret: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
pub enum SettingsFieldType {
    #[serde(rename = "text")]
    Text,
}

/// L2-Sync (für AAWA später): Master-Richtung + Strategie pro Connector.
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Sync {
    pub master: SyncMaster,
    pub strategy: SyncStrategy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncMaster {
    Platform,
    Local,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncStrategy {
    Replace,
    Merge,
}

/// Placement-Konzept (Sebo-Korrektur 2026-11-02): KEIN universeller
/// „Dienste“-Bereich — jedes Manifest deklariert, WO seine Views erscheinen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementKind {
    /// Eigener Top-Level-Bereich (wie ein Mode).
    Own,
    /// Views ERSETZEN eine bestehende Sektion (lokale Daten bleiben erhalten).
    Replace,
    /// Views fügen sich in einen bestehenden Bereich ein (Unter-Sektion/Tab).
    Embed,
}

#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    pub kind: PlacementKind,
    /// Pflicht bei `own`: Label des Top-Level-Bereichs.
    pub label: Option<LocalizedText>,
    /// Nur bei `own`: bekommt einen eigenen konfigurierbaren Mode-Hotkey-Slot.
    #[serde(default)]
    pub hotkey_slot: bool,
    /// Pflicht bei `replace`/`embed`: Ziel-Sektion (z. B. `hangar`, `inventory`).
    pub target: Option<String>,
}

/// Root-Manifest.
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorManifest {
    pub manifest_version: u32,
    /// Eindeutige ID (ASCII, `-`/`_` erlaubt).
    pub id: String,
    pub name: LocalizedText,
    /// Muss https:// sein (Validator).
    pub base_url: String,
    pub auth: Option<Auth>,
    /// WO die Views erscheinen (own | replace | embed).
    pub placement: Placement,
    pub views: Vec<View>,
    /// Screen-Flows (SF1): benannte Screen-Ketten — z. B. SCLM
    /// Teilnehmer-/Ersteller-Flow. Optional: Flows sind additiv zu Views.
    pub flows: Option<Vec<Flow>>,
    /// Generische Settings-Felder des Plugins (Karte A): erscheinen in den
    /// Overlay-Einstellungen je aktivem Connector. Optional — Manifeste ohne
    /// Felder bleiben gültig (Rückwärtskompatibilität).
    #[serde(default)]
    pub settings_fields: Vec<SettingsField>,
    pub sync: Option<Sync>,
}

impl ConnectorManifest {
    /// Deserialisiert aus Bytes (mit Größenlimit-Prüfung seitens Validator).
    pub fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

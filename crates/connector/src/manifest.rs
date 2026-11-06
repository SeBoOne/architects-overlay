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
    /// Ersteller-Cockpit v1 (Karte F, Spec 15): gilt nur für Action-`fields` —
    /// Eingabetyp des Action-Feldes (`text` | `number` | `select`). Default
    /// `text`; Manifests ohne Typangabe bleiben byte-identisch (Key wird nur
    /// serialisiert, wenn deklariert). View-`params` ignorieren das Feld.
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub field_type: Option<FieldType>,
    /// Nur bei `field_type: "select"`: die Optionen (de/en), mindestens eine
    /// (Validator). Der Renderer zeichnet daraus das Auswahlfeld und sendet
    /// den gewählten Wert als `{key}`-Platzhalter in Body/Pfad/Header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<LocalizedText>>,
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
    /// I-SCLM Cockpit UX R2 (Punkt 1): `false` = Add-Formular — der Renderer
    /// darf sichtbare Eingabefelder NICHT aus dem Flow-Zustand vorbelegen
    /// (Plumbing-Keys wie sid/code werden weiter still geseedet). Default
    /// `true` (fehlend) = Bearbeiten-Formular wie bisher vorbefüllt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefill: Option<bool>,
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

/// „Meine Verlosungen“-Scan (SCLM v2 Karte B / Spec 5.3): generische,
/// Manifest-deklarierte Sessionsuche — der Core kennt KEIN SCLM, alle
/// Endpunkte/Feldnamen/Match-Pfade kommen aus dem Manifest:
/// 1. `list_endpoint` lesen (neueste zuerst erwartet),
/// 2. Kandidaten = alle offen (`done_field` falsch) + die ersten `done_cap`
///    erledigten,
/// 3. pro Kandidat `detail_endpoint` mit `{code}` (= Wert von `code_field`),
///    darin `match_path`-Namen ci gegen den gespeicherten Wert von
///    `match_setting_key` (settings_fields) vergleichen.
/// 4. Ersteller-Vereinigung: Keystore-Sessions mit gespeichertem `admin_code`
///    werden zusätzlich geladen — über `creator_detail_endpoint` wenn gesetzt,
///    sonst über `detail_endpoint` (gleicher `{code}`-Platzhalter, Wert =
///    gespeicherter Admin-Code).
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScanSpec {
    /// Eindeutige Scan-ID (ASCII [A-Za-z0-9_-]).
    pub id: String,
    pub title: LocalizedText,
    /// Listen-Endpunkt (relativ zu base_url, ohne `{param}`-Platzhalter).
    pub list_endpoint: String,
    /// Feldname im Listen-Item: „bereits erledigt?“ (bool/0/1).
    pub done_field: String,
    /// Feldname im Listen-Item: Wert für den `{code}`-Platzhalter des
    /// Detail-Endpunkts (z. B. `join_code`).
    pub code_field: String,
    /// Feldname im Listen-Item: Session-Kennung (Keystore-Identität `sid`).
    pub id_field: String,
    /// Feldname im Listen-Item: Eventname für die Anzeige.
    pub name_field: String,
    /// Detail-Endpunkt mit genau einem `{code}`-Platzhalter.
    pub detail_endpoint: String,
    /// Optional: eigener Detail-Endpunkt NUR für die Ersteller-Vereinigung
    /// (Sessions, die über einen gespeicherten `admin_code` bekannt sind).
    /// Fehlt das Feld, nutzt die Vereinigung `detail_endpoint` — alte
    /// Manifeste verhalten sich also unverändert.
    ///
    /// Warum es das gibt: `code_field` füllt `{code}` in BOTHen Pfaden —
    /// Kandidaten bekommen den Listenwert (z. B. `id`/sid, wenn der Server
    /// keine Codes mehr ausliefert), die Vereinigung immer den Keystore-
    /// Admin-Code. Endpunkt + Token passen aber nicht mehr zusammen, sobald
    /// der Server sid- und Code-Lesung trennt (SCLM 4A: sid ->
    /// `/api/public/session/<sid>`, Codes -> `/api/session/<code>`). Mit diesem
    /// Feld bleiben beide Arme korrekt adressiert, ohne dass der Kandidatenarm
    /// wieder einen öffentlich ausgelieferten Join-Code braucht.
    #[serde(default)]
    pub creator_detail_endpoint: Option<String>,
    /// Dot-Path in der Detail-Antwort, der die Teilnehmernamen liefert
    /// (z. B. `participants[].name`).
    pub match_path: String,
    /// settings_fields-Key, dessen gespeicherter Wert ci verglichen wird.
    pub match_setting_key: String,
    /// optionale Header-Templates fürs Detail (`{code}` erlaubt).
    pub headers: Option<BTreeMap<String, String>>,
    /// Obergrenze für Detail-Fetches erledigter Sessions (Spec: letzte 20).
    #[serde(default = "default_done_cap")]
    pub done_cap: usize,
    /// max. parallele Detail-Fetches.
    #[serde(default = "default_scan_concurrency")]
    pub concurrency: usize,
    /// Poll-Intervall (Sekunden). Default 300 (gleiche Regel wie Views).
    #[serde(default)]
    pub poll_seconds: Option<u64>,
}

fn default_done_cap() -> usize {
    20
}

fn default_scan_concurrency() -> usize {
    6
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
    /// Sessionscan „Meine …“-Landliste (Karte B / Spec 5.3). Optional —
    /// Manifeste ohne Scan verhalten sich wie bisher.
    #[serde(default)]
    pub scan: Option<ScanSpec>,
    /// Ersteller-Cockpit v1 (Spec 15): POST-Endpunkt für „Session erstellen“
    /// (z. B. `/api/session/create`). Optional — Manifeste ohne Feld haben
    /// keinen Create-Button (Command liefert error.connector.creator_unsupported).
    /// Kein SCLM-Hardcoding im Core: der Pfad kommt aus dem Manifest.
    #[serde(default)]
    pub create_endpoint: Option<String>,
    pub sync: Option<Sync>,
}

impl ConnectorManifest {
    /// Deserialisiert aus Bytes (mit Größenlimit-Prüfung seitens Validator).
    pub fn parse(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

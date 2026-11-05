//! Screen-Flow-Schema + Flow-Engine (SF1, t_c35bc233).
//!
//! Deklarative Screen-Ketten als Erweiterung des Connector-Manifests:
//! benannte `flows` mit typisierten Screens (`code_entry | list | detail |
//! form | wait | results`), display/exit-Bedingungen, Transitions und
//! Autocomplete-Regeln. SCLM-spezifisches lebt NUR im Manifest (Plugin-Paket);
//! diese Engine ist generisch und kennt keine Connector-IDs.
//!
//! Bedingungen sind Pfad-basiert (`on` = Dot-Path in den gemappten
//! Zustand), Operanden: equals/not_equals/exists/missing — bewusst KEINE
//! Freitext-Logik (Manifest bleibt code-frei, Deny-Unknown schützt).

use crate::manifest::{FormView, HttpMethod, LocalizedText, Param};
use crate::mapping::dot_path;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// Screen-Typen eines Flows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenType {
    /// Suchleisten-Code-Eingabe → Session öffnen (SF2 placement).
    CodeEntry,
    /// Liste (Teilnehmer, Items, …) mit Auswahl.
    List,
    /// Detailansicht (Session, Ergebnis …).
    Detail,
    /// Formular (Wünsche, Ersteller …).
    Form,
    /// Warteseite: poll_seconds + exit-Bedingung (`raffle_done → results`).
    Wait,
    /// Ergebnis-Screen (eigene Gewinne / Ergebnisliste).
    Results,
}

/// Bedingungs-Operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CondOp {
    Equals,
    NotEquals,
    Exists,
    Missing,
}

/// Deklarative Bedingung: `{"on": "raffle_done", "op": "equals", "value": true,
/// "then": "results", "otherwise": "wait"}` — `then`/`otherwise` sind
/// Screen-IDs (nur bei Branch-Bedingungen gepflegt; reine Display-Regeln
/// lassen sie weg).
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    /// Dot-Path in den gemappten Zustand.
    pub on: String,
    pub op: CondOp,
    /// Pflicht bei equals/not_equals.
    pub value: Option<Value>,
    /// Screen-ID, wenn die Bedingung wahr ist.
    pub then: Option<String>,
    /// Screen-ID, wenn die Bedingung falsch ist.
    pub otherwise: Option<String>,
}

/// Autocomplete-Regel: Feld durchsucht einen Endpoint; der Item-Treffer
/// füllt weitere Formular-Felder automatisch (überschreibbar).
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct AutocompleteRule {
    /// Formular-Feld, das den Such-Input trägt.
    pub field: String,
    /// Such-Endpoint (relativ zur base_url).
    pub endpoint: String,
    /// Query-String-Parameter für den Suchtext (Default `q`).
    pub query_param: Option<String>,
    /// Endpoint-Platzhalter `{param}` → deklarierte Param-Keys.
    pub params: Option<Vec<Param>>,
    /// Request-Header-Templates (`{param}`) — z. B. Session-Code.
    pub headers: Option<BTreeMap<String, String>>,
    /// Auto-Fill-Mapping: Formular-Feld → Dot-Path im Treffer-Item.
    pub fill: BTreeMap<String, String>,
}

/// Ein permanenter Sub-Reiter eines Cockpit-Flows (Ersteller-Cockpit v1,
/// Spec 15): `flows[].tabs` = 1–6 Einträge `{id, screen, label{de,en}}`.
/// Hat ein Flow `tabs`, rendert der FlowRenderer die Sub-Reiter-Leiste
/// permanent; initiale Tab = erster Eintrag; Tab-Wechsel = Screen frisch
/// fetchen (Fresh-Fetch-Regel pro Tab-Betreten). Flow ohne `tabs` =
/// bisheriges Wizard-Verhalten unverändert.
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct FlowTab {
    pub id: String,
    /// Ziel-Screen dieses Tabs (muss in `screens` existieren).
    pub screen: String,
    pub label: LocalizedText,
}

/// Code-Einstieg eines Flows (SCLM v2 / Spec 5.4): der Client übergibt einen
/// Session-Code (Suchzeile oder Öffnen-Button), der Core öffnet die Session,
/// joined den Teilnehmer automatisch (Auto-Join/Claim) und liefert den
/// Start-Screen. Alles SCLM-spezifische bleibt im Manifest — dieser Block
/// ist generisch Wohlgeformtheit-geprüft, der Kern kennt keine Connector-IDs.
///
/// Zwei Einstiegs-Semantiken (Ersteller-Cockpit v1):
/// - Teilnehmer-Einstieg: die Match-/Join-Felder (`match_path`, `id_path`,
///   `match_setting_key`, `join_endpoint`) sind ALLE gesetzt — Auto-Join/Claim
///   läuft wie bisher, der eingegebene Code pinnt als join_code.
/// - Admin-Einstieg: die Join-Felder sind ALLE fehlend — der eingegebene Code
///   ist der Ersteller-Code und pinnt als admin_code; Auto-Join entfällt.
///   `admin_path` ist Pflicht (Guardrail: die Session-Antwort muss den
///   Admin-Flag tragen, sonst ist der Code kein Ersteller-Code).
/// - Admin-Route (participant flow): `admin_path` + `admin_flow` gekoppelt —
///   trägt die Session am `admin_path`-Flag wahr, öffnet der Core den
///   Ziel-Flow (Erstellt-Cockpit) statt des Teilnehmer-Flows.
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct FlowOpen {
    /// Param-Key, unter dem der eingegebene Code in den Flow-Zustand pinnt
    /// (Default `code`).
    #[serde(default = "default_open_param")]
    pub param: String,
    /// GET-Endpunkt mit genau einem `{param}`-Platzhalter (Session laden).
    pub session_endpoint: String,
    /// Header-Templates fürs Session-GET (`{param}` erlaubt).
    pub headers: Option<BTreeMap<String, String>>,
    /// Dot-Paths in der Session-Antwort → Flow-Zustand (muss `sid` enthalten
    /// — `join_endpoint` braucht ihn; weitere üblich: name, max_wishes,
    /// raffle_done, raffle_mode).
    pub mapping: BTreeMap<String, String>,
    /// Dot-Path in der Session-Antwort, der Teilnehmernamen liefert
    /// (ci-EXAKTER Vergleich gegen `match_setting_key`). Nur im
    /// Teilnehmer-Einstieg gesetzt (alle Join-Felder oder keines — Validator).
    pub match_path: Option<String>,
    /// Dot-Path mit den zugehörigen Teilnehmer-IDs (indexparallel zu
    /// `match_path`).
    pub id_path: Option<String>,
    /// settings_fields-Key mit dem gespeicherten Benutzernamen.
    pub match_setting_key: Option<String>,
    /// POST-Endpunkt der Selbst-Anlage (`{sid}`-Platzhalter Pflicht).
    pub join_endpoint: Option<String>,
    /// Dot-Path in der Session-Antwort auf den Admin-Flag (z. B. `is_admin`).
    /// Am Teilnehmer-Einstieg + `admin_flow` gekoppelt: Flag wahr → Core
    /// öffnet `admin_flow` statt des Teilnehmer-Flows. Am Admin-Einstieg
    /// (ohne Join-Felder) Pflicht und wahr-Pflicht: fehlender Flag-Wert im
    /// GET bedeutet „kein Ersteller-Code“ → Fehler, kein Keystore-Schaden.
    pub admin_path: Option<String>,
    /// Flow-ID, die bei gesetztem `admin_path`-Flag geöffnet wird
    /// (Ersteller-Cockpit). Muss existieren, einen `open`-Block mit
    /// Admin-Semantik tragen und darf nicht der eigene Flow sein.
    pub admin_flow: Option<String>,
    /// Body der Selbst-Anlage; `{param}` und Settings-Keys werden gefüllt
    /// (Default `{"name": "{match_setting_key}"}` — POST nur, wenn der Name
    /// NICHT bereits in der Teilnehmerliste steht).
    pub join_body: Option<Value>,
    /// Header-Templates fürs Join-POST (Session-Code als Header; pcode hat
    /// hier nichts verloren — pcode ist ausschließlich Body, Spec Abschnitt 3).
    pub join_headers: Option<BTreeMap<String, String>>,
    /// Start-Screen, solange die Ziehung offen ist (Normalpfad).
    pub open_screen: String,
    /// Start-Screen, wenn `raffle_done` beim Öffnen schon true ist — der
    /// Client landet direkt im Ergebnis.
    pub done_screen: String,
    /// Dot-Path im Session-Objekt auf das Done-Flag (Default `raffle_done`).
    #[serde(default = "default_done_path")]
    pub done_path: String,
    /// Resume-Regel (D-A Feedback 2): Dot-Pfad RELATIV zum gematchten
    /// Teilnehmer-Objekt (z. B. `wishes`). „Gesetzt“ = nicht-leeres Array
    /// oder bool true — der Nutzer hat den Flow bereits begonnen.
    pub progress_path: Option<String>,
    /// Ziel-Screen bei gesetzter `progress_path` (z. B. `wait`). Fehlend →
    /// normaler `open_screen`. `done_path`/`done_screen` haben VORRANG
    /// ( Gezogene landet immer im Ergebnis).
    pub progress_screen: Option<String>,
    /// Selbst-Wünsche-Injektion (D-A Feedback 8): der Core löst die
    /// wishes-IDs des gematchten Teilnehmers gegen die Session-Collection
    /// auf und injiziert die Item-Namen als `state["wished"]` in den
    /// Flow-Zustand (wait-Screen zeigt sie ohne Extra-Request).
    pub self_wishes: Option<SelfWishes>,
}

/// Konfiguration der Selbst-Wünsche-Auflösung (D-A). Alle Pfade sind
/// optional und defaults-getreu — `"self_wishes": {}` reicht als Schalter.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SelfWishes {
    /// Dot-Pfad RELATIV zum gematchten Teilnehmer-Objekt mit den loot-IDs.
    #[serde(default = "default_sw_wishes_path")]
    pub wishes_path: String,
    /// Dot-Pfad in der Session-Antwort auf die Item-Liste (z. B. `loot`).
    #[serde(default = "default_sw_collection")]
    pub collection: String,
    /// Dot-Pfad RELATIV zu einem Collection-Item mit der Item-ID.
    #[serde(default = "default_sw_id_path")]
    pub id_path: String,
    /// Dot-Pfad RELATIV zu einem Collection-Item mit dem Item-Namen.
    #[serde(default = "default_sw_name_path")]
    pub name_path: String,
}

impl Default for SelfWishes {
    fn default() -> Self {
        Self {
            wishes_path: default_sw_wishes_path(),
            collection: default_sw_collection(),
            id_path: default_sw_id_path(),
            name_path: default_sw_name_path(),
        }
    }
}

fn default_open_param() -> String {
    "code".to_owned()
}

fn default_done_path() -> String {
    "raffle_done".to_owned()
}

fn default_sw_wishes_path() -> String {
    "wishes".to_owned()
}

fn default_sw_collection() -> String {
    "loot".to_owned()
}

fn default_sw_id_path() -> String {
    "id".to_owned()
}

fn default_sw_name_path() -> String {
    "name".to_owned()
}

/// Multi-Select-Liste (SF2): Zeilen sind markierbar (Checkboxen), der
/// Confirm führt die Screen-Action aus. `max_path` ist ein Dot-Path in den
/// Flow-Zustand mit dem Maximum (z. B. `max_wishes`) — fehlt er, gilt
/// unbegrenzt.
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct MultiSpec {
    /// Dot-Path in den Flow-Zustand mit dem Maximum.
    pub max_path: Option<String>,
    /// I-SCLM Cockpit UX R2 (Punkt 4): Modal-Modus eines multi-Ziels, wenn
    /// es als Zeilen-Modal geöffnet wird — `wishes` (Loot für einen
    /// Teilnehmer auswählen) oder `assign` (Teilnehmer für einen Gegenstand
    /// auswählen, Eignung über freie Wunsch-Slots). Fehlt der Modus, gilt
    /// `wishes` (Bestandsmanifeste bleiben byte-identisch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<MultiMode>,
}

/// Modal-Auswahlrichtung eines multi-Screens (Siehe `MultiSpec::mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MultiMode {
    Wishes,
    Assign,
}

/// Generische Screen-Aktion (SF2): Button unter Detail-/Listen-/Results-
/// Screens. Entweder Write (`method` + `path`) oder reine Navigation
/// (`goto`). SCLM-spezifisches bleibt im Manifest.
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScreenAction {
    pub id: String,
    pub title: LocalizedText,
    /// HTTP-Methode; fehlt sie, ist die Action reine Navigation (`goto`).
    pub method: Option<HttpMethod>,
    /// Pfad mit `{param}`-Platzhaltern (Pflicht bei `method`).
    pub path: Option<String>,
    /// Deklarierte Platzhalter-Keys (für die Validator-Prüfung).
    pub params: Option<Vec<Param>>,
    /// Request-Header-Templates (`{param}`) — z. B. Session-Code.
    pub headers: Option<BTreeMap<String, String>>,
    /// Body-Vorlage; `{param}`-Platzhalter werden JSON-bewusst ersetzt.
    pub body_template: Option<Value>,
    /// Antwort → Flow-Zustand (`"pcode": "pcode"` pinnt das Antwortfeld
    /// `pcode` unter dem Zustands-Key `pcode`) — z. B. pcode-Claim.
    pub pins_map: Option<BTreeMap<String, String>>,
    /// Eingabefelder der Action (SF2-R1 D3, generisch): z. B. pcode-Claim
    /// beim ersten Wishes-Save — der Nutzer tippt seinen Claim-Code, der
    /// Wert fließt als `{key}`-Binding in Body/Path/Headers. Bereits
    /// gepinnte Werte werden vorgeseedet (User-Input gewinnt).
    /// Ersteller-Cockpit v1 (Karte F, Spec 15): ein Feld kann `type`
    /// (`text`|`number`|`select`, Default text) und bei select `options`
    /// (de/en, nicht leer) tragen — der Renderer zeichnet daraus ein
    /// Auswahlfeld (z. B. max_wishes 1–6 auf c_overview, PATCH settings
    /// sofort). Fehlende `type` = Textfeld wie bisher.
    pub fields: Option<Vec<Param>>,
    /// Ein-Regel (SCLM v2 Requirement 9/one-way-Flags): Action nur aktiv,
    /// wenn die Bedingung gegen den Flow-Zustand wahr ist (z. B.
    /// `enabled_if: {on: "collected", op: "not_equals", value: true}` →
    /// „Als abgeholt markieren“ ist nach dem Setzen deaktiviert). Fehlende
    /// Felder gelten bei not_equals als wahr (Flag nie gesetzt = actionell).
    pub enabled_if: Option<Condition>,
    /// Ziel-Screen nach der Action (ohne `method`: reine Navigation).
    pub goto: Option<String>,
}

impl ScreenAction {
    pub fn is_navigation(&self) -> bool {
        self.method.is_none()
    }
}

/// Ein Screen im Flow.
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Screen {
    pub id: String,
    #[serde(rename = "type")]
    pub screen_type: ScreenType,
    pub title: LocalizedText,
    /// API-Endpunkt (relativ zur base_url). Nicht bei `form` (→ form.path).
    pub endpoint: Option<String>,
    /// Feld → Dot-Path in der Antwort (`"name": "name"`).
    #[serde(default)]
    pub mapping: BTreeMap<String, String>,
    /// Endpoint-`{param}`-Platzhalter.
    pub params: Option<Vec<Param>>,
    /// Request-Header-Templates (`{param}`) — Session-Codes.
    pub headers: Option<BTreeMap<String, String>>,
    /// Poll-Intervall (Sekunden) — Pflicht/Sinn nur bei `wait`.
    pub poll_seconds: Option<u64>,
    /// Nur bei `type: form`.
    pub form: Option<FormView>,
    /// Keys, die aus dieser Screen-Eingabe in den Flow-Zustand gepinnt
    /// werden (pro Session — z. B. `code`, `pcode`). NICHT global.
    pub pins: Option<Vec<String>>,
    /// Display-Regel: Screen nur zeigen, wenn Bedingung wahr; sonst
    /// (optional) `otherwise`-Screen.
    pub when: Option<Condition>,
    /// Transition nach Formular/Code-Submit → nächster Screen.
    pub on_submit: Option<String>,
    /// Transition nach Listenauswahl → nächster Screen.
    pub on_select: Option<String>,
    /// Wait-Exit: Bedingung im Poll → `then`-Screen (z. B. `raffle_done`).
    pub exit: Option<Condition>,
    /// Autocomplete-Regeln (Item-Suche + Auto-Fill).
    pub autocomplete: Option<Vec<AutocompleteRule>>,
    /// Multi-Select (nur `list`): Zeilen markierbar, Confirm → Action.
    pub multi: Option<MultiSpec>,
    /// Generische Aktionen (Buttons) unter dem Screen-Inhalt.
    pub actions: Option<Vec<ScreenAction>>,
    /// Screen-Antwort → Session-Keystore (Spec 5.2): `{"result_id": "rid"}`
    /// = Keystore-Spalte `result_id` aus Flow-Zustands-Key `rid`. Der Core
    /// schreibt die Spalte, sobald der Screen einen eindeutigen Treffer liefert
    /// (first-write wins, partielles Upsert wie bei Karte A).
    pub keystore_fields: Option<BTreeMap<String, String>>,
}

/// Benannter Flow: Einstieg + Kette von Screens.
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Flow {
    pub id: String,
    pub title: LocalizedText,
    /// Entry-Screen-ID (muss in `screens` existieren).
    pub entry: String,
    /// Permanente Sub-Reiter (Ersteller-Cockpit v1, Spec 15): 1–6
    /// `{id, screen, label{de,en}}`. `None` = Wizard-Verhalten wie bisher.
    /// Tab-IDs/Reihenfolge sind Manifest-Sache; der Kern prüft nur
    /// Wohlgeformtheit (eindeutige IDs, bekannte Screens).
    #[serde(default)]
    pub tabs: Option<Vec<FlowTab>>,
    pub screens: Vec<Screen>,
    /// Code-Einstieg (SCLM v2 / Spec 5.4): Session-Code → Auto-Join →
    /// Start-Screen. Optional: Flows ohne `open` haben manuellen Einstieg
    /// (Rückwärtskompatibilität).
    pub open: Option<FlowOpen>,
}

impl Flow {
    pub fn screen(&self, id: &str) -> Option<&Screen> {
        self.screens.iter().find(|s| s.id == id)
    }
    pub fn entry_screen(&self) -> Option<&Screen> {
        self.screen(&self.entry)
    }
}

/// Bedingung gegen den Zustand auswerten. `state` = gemapptes Objekt
/// (letzte Fetch-Antwort + gepinnte Werte). Fehlende Pfade = falsch
/// (tolerant wie Dot-Path-Mapping — fehlendes Feld ist kein Runtime-Fehler).
pub fn evaluate(condition: &Condition, state: &Value) -> bool {
    let hits = match dot_path(state, &condition.on) {
        Ok(h) => h,
        Err(_) => return false,
    };
    let first: Option<&Value> = hits.first().copied();
    let first_is_null = matches!(first, Some(&Value::Null));
    match condition.op {
        CondOp::Exists => first.is_some() && !first_is_null,
        CondOp::Missing => first.is_none() || first_is_null,
        CondOp::Equals | CondOp::NotEquals => {
            let Some(value) = &condition.value else {
                return false;
            };
            // Bool-Toleranz (SF2-Befund Live-Probe): SQLite-Backends liefern
            // Bools als 0/1-Ints — ein Boolean-Vergleichswert matcht deshalb
            // auch die Zahl-Äquivalente (1==true, 0==false). Sonst feuert
            // `raffle_done equals true` nie gegen `raffle_done: 1`.
            let eq = match (value.as_bool(), first) {
                (Some(b), Some(Value::Number(n))) => {
                    n.as_f64().map(|f| (f != 0.0) == b).unwrap_or(false)
                }
                _ => first == Some(value),
            };
            if condition.op == CondOp::Equals {
                eq
            } else {
                !eq
            }
        }
    }
}

/// Transition-Auflösung für eine Bedingung mit Branch (`then`/`otherwise`):
/// liefert die Ziel-Screen-ID gemäß Auswertung.
pub fn branch_target(condition: &Condition, state: &Value) -> Option<String> {
    if evaluate(condition, state) {
        condition.then.clone()
    } else {
        condition.otherwise.clone()
    }
}

/// Wait-Exit: Bedingung im Poll erfüllt → Ziel-Screen.
pub fn wait_exit(screen: &Screen, state: &Value) -> Option<String> {
    screen
        .exit
        .as_ref()
        .filter(|c| evaluate(c, state))
        .and_then(|c| c.then.clone())
}

/// Auto-Fill-Mapping: Item-Treffer → Formular-Feldwerte. Erster Treffer je
/// Dot-Path; fehlende Pfade füllen das Feld NICHT (Nutzer-Eingabe bleibt).
pub fn autofill(
    item: &Value,
    rule: &AutocompleteRule,
) -> Result<BTreeMap<String, Value>, crate::mapping::DotPathError> {
    let mut out = BTreeMap::new();
    for (field, path) in &rule.fill {
        if let Some(hit) = dot_path(item, path)?.first().filter(|hit| !hit.is_null()) {
            out.insert(field.clone(), (*hit).clone());
        }
    }
    Ok(out)
}

/// `{param}`-Platzhalter in einem Template/String auflösen — Bindings
/// stammen aus dem Flow-Zustand (gepinnte Session-Codes + Formular-Werte).
/// Unbekannte Platzhalter bleiben UNBERÜHRT (kein Still-Schweigen).
pub fn resolve_placeholders(input: &str, bindings: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) => {
                let key = &after[..end];
                match bindings.get(key) {
                    Some(v) => out.push_str(v),
                    None => {
                        out.push('{');
                        out.push_str(key);
                        out.push('}');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                return out;
            }
        }
    }
    out + rest
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn cond(on: &str, op: CondOp, value: Option<Value>) -> Condition {
        Condition {
            on: on.into(),
            op,
            value,
            then: None,
            otherwise: None,
        }
    }

    #[test]
    fn equals_bool_tolerates_sqlite_ints() {
        // SF2-Befund Live-Probe: get_session liefert raffle_done als 0/1-Int
        // (SQLite-Row) — `equals true` muss trotzdem feuern.
        let c = Condition {
            on: "raffle_done".into(),
            op: CondOp::Equals,
            value: Some(json!(true)),
            then: Some("results".into()),
            otherwise: Some("wait".into()),
        };
        assert_eq!(
            branch_target(&c, &json!({"raffle_done": 1})).as_deref(),
            Some("results")
        );
        assert_eq!(
            branch_target(&c, &json!({"raffle_done": 0})).as_deref(),
            Some("wait")
        );
        assert_eq!(
            branch_target(&c, &json!({"raffle_done": true})).as_deref(),
            Some("results")
        );
        assert_eq!(
            branch_target(&c, &json!({"raffle_done": false})).as_deref(),
            Some("wait")
        );
        // Nicht-Bool-Vergleichswerte bleiben strikt (keine Truthiness-Magie).
        let n = Condition {
            on: "count".into(),
            op: CondOp::Equals,
            value: Some(json!(1)),
            then: None,
            otherwise: None,
        };
        assert!(evaluate(&n, &json!({"count": 1})));
        assert!(!evaluate(&n, &json!({"count": true})));
    }

    #[test]
    fn equals_not_equals_exists_missing() {
        let state = json!({"raffle_done": true, "name": "x"});
        assert!(evaluate(
            &cond("raffle_done", CondOp::Equals, Some(json!(true))),
            &state
        ));
        assert!(!evaluate(
            &cond("raffle_done", CondOp::NotEquals, Some(json!(true))),
            &state
        ));
        assert!(evaluate(&cond("name", CondOp::Exists, None), &state));
        assert!(!evaluate(&cond("absent", CondOp::Exists, None), &state));
        assert!(evaluate(&cond("absent", CondOp::Missing, None), &state));
        // Tolerant: kein Fehler bei kaputtem Pfad — einfach falsch.
        assert!(!evaluate(&cond("x[", CondOp::Exists, None), &state));
    }

    #[test]
    fn branch_targets_pick_then_or_otherwise() {
        let state = json!({"raffle_done": true});
        let c = Condition {
            on: "raffle_done".into(),
            op: CondOp::Equals,
            value: Some(json!(true)),
            then: Some("results".into()),
            otherwise: Some("wait".into()),
        };
        assert_eq!(branch_target(&c, &state).as_deref(), Some("results"));
        assert_eq!(
            branch_target(&c, &json!({"raffle_done": false})).as_deref(),
            Some("wait")
        );
        let c2 = Condition {
            on: "raffle_done".into(),
            op: CondOp::Equals,
            value: Some(json!(true)),
            then: Some("results".into()),
            otherwise: None,
        };
        assert_eq!(branch_target(&c2, &json!({})), None);
    }

    fn screen(id: &str, t: ScreenType) -> Screen {
        Screen {
            id: id.into(),
            screen_type: t,
            title: LocalizedText {
                de: id.into(),
                en: id.into(),
            },
            endpoint: None,
            mapping: BTreeMap::new(),
            params: None,
            headers: None,
            poll_seconds: None,
            form: None,
            pins: None,
            when: None,
            on_submit: None,
            on_select: None,
            exit: None,
            autocomplete: None,
            multi: None,
            actions: None,
            keystore_fields: None,
        }
    }

    #[test]
    fn wait_exit_fires_only_when_condition_true() {
        let mut wait = screen("wait", ScreenType::Wait);
        wait.exit = Some(Condition {
            on: "raffle_done".into(),
            op: CondOp::Equals,
            value: Some(json!(true)),
            then: Some("results".into()),
            otherwise: None,
        });
        assert_eq!(wait_exit(&wait, &json!({"raffle_done": false})), None);
        assert_eq!(
            wait_exit(&wait, &json!({"raffle_done": true})).as_deref(),
            Some("results")
        );
        // ohne exit: nie
        let plain = screen("wait2", ScreenType::Wait);
        assert_eq!(wait_exit(&plain, &json!({"raffle_done": true})), None);
    }

    #[test]
    fn flow_lookup_entry_and_screens() {
        let mut s = screen("start", ScreenType::CodeEntry);
        s.on_submit = Some("list".into());
        let f = Flow {
            id: "participant".into(),
            title: LocalizedText {
                de: "Teilnehmer".into(),
                en: "Participant".into(),
            },
            entry: "start".into(),
            tabs: None,
            screens: vec![s, screen("list", ScreenType::List)],
            open: None,
        };
        assert_eq!(f.entry_screen().map(|x| x.id.as_str()), Some("start"));
        assert_eq!(
            f.screen("list").map(|x| x.screen_type),
            Some(ScreenType::List)
        );
        assert!(f.screen("nope").is_none());
    }

    #[test]
    fn autofill_maps_first_hits_and_skips_missing() {
        let rule = AutocompleteRule {
            field: "name".into(),
            endpoint: "/api/items/search".into(),
            query_param: Some("q".into()),
            params: None,
            headers: None,
            fill: [
                ("size".into(), "size".into()),
                ("grade".into(), "grade".into()),
                ("class".into(), "cls".into()),
                ("never".into(), "absent.path".into()),
            ]
            .into_iter()
            .collect(),
        };
        let item = json!({"name": "Hammer", "size": "S3", "grade": "C", "cls": null});
        let filled = autofill(&item, &rule).unwrap();
        assert_eq!(filled.get("size"), Some(&json!("S3")));
        assert_eq!(filled.get("grade"), Some(&json!("C")));
        // null-Felder füllen NICHT (klarer als stillschweigende Überschreibung)
        assert!(!filled.contains_key("class"));
        // fehlender Pfad: Feld bleibt unberührt
        assert!(!filled.contains_key("never"));
    }

    #[test]
    fn autofill_bad_path_errors() {
        let rule = AutocompleteRule {
            field: "name".into(),
            endpoint: "/api/items/search".into(),
            query_param: None,
            params: None,
            headers: None,
            fill: [("x".into(), "bad[".into())].into_iter().collect(),
        };
        assert!(autofill(&json!({}), &rule).is_err());
    }

    /// SF3: Leerer Fill-Pfad = „Treffer selbst" — String-Listen-Endpoints
    /// (SCLM Teilnehmer-/Waren-Suche) füllen so das Eingabefeld direkt.
    #[test]
    fn autofill_empty_path_fills_item_itself() {
        let rule = AutocompleteRule {
            field: "name".into(),
            endpoint: "/api/commodities/search".into(),
            query_param: Some("q".into()),
            params: None,
            headers: None,
            fill: [("name".into(), "".into())].into_iter().collect(),
        };
        let filled = autofill(&json!("Titanium"), &rule).unwrap();
        assert_eq!(filled.get("name"), Some(&json!("Titanium")));
        // null/leer füllt nicht
        assert!(autofill(&json!(null), &rule).unwrap().is_empty());
    }

    #[test]
    fn placeholders_resolve_from_bindings_only() {
        let b: BTreeMap<String, String> = [("code".into(), "ABC-1".into())].into_iter().collect();
        assert_eq!(
            resolve_placeholders("/api/session/{code}", &b),
            "/api/session/ABC-1"
        );
        // unbekannt bleibt stehen (kein Still-Schweigen)
        assert_eq!(resolve_placeholders("/api/{kode}", &b), "/api/{kode}");
        assert_eq!(resolve_placeholders("/api/plain", &b), "/api/plain");
        // geschweifte Klammer ohne Ende: unverändert
        assert_eq!(resolve_placeholders("/api/{open", &b), "/api/{open");
    }

    #[test]
    fn screens_deserialize_strictly() {
        let s = screen("a", ScreenType::Form);
        let v = serde_json::to_value(&s).unwrap();
        // deny_unknown_fields: Tippfer muss scheitern
        let mut with_typo = v.clone();
        with_typo["typo"] = json!(1);
        let parsed: Result<Screen, _> = serde_json::from_value(with_typo);
        assert!(parsed.is_err());
        let parsed: Screen = serde_json::from_value(v).unwrap();
        assert_eq!(parsed.id, "a");
    }

    #[test]
    fn wait_screen_parses_from_json() {
        let text = r#"{
            "id": "wait", "type": "wait",
            "title": {"de": "Warten", "en": "Wait"},
            "poll_seconds": 15,
            "exit": {"on": "raffle_done", "op": "equals", "value": true, "then": "results"}
        }"#;
        let s: Screen = serde_json::from_str(text).unwrap();
        assert_eq!(s.screen_type, ScreenType::Wait);
        assert_eq!(s.poll_seconds, Some(15));
        assert_eq!(s.exit.as_ref().unwrap().then.as_deref(), Some("results"));
    }

    #[test]
    fn multi_and_actions_parse_from_json() {
        let text = r#"{
            "id": "wishes", "type": "list",
            "title": {"de": "Wünsche", "en": "Wishes"},
            "endpoint": "/api/session/{code}",
            "params": [{"key": "code", "label": {"de": "Code", "en": "Code"}}],
            "headers": {"X-SCLM-Code": "{code}"},
            "multi": {"max_path": "max_wishes"},
            "pins": ["loot_id"],
            "actions": [
                {
                    "id": "confirm",
                    "title": {"de": "Bestätigen", "en": "Confirm"},
                    "method": "post",
                    "path": "/api/session/{sid}/wishes",
                    "headers": {"X-SCLM-Code": "{code}"},
                    "body_template": {"loot_ids": "{loot_id}"},
                    "goto": "wait"
                },
                {
                    "id": "add_self",
                    "title": {"de": "Selbst hinzufügen", "en": "Add myself"},
                    "goto": "addme"
                }
            ]
        }"#;
        let s: Screen = serde_json::from_str(text).unwrap();
        assert_eq!(
            s.multi.as_ref().unwrap().max_path.as_deref(),
            Some("max_wishes")
        );
        let actions = s.actions.unwrap();
        assert_eq!(actions.len(), 2);
        assert!(!actions[0].is_navigation());
        assert!(actions[1].is_navigation());
        assert_eq!(actions[1].goto.as_deref(), Some("addme"));
        // deny_unknown_fields: Tippfer scheitert
        let bad: Result<Screen, _> =
            serde_json::from_str(text.replace("\"max_path\"", "\"max_pat\"").as_str());
        assert!(bad.is_err());
    }

    #[test]
    fn pins_map_parses_from_json() {
        let text = r#"{
            "id": "addme", "type": "form",
            "title": {"de": "Beitreten", "en": "Join"},
            "actions": [{
                "id": "noop", "title": {"de": "x", "en": "x"},
                "method": "post", "path": "/api/x",
                "pins_map": {"pcode": "pcode", "participant_id": "id"}
            }]
        }"#;
        let s: Screen = serde_json::from_str(text).unwrap();
        let actions = s.actions.unwrap();
        let pm = actions[0].pins_map.as_ref().unwrap();
        assert_eq!(pm.get("pcode"), Some(&"pcode".to_string()));
        assert_eq!(pm.get("participant_id"), Some(&"id".to_string()));
    }

    // ---- SCLM v2 (Spec 5.4): open-Block, enabled_if, keystore_fields -------

    #[test]
    fn flow_open_block_parses_with_defaults() {
        let text = r#"{
            "id": "participant", "title": {"de": "T", "en": "T"},
            "entry": "wishes",
            "open": {
                "session_endpoint": "/api/session/{code}",
                "headers": {"X-SCLM-Code": "{code}"},
                "mapping": {"sid": "id", "raffle_done": "raffle_done"},
                "match_path": "participants[].name",
                "id_path": "participants[].id",
                "match_setting_key": "sclm_username",
                "join_endpoint": "/api/session/{sid}/participants",
                "open_screen": "wishes",
                "done_screen": "results"
            },
            "screens": []
        }"#;
        let f: Flow = serde_json::from_str(text).unwrap();
        let o = f.open.expect("open block");
        assert_eq!(o.param, "code"); // Default
        assert_eq!(o.done_path, "raffle_done"); // Default
        assert!(o.join_body.is_none()); // Core füllt Default-Body
        // Tippfer im open-Block scheitert (deny_unknown_fields)
        let bad: Result<Flow, _> =
            serde_json::from_str(text.replace("open_screen", "open_scrn").as_str());
        assert!(bad.is_err());
        // Flow ohne open bleibt gültig (Rückwärtskompatibilität)
        let plain: Flow = serde_json::from_str(
            r#"{"id":"f","title":{"de":"T","en":"T"},"entry":"a","screens":[]}"#,
        )
        .unwrap();
        assert!(plain.open.is_none());
    }

    #[test]
    fn enabled_if_and_keystore_fields_parse() {
        let text = r#"{
            "id": "results", "type": "results",
            "title": {"de": "Mein Gewinn", "en": "My Win"},
            "endpoint": "/api/x",
            "keystore_fields": {"result_id": "rid"},
            "actions": [{
                "id": "collect", "title": {"de": "C", "en": "C"},
                "method": "patch", "path": "/api/r/{rid}",
                "enabled_if": {"on": "collected", "op": "not_equals", "value": true}
            }]
        }"#;
        let s: Screen = serde_json::from_str(text).unwrap();
        assert_eq!(
            s.keystore_fields.as_ref().unwrap().get("result_id"),
            Some(&"rid".to_string())
        );
        let a = &s.actions.unwrap()[0];
        let c = a.enabled_if.as_ref().unwrap();
        assert_eq!(c.on, "collected");
        assert_eq!(c.op, CondOp::NotEquals);
        assert_eq!(c.value, Some(json!(true)));
        // enabled_if ist Bedingung wie when/exit — deny_unknown_fields gilt
        let bad: Result<Screen, _> = serde_json::from_str(&text.replace("enabled_if", "enabledif"));
        assert!(bad.is_err());
    }

    // ---- Ersteller-Cockpit v1 (Spec 15): tabs + Admin-Einstieg -------------

    #[test]
    fn flow_tabs_round_trip_parses_and_serializes() {
        // Frontend-Kontrakt: flows[].tabs durchläuft den Rust-Parser und
        // wird identisch zurückserialisiert.
        let text = r#"{
            "id": "creator", "title": {"de": "E", "en": "C"},
            "entry": "c_overview",
            "tabs": [
                {"id": "overview", "screen": "c_overview", "label": {"de": "Übersicht", "en": "Overview"}},
                {"id": "raffle", "screen": "c_results", "label": {"de": "Verlosung", "en": "Raffle"}}
            ],
            "screens": []
        }"#;
        let f: Flow = serde_json::from_str(text).unwrap();
        let tabs = f.tabs.as_ref().expect("tabs");
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[0].id, "overview");
        assert_eq!(tabs[0].screen, "c_overview");
        assert_eq!(tabs[1].label.de, "Verlosung");
        let back = serde_json::to_value(&f).unwrap();
        assert_eq!(back["tabs"][0]["screen"], json!("c_overview"));
        assert_eq!(back["tabs"][1]["label"]["en"], json!("Raffle"));
        // Flow ohne tabs: Feld fehlt im JSON und wird toleriert (None)
        let plain: Flow = serde_json::from_str(
            r#"{"id":"f","title":{"de":"T","en":"T"},"entry":"a","screens":[]}"#,
        )
        .unwrap();
        assert!(plain.tabs.is_none());
        // deny_unknown_fields im Tab-Eintrag
        let bad: Result<Flow, _> =
            serde_json::from_str(text.replace("\"screen\"", "\"scrn\"").as_str());
        assert!(bad.is_err());
    }

    #[test]
    fn flow_action_field_select_type_and_options_round_trip() {
        // Frontend-Kontrakt (Karte F R2, Review-R1): Action-`fields` tragen
        // `type` (text|number|select) und bei select `options` (de/en) — der
        // Renderer braucht sie, um den max_wishes-Select 1–6 auf c_overview
        // generisch zu zeichnen (kein Hardcoding in Karte G).
        let text = r#"{
            "id": "a", "title": {"de": "A", "en": "A"}, "entry": "c_overview",
            "screens": [{
                "id": "c_overview", "type": "detail", "title": {"de": "Ü", "en": "O"},
                "actions": [{
                    "id": "max_wishes", "title": {"de": "M", "en": "M"},
                    "method": "patch", "path": "/api/session/{sid}/settings",
                    "params": [{"key": "sid", "label": {"de": "S", "en": "S"}}],
                    "body_template": {"max_wishes": "{max_wishes}"},
                    "fields": [
                        {"key": "max_wishes", "label": {"de": "Max", "en": "Max"},
                         "type": "select", "required": true,
                         "options": [{"de": "1", "en": "1"}, {"de": "6", "en": "6"}]},
                        {"key": "note", "label": {"de": "N", "en": "N"}, "type": "number"}
                    ]
                }]
            }]
        }"#;
        let f: Flow = serde_json::from_str(text).unwrap();
        let action = &f.screens[0].actions.as_ref().unwrap()[0];
        let fields = action.fields.as_ref().expect("fields");
        assert_eq!(
            fields[0].field_type,
            Some(crate::manifest::FieldType::Select)
        );
        assert_eq!(fields[0].required, Some(true));
        assert_eq!(fields[0].options.as_ref().expect("options").len(), 2);
        assert_eq!(fields[0].options.as_ref().unwrap()[1].en, "6");
        assert_eq!(
            fields[1].field_type,
            Some(crate::manifest::FieldType::Number)
        );
        assert!(fields[1].options.is_none());
        // Round-trip: Select und seine Optionen landen unverändert im DTO.
        let back = serde_json::to_value(&f).unwrap();
        let f0 = &back["screens"][0]["actions"][0]["fields"][0];
        assert_eq!(f0["type"], json!("select"));
        assert_eq!(f0["options"][0], json!({"de": "1", "en": "1"}));
        assert_eq!(
            back["screens"][0]["actions"][0]["fields"][1]["type"],
            json!("number")
        );
        // Rückwärtskompatibilität: ein Feld ohne type/options (alle bestehenden
        // Manifeste) parst UND serialisiert ohne die neuen Keys.
        let legacy: Screen = serde_json::from_str(
            r#"{"id":"s","type":"list","title":{"de":"l","en":"l"},"actions":[{"id":"x","title":{"de":"x","en":"x"},"method":"patch","path":"/api/x","fields":[{"key":"delta","label":{"de":"D","en":"D"}}]}]}"#,
        )
        .unwrap();
        let lf = &legacy.actions.as_ref().unwrap()[0].fields.as_ref().unwrap()[0];
        assert_eq!(lf.field_type, None);
        assert_eq!(lf.options, None);
        let lb = serde_json::to_value(&legacy).unwrap();
        let lfield = &lb["actions"][0]["fields"][0];
        assert!(!lfield.as_object().unwrap().contains_key("type"));
        assert!(!lfield.as_object().unwrap().contains_key("options"));
        // deny_unknown_fields: Tippfer "opcion" scheitert am Schema (die
        // select-ohne-options-Kombination prüft der Validator, nicht Serde).
        let bad: Result<Flow, _> =
            serde_json::from_str(text.replace("\"options\"", "\"opcion\"").as_str());
        assert!(bad.is_err());
    }

    #[test]
    fn flow_open_admin_entry_parses_without_join_fields() {
        // Admin-Einstieg: alle Join-Felder fehlend, dafür admin_path.
        let text = r#"{
            "id": "creator", "title": {"de": "E", "en": "C"}, "entry": "c_overview",
            "open": {
                "session_endpoint": "/api/session/{code}",
                "headers": {"X-SCLM-Code": "{code}"},
                "mapping": {"sid": "id", "admin_code": "admin_code"},
                "admin_path": "is_admin",
                "open_screen": "c_overview",
                "done_screen": "c_results"
            },
            "screens": []
        }"#;
        let f: Flow = serde_json::from_str(text).unwrap();
        let o = f.open.expect("admin open block");
        assert!(o.match_path.is_none());
        assert!(o.id_path.is_none());
        assert!(o.match_setting_key.is_none());
        assert!(o.join_endpoint.is_none());
        assert_eq!(o.admin_path.as_deref(), Some("is_admin"));
        assert!(o.admin_flow.is_none());
        assert_eq!(o.open_screen, "c_overview");
    }

    #[test]
    fn flow_open_admin_route_pair_parses_on_participant_flow() {
        let text = r#"{
            "id": "participant", "title": {"de": "T", "en": "P"}, "entry": "wishes",
            "open": {
                "session_endpoint": "/api/session/{code}",
                "mapping": {"sid": "id"},
                "match_path": "participants[].name",
                "id_path": "participants[].id",
                "match_setting_key": "sclm_username",
                "join_endpoint": "/api/session/{sid}/participants",
                "admin_path": "is_admin",
                "admin_flow": "creator",
                "open_screen": "wishes",
                "done_screen": "results"
            },
            "screens": []
        }"#;
        let f: Flow = serde_json::from_str(text).unwrap();
        let o = f.open.expect("participant open with admin route");
        assert_eq!(o.admin_path.as_deref(), Some("is_admin"));
        assert_eq!(o.admin_flow.as_deref(), Some("creator"));
        assert_eq!(o.match_path.as_deref(), Some("participants[].name"));
    }
}

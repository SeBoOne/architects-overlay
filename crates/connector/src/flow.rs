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

/// Multi-Select-Liste (SF2): Zeilen sind markierbar (Checkboxen), der
/// Confirm führt die Screen-Action aus. `max_path` ist ein Dot-Path in den
/// Flow-Zustand mit dem Maximum (z. B. `max_wishes`) — fehlt er, gilt
/// unbegrenzt.
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct MultiSpec {
    /// Dot-Path in den Flow-Zustand mit dem Maximum.
    pub max_path: Option<String>,
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
    pub fields: Option<Vec<Param>>,
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
}

/// Benannter Flow: Einstieg + Kette von Screens.
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Flow {
    pub id: String,
    pub title: LocalizedText,
    /// Entry-Screen-ID (muss in `screens` existieren).
    pub entry: String,
    pub screens: Vec<Screen>,
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
                (Some(b), Some(&Value::Number(ref n))) => {
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
            screens: vec![s, screen("list", ScreenType::List)],
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
}

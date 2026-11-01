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

use crate::manifest::{FormView, LocalizedText, Param};
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
            let eq = first == Some(value);
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
}

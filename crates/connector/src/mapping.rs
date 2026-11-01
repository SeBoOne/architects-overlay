//! Dot-Path-Mapping-Engine: `"name": "data[].ship_name"`.
//!
//! Pfad-Syntax:
//! - `key` — Objekt-Schlüssel
//! - `[]` — über ein Array iterieren (pro Element weiter)
//! - `[3]` — Array-Index
//!
//! `dot_path(json, "data[].ship_name")` liefert alle Treffer als `Vec`.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct DotPathError(pub String);

impl std::fmt::Display for DotPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Auswertet-Algorithmus: kompiliert den Pfad einmal und durchläuft
/// `Value`-Referenzen (Listen-Pfade erzeugen mehrere Treffer).
pub fn dot_path<'a>(value: &'a Value, path: &str) -> Result<Vec<&'a Value>, DotPathError> {
    let tokens = parse_path(path)?;
    let mut current = vec![value];
    for token in tokens {
        let mut next = Vec::new();
        for node in current {
            match token {
                Token::Key(ref key) => match node {
                    Value::Object(map) => {
                        if let Some(v) = map.get(key.as_str()) {
                            next.push(v);
                        }
                        // Fehlender Schlüssel = fehlendes Feld → leerer Treffer,
                        // kein Fehler (tolerante DTOs, Serde-Null-Regel).
                    }
                    Value::Null => {}
                    _ => return Err(DotPathError(format!("expected object at '{key}'"))),
                },
                Token::Iterate => match node {
                    Value::Array(items) => next.extend(items.iter()),
                    Value::Null => {}
                    _ => return Err(DotPathError("expected array at '[]'".into())),
                },
                Token::Index(i) => match node {
                    Value::Array(items) => {
                        if let Some(v) = items.get(i) {
                            next.push(v)
                        }
                    }
                    Value::Null => {}
                    _ => return Err(DotPathError(format!("expected array at '[{i}]'"))),
                },
            }
        }
        current = next;
    }
    Ok(current)
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Key(String),
    Iterate,
    Index(usize),
}

/// `data[].ship_name` → [Key(data), Iterate, Key(ship_name)]
fn parse_path(path: &str) -> Result<Vec<Token>, DotPathError> {
    let mut tokens = Vec::new();
    let mut chars = path.chars().peekable();
    let mut key = String::new();
    let flush = |key: &mut String, tokens: &mut Vec<Token>| {
        if !key.is_empty() {
            tokens.push(Token::Key(std::mem::take(key)));
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '.' => flush(&mut key, &mut tokens),
            '[' => {
                flush(&mut key, &mut tokens);
                let mut idx = String::new();
                loop {
                    match chars.next() {
                        Some(']') => break,
                        Some(c2) => idx.push(c2),
                        None => return Err(DotPathError("unterminated '['".into())),
                    }
                }
                if idx.is_empty() {
                    tokens.push(Token::Iterate);
                } else {
                    let i = idx
                        .parse::<usize>()
                        .map_err(|_| DotPathError(format!("bad index '{idx}'")))?;
                    tokens.push(Token::Index(i));
                }
            }
            c2 => key.push(c2),
        }
    }
    flush(&mut key, &mut tokens);
    if tokens.is_empty() {
        return Err(DotPathError("empty path".into()));
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn iterates_arrays() {
        let data = json!({"data": [
            {"ship_name": "Sabre"}, {"ship_name": "Kraken"}, {"other": 1}
        ]});
        let hits = dot_path(&data, "data[].ship_name").unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].as_str().unwrap(), "Sabre");
        assert_eq!(hits[1].as_str().unwrap(), "Kraken");
    }

    #[test]
    fn plain_key_and_index() {
        let data = json!({"data": [{"n": 1}, {"n": 2}, {"n": 3}]});
        assert_eq!(dot_path(&data, "data[1].n").unwrap()[0], &json!(2));
        assert_eq!(
            dot_path(&data, "data").unwrap()[0],
            data.get("data").unwrap()
        );
    }

    #[test]
    fn nested_iteration() {
        let data = json!({"sessions": [
            {"id": 1, "wishes": [{"item": "A"}, {"item": "B"}]},
            {"id": 2, "wishes": [{"item": "C"}]}
        ]});
        let hits = dot_path(&data, "sessions[].wishes[].item").unwrap();
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[2].as_str().unwrap(), "C");
    }

    #[test]
    fn missing_keys_are_tolerant() {
        let data = json!({"data": [{"n": 1}, {}]});
        let hits = dot_path(&data, "data[].n").unwrap();
        assert_eq!(hits.len(), 1);
        // null-Felder (Serde-Null-Regel) liefern null, kein Fehler
        let data2 = json!({"data": [{"n": null}]});
        assert_eq!(dot_path(&data2, "data[].n").unwrap()[0], &Value::Null);
    }

    #[test]
    fn wrong_shapes_error() {
        let data = json!({"data": 5});
        assert!(dot_path(&data, "data[]").is_err());
        assert!(dot_path(&data, "data[].x").is_err());
        assert!(dot_path(&data, "x[.y").is_err());
        assert!(dot_path(&data, "").is_err());
    }
}

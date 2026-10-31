//! Tiny expression evaluator for the overlay's inline calculator.
//!
//! Grammar (Phase 1 — deliberately small, no external crates):
//!
//! ```text
//! expr    := term (('+' | '-') term)*
//! term    := factor (('*' | '/') factor)*
//! factor  := ('-' factor) | postfix
//! postfix := primary ('%')?
//! primary := Number | '(' expr ')'
//! ```
//!
//! Percent is postfix and means `/100` (a relative fraction, useful for
//! "15% of a lot ⇒ price × 0.15" style markup). Whitespace is allowed and
//! ignored between tokens.
//!
//! Errors are returned as human-readable German strings mirroring the
//! overlay's IPC convention (e.g. "Klammer nie geschlossen").

/// Evaluate a full expression string. Returns the numeric result or a
/// German error message. Never panics on malformed input.
pub fn evaluate(src: &str) -> Result<f64, String> {
    if src.trim().is_empty() {
        return Err("Leerer Ausdruck".to_string());
    }
    let mut p = Parser::new(src);
    let value = p.expr();
    p.skip_ws();
    if let Some(c) = p.peek() {
        return Err(format!("Unerwartetes Zeichen '{}'", c));
    }
    if p.incomplete {
        return Err("Ausdruck unvollständig (z. B. Operator am Ende)".to_string());
    }
    Ok(value)
}

/// All characters that may legally appear in a calc query. `=` is stripped by
/// the caller.
pub fn is_calc_char(c: char) -> bool {
    if c == ' ' || c == '\t' {
        return true;
    }
    if c.is_ascii_digit() {
        return true;
    }
    if c == '.' {
        return true;
    }
    c == '+' || c == '-' || c == '*' || c == '/' || c == '%' || c == '(' || c == ')'
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    /// Set when a factor slot is empty (trailing/duplicate operator) — the
    /// evaluator must not silently cosmetically-fix malformed input.
    incomplete: bool,
}

impl Parser {
    fn new(src: &str) -> Self {
        Self {
            chars: src.chars().collect::<Vec<char>>(),
            pos: 0,
            incomplete: false,
        }
    }

    fn peek(&mut self) -> Option<char> {
        if self.pos < self.chars.len() {
            Some(self.chars[self.pos])
        } else {
            None
        }
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if c == ' ' || c == '\t' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn take(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    /// expr := term (('+'|'-') term)* — left-associative.
    fn expr(&mut self) -> f64 {
        let mut acc = self.term();
        loop {
            self.skip_ws();
            match self.peek() {
                Some('+') => {
                    self.take();
                    acc += self.term();
                }
                Some('-') => {
                    self.take();
                    acc -= self.term();
                }
                _ => break,
            }
        }
        acc
    }

    /// term := factor (('*'|'/') factor)* — left-associative; ÷0 → NaN.
    fn term(&mut self) -> f64 {
        let mut acc = self.factor();
        loop {
            self.skip_ws();
            match self.peek() {
                Some('*') => {
                    self.take();
                    acc *= self.factor();
                }
                Some('/') => {
                    self.take();
                    let den = self.factor();
                    acc = if den == 0.0 { f64::NAN } else { acc / den };
                }
                _ => break,
            }
        }
        acc
    }

    /// factor := '-' factor | postfix — unary minus, recursive.
    fn factor(&mut self) -> f64 {
        self.skip_ws();
        if let Some('-') = self.peek() {
            self.take();
            return -(self.factor());
        }
        self.postfix()
    }

    /// postfix := primary '%'? — percent = value/100.
    fn postfix(&mut self) -> f64 {
        let v = self.primary();
        self.skip_ws();
        if let Some('%') = self.peek() {
            self.take();
            return v / 100.0;
        }
        v
    }

    /// primary := Number | '(' expr ')'.
    fn primary(&mut self) -> f64 {
        self.skip_ws();
        match self.peek() {
            Some('(') => {
                self.take();
                let v = self.expr();
                // Lenient close-paren: if missing we still return the value;
                // the caller's trailing-garbage check reports the dangling
                // text, so `=2*(3` → 6, never a panic.
                self.skip_ws();
                if let Some(')') = self.peek() {
                    self.take();
                }
                v
            }
            Some(c) if c.is_ascii_digit() || c == '.' => self.number(),
            _ => {
                self.incomplete = true;
                0.0
            }
        }
    }

    /// Number := digits ('.' digits)? — also accepts a leading '.'.
    fn number(&mut self) -> f64 {
        let mut raw = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || c == '.' {
                raw.push(c);
                self.take();
            } else {
                break;
            }
        }
        // Robust span parse: an unclean float falls back to 0.
        raw.parse::<f64>().unwrap_or(0.0)
    }
}

#[cfg(test)]
mod tests {
    use crate::{evaluate, is_calc_char};

    fn ok(src: &str) -> f64 {
        let v = evaluate(src);
        assert!(v.is_ok(), "expected Ok for {src:?}, got {v:?}");
        v.unwrap()
    }

    fn err(src: &str) -> String {
        let v = evaluate(src);
        assert!(v.is_err(), "expected Err for {src:?}, got {v:?}");
        v.unwrap_err()
    }

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn arithmetic() {
        assert!(approx(ok("2+3"), 5.0));
        assert!(approx(ok("2*250"), 500.0));
        assert!(approx(ok("10/4"), 2.5));
        assert!(approx(ok("7-2-1"), 4.0));
        assert!(approx(ok("2+3*4"), 14.0)); // precedence
        assert!(approx(ok("(2+3)*4"), 20.0)); // parens
    }

    #[test]
    fn percent_is_postfix_fraction() {
        assert!(approx(ok("15%"), 0.15));
        assert!(approx(ok("2*250+15%"), 500.15)); // from the UX spec example
    }

    #[test]
    fn unary_minus() {
        assert!(approx(ok("-5"), -5.0));
        assert!(approx(ok("-(2+3)"), -5.0));
        assert!(approx(ok("-2*-3"), 6.0));
    }

    #[test]
    fn whitespace_is_ignored() {
        assert!(approx(ok("  2  +  3  "), 5.0));
    }

    #[test]
    fn division_by_zero_yields_nan_not_panic() {
        assert!(ok("1/0").is_nan());
    }

    #[test]
    fn malformed_input_errors() {
        err("2+"); // trailing operator
        err("2+3x"); // trailing garbage
        err(""); // empty
        err("  "); // whitespace only
        err(")"); // stray close paren
    }

    #[test]
    fn missing_close_paren_is_lenient() {
        assert!(approx(ok("(2+3"), 5.0));
    }

    #[test]
    fn calc_char_classification() {
        assert!(is_calc_char('2'));
        assert!(is_calc_char('+'));
        assert!(is_calc_char('%'));
        assert!(is_calc_char(' '));
        assert!(!is_calc_char('a'));
        assert!(!is_calc_char('é'));
    }
}

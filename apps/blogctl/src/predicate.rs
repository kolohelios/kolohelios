//! Tiny predicate language used by stage exit criteria.
//!
//! Grammar is intentionally boring — no eval, no expressions:
//!
//! ```text
//! <accessor> <op> <literal>
//! ```
//!
//! Accessors: `body.words`, `body.chars`, `frontmatter.<field>`,
//! `frontmatter.<field>.len`. Ops: `==`, `!=`, `>=`, `<=`, `>`, `<`.
//! Literals: integer, double-quoted string, `true`/`false`. Adding a new
//! accessor is a new variant — the language resists growing into an
//! expression evaluator.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::post::Post;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Accessor {
    BodyWords,
    BodyChars,
    Frontmatter(String),
    FrontmatterLen(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Eq,
    Ne,
    Ge,
    Le,
    Gt,
    Lt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Literal {
    Int(i64),
    Str(String),
    Bool(bool),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Predicate {
    pub accessor: Accessor,
    pub op: Op,
    pub literal: Literal,
}

/// The value an accessor resolves to. Narrow on purpose — the predicate
/// language can only compare these three shapes, plus an "absent" signal
/// for unset frontmatter fields.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Resolved {
    Int(i64),
    Str(String),
    Bool(bool),
    Absent,
}

impl Resolved {
    fn type_name(&self) -> &'static str {
        match self {
            Resolved::Int(_) => "int",
            Resolved::Str(_) => "string",
            Resolved::Bool(_) => "bool",
            Resolved::Absent => "absent",
        }
    }
}

impl Predicate {
    /// Evaluate this predicate against a `Post`. Returns `Ok(bool)` on a
    /// successful comparison; `Err(PredicateEval)` when the accessor
    /// can't be resolved or the literal's type doesn't match the
    /// accessor's value type.
    pub fn evaluate(&self, post: &Post) -> Result<bool> {
        let resolved = self
            .accessor
            .resolve(post)
            .map_err(|reason| self.eval_err(reason))?;
        compare(&resolved, self.op, &self.literal).map_err(|reason| self.eval_err(reason))
    }

    fn eval_err(&self, reason: String) -> Error {
        Error::PredicateEval {
            predicate: self.to_string(),
            reason,
        }
    }

    /// Parse a single predicate line. Three tokens: an accessor, an
    /// operator, and a literal. The literal token is "everything after
    /// the operator, trimmed" so quoted strings can contain spaces.
    pub fn parse(input: &str) -> Result<Self> {
        let trimmed = input.trim();
        let (acc_tok, after_acc) = next_token(trimmed).ok_or_else(|| Error::PredicateParse {
            message: format!("empty predicate: {input:?}"),
        })?;
        let (op_tok, after_op) = next_token(after_acc).ok_or_else(|| Error::PredicateParse {
            message: format!("missing operator: {input:?}"),
        })?;
        let lit_tok = after_op.trim();
        if lit_tok.is_empty() {
            return Err(Error::PredicateParse {
                message: format!("missing literal: {input:?}"),
            });
        }

        Ok(Self {
            accessor: parse_accessor(acc_tok)?,
            op: parse_op(op_tok)?,
            literal: parse_literal(lit_tok)?,
        })
    }
}

impl FromStr for Predicate {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s)
    }
}

impl Serialize for Predicate {
    fn serialize<S: Serializer>(&self, ser: S) -> std::result::Result<S::Ok, S::Error> {
        ser.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Predicate {
    fn deserialize<D: Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        Predicate::parse(&s).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for Predicate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}", self.accessor, self.op, self.literal)
    }
}

impl fmt::Display for Accessor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Accessor::BodyWords => f.write_str("body.words"),
            Accessor::BodyChars => f.write_str("body.chars"),
            Accessor::Frontmatter(field) => write!(f, "frontmatter.{field}"),
            Accessor::FrontmatterLen(field) => write!(f, "frontmatter.{field}.len"),
        }
    }
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Op::Eq => "==",
            Op::Ne => "!=",
            Op::Ge => ">=",
            Op::Le => "<=",
            Op::Gt => ">",
            Op::Lt => "<",
        })
    }
}

impl fmt::Display for Literal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Literal::Int(n) => write!(f, "{n}"),
            Literal::Str(s) => write!(f, "{s:?}"),
            Literal::Bool(b) => write!(f, "{b}"),
        }
    }
}

impl Accessor {
    fn resolve(&self, post: &Post) -> std::result::Result<Resolved, String> {
        match self {
            Accessor::BodyWords => Ok(Resolved::Int(count_words(&post.body) as i64)),
            Accessor::BodyChars => Ok(Resolved::Int(post.body.chars().count() as i64)),
            Accessor::Frontmatter(field) => {
                let tree = frontmatter_tree(post)?;
                match tree.get(field) {
                    None => Ok(Resolved::Absent),
                    Some(v) => resolve_scalar(v, field),
                }
            }
            Accessor::FrontmatterLen(field) => {
                let tree = frontmatter_tree(post)?;
                match tree.get(field) {
                    None => Ok(Resolved::Absent),
                    Some(Value::Array(items)) => Ok(Resolved::Int(items.len() as i64)),
                    Some(Value::String(s)) => Ok(Resolved::Int(s.chars().count() as i64)),
                    Some(other) => Err(format!(
                        "frontmatter.{field}.len is only defined for arrays and strings, got {}",
                        json_type(other)
                    )),
                }
            }
        }
    }
}

fn count_words(body: &str) -> usize {
    body.split_whitespace().count()
}

/// Serialize `PostMetadata` to a JSON object so the accessor can walk it
/// dynamically without growing a hard-coded match arm per field. The
/// clone is cheap relative to a predicate-gated stage transition.
fn frontmatter_tree(post: &Post) -> std::result::Result<serde_json::Map<String, Value>, String> {
    let value = serde_json::to_value(&post.metadata)
        .map_err(|e| format!("could not serialize frontmatter: {e}"))?;
    match value {
        Value::Object(map) => Ok(map),
        _ => Err("frontmatter did not serialize to an object".into()),
    }
}

fn resolve_scalar(value: &Value, field: &str) -> std::result::Result<Resolved, String> {
    match value {
        Value::Null => Ok(Resolved::Absent),
        Value::Bool(b) => Ok(Resolved::Bool(*b)),
        Value::String(s) => Ok(Resolved::Str(s.clone())),
        Value::Number(n) => n
            .as_i64()
            .map(Resolved::Int)
            .ok_or_else(|| format!("frontmatter.{field} is not an integer")),
        Value::Array(_) | Value::Object(_) => Err(format!(
            "frontmatter.{field} is a {}; use .len or pick a scalar field",
            json_type(value)
        )),
    }
}

fn json_type(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn compare(resolved: &Resolved, op: Op, literal: &Literal) -> std::result::Result<bool, String> {
    if matches!(resolved, Resolved::Absent) {
        return Err("accessor resolved to absent (field unset)".into());
    }
    match (resolved, literal) {
        (Resolved::Int(lhs), Literal::Int(rhs)) => Ok(apply_ord(*lhs, *rhs, op)),
        (Resolved::Str(lhs), Literal::Str(rhs)) => apply_eq(lhs == rhs, op, "string"),
        (Resolved::Bool(lhs), Literal::Bool(rhs)) => apply_eq(lhs == rhs, op, "bool"),
        (lhs, rhs) => Err(format!(
            "type mismatch: accessor is {}, literal is {}",
            lhs.type_name(),
            literal_type(rhs),
        )),
    }
}

fn apply_ord(lhs: i64, rhs: i64, op: Op) -> bool {
    match op {
        Op::Eq => lhs == rhs,
        Op::Ne => lhs != rhs,
        Op::Ge => lhs >= rhs,
        Op::Le => lhs <= rhs,
        Op::Gt => lhs > rhs,
        Op::Lt => lhs < rhs,
    }
}

fn apply_eq(eq: bool, op: Op, type_name: &str) -> std::result::Result<bool, String> {
    match op {
        Op::Eq => Ok(eq),
        Op::Ne => Ok(!eq),
        other => Err(format!(
            "operator {other} is not defined for {type_name}; only == and != apply"
        )),
    }
}

fn literal_type(l: &Literal) -> &'static str {
    match l {
        Literal::Int(_) => "int",
        Literal::Str(_) => "string",
        Literal::Bool(_) => "bool",
    }
}

/// Lift the leading non-whitespace run as a token. Returns the token
/// and the rest of the slice starting at the next character (which may
/// be whitespace — callers re-trim before scanning).
fn next_token(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    if s.is_empty() {
        return None;
    }
    let end = s.find(char::is_whitespace).unwrap_or(s.len());
    Some((&s[..end], &s[end..]))
}

fn parse_accessor(tok: &str) -> Result<Accessor> {
    match tok {
        "body.words" => Ok(Accessor::BodyWords),
        "body.chars" => Ok(Accessor::BodyChars),
        other => {
            if let Some(rest) = other.strip_prefix("frontmatter.") {
                if rest.is_empty() {
                    return Err(Error::PredicateParse {
                        message: format!("frontmatter accessor missing field name: {tok:?}"),
                    });
                }
                if let Some(field) = rest.strip_suffix(".len") {
                    if field.is_empty() || field.contains('.') {
                        return Err(Error::PredicateParse {
                            message: format!("invalid frontmatter accessor: {tok:?}"),
                        });
                    }
                    Ok(Accessor::FrontmatterLen(field.to_string()))
                } else if rest.contains('.') {
                    Err(Error::PredicateParse {
                        message: format!("nested frontmatter paths not supported: {tok:?}"),
                    })
                } else {
                    Ok(Accessor::Frontmatter(rest.to_string()))
                }
            } else {
                Err(Error::PredicateParse {
                    message: format!("unknown accessor: {tok:?}"),
                })
            }
        }
    }
}

fn parse_op(tok: &str) -> Result<Op> {
    match tok {
        "==" => Ok(Op::Eq),
        "!=" => Ok(Op::Ne),
        ">=" => Ok(Op::Ge),
        "<=" => Ok(Op::Le),
        ">" => Ok(Op::Gt),
        "<" => Ok(Op::Lt),
        other => Err(Error::PredicateParse {
            message: format!("unknown operator: {other:?}"),
        }),
    }
}

fn parse_literal(tok: &str) -> Result<Literal> {
    if tok == "true" {
        return Ok(Literal::Bool(true));
    }
    if tok == "false" {
        return Ok(Literal::Bool(false));
    }
    if let Some(inner) = tok.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        // No escape syntax — embedded `"` is rejected to keep the grammar boring.
        if inner.contains('"') {
            return Err(Error::PredicateParse {
                message: format!("string literal may not contain `\"`: {tok:?}"),
            });
        }
        return Ok(Literal::Str(inner.to_string()));
    }
    tok.parse::<i64>()
        .map(Literal::Int)
        .map_err(|_| Error::PredicateParse {
            message: format!("not a valid literal: {tok:?}"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_body_words_with_int_literal() {
        let p = Predicate::parse("body.words >= 150").unwrap();
        assert_eq!(p.accessor, Accessor::BodyWords);
        assert_eq!(p.op, Op::Ge);
        assert_eq!(p.literal, Literal::Int(150));
    }

    #[test]
    fn parses_body_chars() {
        let p = Predicate::parse("body.chars > 0").unwrap();
        assert_eq!(p.accessor, Accessor::BodyChars);
    }

    #[test]
    fn parses_frontmatter_field_with_string_literal() {
        let p = Predicate::parse(r#"frontmatter.title == "Hello""#).unwrap();
        assert_eq!(p.accessor, Accessor::Frontmatter("title".to_string()));
        assert_eq!(p.op, Op::Eq);
        assert_eq!(p.literal, Literal::Str("Hello".to_string()));
    }

    #[test]
    fn parses_frontmatter_len() {
        let p = Predicate::parse("frontmatter.tags.len > 0").unwrap();
        assert_eq!(p.accessor, Accessor::FrontmatterLen("tags".to_string()));
        assert_eq!(p.op, Op::Gt);
        assert_eq!(p.literal, Literal::Int(0));
    }

    #[test]
    fn parses_bool_literal() {
        let p = Predicate::parse("frontmatter.history_checked == true").unwrap();
        assert_eq!(p.literal, Literal::Bool(true));
        let p = Predicate::parse("frontmatter.history_checked != false").unwrap();
        assert_eq!(p.literal, Literal::Bool(false));
    }

    #[test]
    fn parses_negative_int() {
        let p = Predicate::parse("body.words > -1").unwrap();
        assert_eq!(p.literal, Literal::Int(-1));
    }

    #[test]
    fn parses_string_literal_with_internal_whitespace() {
        let p = Predicate::parse(r#"frontmatter.title == "Hello World""#).unwrap();
        assert_eq!(p.literal, Literal::Str("Hello World".to_string()));
    }

    #[test]
    fn tolerates_extra_internal_whitespace() {
        let p = Predicate::parse("  body.words   >=    150  ").unwrap();
        assert_eq!(p.accessor, Accessor::BodyWords);
        assert_eq!(p.literal, Literal::Int(150));
    }

    #[test]
    fn rejects_empty_input() {
        assert!(matches!(
            Predicate::parse(""),
            Err(Error::PredicateParse { .. })
        ));
        assert!(matches!(
            Predicate::parse("   "),
            Err(Error::PredicateParse { .. })
        ));
    }

    #[test]
    fn rejects_missing_literal() {
        assert!(matches!(
            Predicate::parse("body.words >="),
            Err(Error::PredicateParse { .. })
        ));
    }

    #[test]
    fn rejects_unknown_accessor() {
        assert!(matches!(
            Predicate::parse("post.length > 0"),
            Err(Error::PredicateParse { .. })
        ));
    }

    #[test]
    fn rejects_unknown_operator() {
        assert!(matches!(
            Predicate::parse("body.words =~ 150"),
            Err(Error::PredicateParse { .. })
        ));
    }

    #[test]
    fn rejects_nested_frontmatter_path() {
        // The boring-grammar rule: single field name after `frontmatter.`,
        // with the only allowed suffix being `.len`.
        assert!(matches!(
            Predicate::parse("frontmatter.classifications.format == \"thesis\""),
            Err(Error::PredicateParse { .. })
        ));
    }

    #[test]
    fn rejects_bare_frontmatter() {
        assert!(matches!(
            Predicate::parse("frontmatter. == 1"),
            Err(Error::PredicateParse { .. })
        ));
    }

    #[test]
    fn rejects_invalid_literal() {
        assert!(matches!(
            Predicate::parse("body.words >= maybe"),
            Err(Error::PredicateParse { .. })
        ));
    }

    #[test]
    fn rejects_string_literal_with_embedded_quote() {
        assert!(matches!(
            Predicate::parse(r#"frontmatter.title == "a"b""#),
            Err(Error::PredicateParse { .. })
        ));
    }

    #[test]
    fn display_round_trips_through_parse() {
        let cases = [
            "body.words >= 150",
            "body.chars < 1000",
            r#"frontmatter.title == "Example""#,
            "frontmatter.tags.len > 0",
            "frontmatter.history_checked == true",
        ];
        for input in cases {
            let p = Predicate::parse(input).unwrap();
            let rendered = p.to_string();
            let reparsed = Predicate::parse(&rendered).unwrap();
            assert_eq!(p, reparsed, "round-trip failed for {input:?}");
        }
    }

    #[test]
    fn from_str_dispatches_to_parse() {
        let p: Predicate = "body.words >= 1".parse().unwrap();
        assert_eq!(p.accessor, Accessor::BodyWords);
    }

    mod evaluate {
        use super::*;
        use crate::classifications::Classifications;
        use crate::kind::Kind;
        use crate::post::{Post, PostMetadata};
        use crate::stage::Stage;
        use time::macros::datetime;

        fn fixture(body: &str) -> Post {
            Post::new(
                PostMetadata {
                    title: "Example Title".into(),
                    slug: "example".into(),
                    kind: Kind::Post,
                    theme: "standard".into(),
                    status: Stage::Ideation,
                    created_at: datetime!(2026-05-03 00:00:00 UTC),
                    updated_at: datetime!(2026-05-03 00:00:00 UTC),
                    tags: vec!["rust".into(), "tooling".into()],
                    todoist_task_id: None,
                    history_checked: true,
                    targets: vec![],
                    classifications: Classifications::default(),
                    ai: None,
                },
                body,
            )
        }

        fn eval(input: &str, post: &Post) -> Result<bool> {
            Predicate::parse(input).unwrap().evaluate(post)
        }

        #[test]
        fn body_words_counts_whitespace_separated_tokens() {
            let post = fixture("one two three four five");
            assert!(eval("body.words >= 5", &post).unwrap());
            assert!(eval("body.words == 5", &post).unwrap());
            assert!(!eval("body.words > 5", &post).unwrap());
        }

        #[test]
        fn body_words_handles_runs_of_whitespace() {
            let post = fixture("alpha   beta\nbeta\tgamma");
            // Four tokens — "alpha", "beta", "beta", "gamma".
            assert!(eval("body.words == 4", &post).unwrap());
        }

        #[test]
        fn body_chars_counts_unicode_characters() {
            let post = fixture("héllo"); // 5 chars, 6 bytes.
            assert!(eval("body.chars == 5", &post).unwrap());
        }

        #[test]
        fn frontmatter_string_equality() {
            let post = fixture("body");
            assert!(eval(r#"frontmatter.title == "Example Title""#, &post).unwrap());
            assert!(eval(r#"frontmatter.title != "Other""#, &post).unwrap());
        }

        #[test]
        fn frontmatter_bool_equality() {
            let post = fixture("body");
            assert!(eval("frontmatter.history_checked == true", &post).unwrap());
            assert!(!eval("frontmatter.history_checked == false", &post).unwrap());
        }

        #[test]
        fn frontmatter_string_rejects_ordering_operator() {
            let post = fixture("body");
            let err = eval(r#"frontmatter.title > "A""#, &post).unwrap_err();
            assert!(
                matches!(err, Error::PredicateEval { ref reason, .. } if reason.contains("only == and !=")),
                "got: {err:?}"
            );
        }

        #[test]
        fn frontmatter_len_on_array() {
            let post = fixture("body");
            // tags = ["rust", "tooling"] from the fixture.
            assert!(eval("frontmatter.tags.len == 2", &post).unwrap());
            assert!(eval("frontmatter.tags.len > 0", &post).unwrap());
        }

        #[test]
        fn frontmatter_len_on_string() {
            let post = fixture("body");
            // title = "Example Title" — 13 chars.
            assert!(eval("frontmatter.title.len == 13", &post).unwrap());
        }

        #[test]
        fn type_mismatch_errors_with_clear_message() {
            let post = fixture("body");
            let err = eval(r#"frontmatter.title == 5"#, &post).unwrap_err();
            assert!(
                matches!(err, Error::PredicateEval { ref reason, .. } if reason.contains("type mismatch")),
                "got: {err:?}"
            );
        }

        #[test]
        fn absent_frontmatter_field_errors() {
            // `todoist_task_id` is None on the fixture and serializes to
            // null; the evaluator surfaces that distinctly from a value
            // mismatch.
            let post = fixture("body");
            let err = eval(r#"frontmatter.todoist_task_id == "x""#, &post).unwrap_err();
            assert!(
                matches!(err, Error::PredicateEval { ref reason, .. } if reason.contains("absent")),
                "got: {err:?}"
            );
        }

        #[test]
        fn unknown_frontmatter_field_errors() {
            let post = fixture("body");
            let err = eval(r#"frontmatter.no_such_field == "x""#, &post).unwrap_err();
            assert!(
                matches!(err, Error::PredicateEval { ref reason, .. } if reason.contains("absent")),
                "got: {err:?}"
            );
        }

        #[test]
        fn eval_error_carries_canonical_predicate_text() {
            let post = fixture("");
            let err = eval(r#"frontmatter.title == 5"#, &post).unwrap_err();
            match err {
                Error::PredicateEval { predicate, .. } => {
                    // Display canonicalizes: int literal stays unquoted.
                    assert_eq!(predicate, "frontmatter.title == 5");
                }
                other => panic!("expected PredicateEval, got {other:?}"),
            }
        }
    }
}

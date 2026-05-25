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

use crate::error::{Error, Result};

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

impl Predicate {
    /// Parse a single predicate line. Three tokens: an accessor, an
    /// operator, and a literal. The literal token is "everything after
    /// the operator, trimmed" so quoted strings can contain spaces.
    pub fn parse(input: &str) -> Result<Self> {
        let trimmed = input.trim();
        let (acc_tok, after_acc) = next_token(trimmed)
            .ok_or_else(|| Error::PredicateParse(format!("empty predicate: {input:?}")))?;
        let (op_tok, after_op) = next_token(after_acc)
            .ok_or_else(|| Error::PredicateParse(format!("missing operator: {input:?}")))?;
        let lit_tok = after_op.trim();
        if lit_tok.is_empty() {
            return Err(Error::PredicateParse(format!("missing literal: {input:?}")));
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
                    return Err(Error::PredicateParse(format!(
                        "frontmatter accessor missing field name: {tok:?}"
                    )));
                }
                if let Some(field) = rest.strip_suffix(".len") {
                    if field.is_empty() || field.contains('.') {
                        return Err(Error::PredicateParse(format!(
                            "invalid frontmatter accessor: {tok:?}"
                        )));
                    }
                    Ok(Accessor::FrontmatterLen(field.to_string()))
                } else if rest.contains('.') {
                    Err(Error::PredicateParse(format!(
                        "nested frontmatter paths not supported: {tok:?}"
                    )))
                } else {
                    Ok(Accessor::Frontmatter(rest.to_string()))
                }
            } else {
                Err(Error::PredicateParse(format!("unknown accessor: {tok:?}")))
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
        other => Err(Error::PredicateParse(format!(
            "unknown operator: {other:?}"
        ))),
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
            return Err(Error::PredicateParse(format!(
                "string literal may not contain `\"`: {tok:?}"
            )));
        }
        return Ok(Literal::Str(inner.to_string()));
    }
    tok.parse::<i64>()
        .map(Literal::Int)
        .map_err(|_| Error::PredicateParse(format!("not a valid literal: {tok:?}")))
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
            Err(Error::PredicateParse(_))
        ));
        assert!(matches!(
            Predicate::parse("   "),
            Err(Error::PredicateParse(_))
        ));
    }

    #[test]
    fn rejects_missing_literal() {
        assert!(matches!(
            Predicate::parse("body.words >="),
            Err(Error::PredicateParse(_))
        ));
    }

    #[test]
    fn rejects_unknown_accessor() {
        assert!(matches!(
            Predicate::parse("post.length > 0"),
            Err(Error::PredicateParse(_))
        ));
    }

    #[test]
    fn rejects_unknown_operator() {
        assert!(matches!(
            Predicate::parse("body.words =~ 150"),
            Err(Error::PredicateParse(_))
        ));
    }

    #[test]
    fn rejects_nested_frontmatter_path() {
        // The boring-grammar rule: single field name after `frontmatter.`,
        // with the only allowed suffix being `.len`.
        assert!(matches!(
            Predicate::parse("frontmatter.classifications.format == \"thesis\""),
            Err(Error::PredicateParse(_))
        ));
    }

    #[test]
    fn rejects_bare_frontmatter() {
        assert!(matches!(
            Predicate::parse("frontmatter. == 1"),
            Err(Error::PredicateParse(_))
        ));
    }

    #[test]
    fn rejects_invalid_literal() {
        assert!(matches!(
            Predicate::parse("body.words >= maybe"),
            Err(Error::PredicateParse(_))
        ));
    }

    #[test]
    fn rejects_string_literal_with_embedded_quote() {
        assert!(matches!(
            Predicate::parse(r#"frontmatter.title == "a"b""#),
            Err(Error::PredicateParse(_))
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
}

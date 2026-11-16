//! AimQL-like query language for runs and metrics.
//!
//! Grammar (initial scope):
//!
//! ```text
//! query      := or_part ("and" or_part)*          -- or_parts are conjoined
//! or_part    := comparison ("or" comparison)*     -- or binds tighter
//! comparison := path op literal
//! path       := ("run" | "metrics") "." identifier ("." identifier)*
//! op         := "==" | "!=" | ">" | ">=" | "<" | "<=" | "~="
//! literal    := string | number | "true" | "false"
//! ```
//!
//! Example: `run.name == "x" and metrics.cats_score > 0.9`
//!
//! The parser is hand-written (no external dependency) and produces a
//! [`Query`] AST — a conjunction of OR-groups — that backends lower onto
//! SQL predicates.

use crate::error::{CoreError, Result};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum ComparisonOp {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    Match,
}

impl fmt::Display for ComparisonOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            ComparisonOp::Eq => "==",
            ComparisonOp::Ne => "!=",
            ComparisonOp::Gt => ">",
            ComparisonOp::Ge => ">=",
            ComparisonOp::Lt => "<",
            ComparisonOp::Le => "<=",
            ComparisonOp::Match => "~=",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Path(pub Vec<String>);

impl Path {
    pub fn is_metric(&self) -> bool {
        self.0.first().map(|s| s == "metrics").unwrap_or(false)
    }
    pub fn metric_name(&self) -> Option<&str> {
        if self.is_metric() {
            self.0.get(1).map(String::as_str)
        } else {
            None
        }
    }
    pub fn metric_field(&self) -> Option<&str> {
        if self.is_metric() {
            self.0.get(2).map(String::as_str)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Str(String),
    Num(f64),
    Bool(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    pub path: Path,
    pub op: ComparisonOp,
    pub literal: Literal,
}

/// An OR-group of comparisons (`a or b or c`).
#[derive(Debug, Clone, PartialEq)]
pub struct OrChain(pub Vec<Comparison>);

/// A conjunction of OR-groups: `group1 and group2 and ...`
#[derive(Debug, Clone, PartialEq)]
pub struct Query(pub Vec<OrChain>);

impl Query {
    /// True if the query is empty (matches everything).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for Query {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, chain) in self.0.iter().enumerate() {
            if i > 0 {
                write!(f, " and ")?;
            }
            for (j, cmp) in chain.0.iter().enumerate() {
                if j > 0 {
                    write!(f, " or ")?;
                }
                write!(f, "{} {} {}", cmp.path.0.join("."), cmp.op, fmt_literal(&cmp.literal))?;
            }
        }
        Ok(())
    }
}

fn fmt_literal(lit: &Literal) -> String {
    match lit {
        Literal::Str(s) => format!("\"{s}\""),
        Literal::Num(n) => format!("{n}"),
        Literal::Bool(b) => format!("{b}"),
    }
}

struct Lexer<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        Lexer { input, pos: 0 }
    }

    fn skip_ws(&mut self) {
        while self.input[self.pos..].starts_with(char::is_whitespace) {
            self.pos += 1;
        }
    }

    fn next_token(&mut self) -> Option<Result<String>> {
        self.skip_ws();
        let rest = &self.input[self.pos..];
        if rest.is_empty() {
            return None;
        }
        for op in ["==", "!=", ">=", "<=", "~=", ">", "<"] {
            if rest.starts_with(op) {
                self.pos += op.len();
                return Some(Ok(op.to_string()));
            }
        }
        if rest.starts_with('"') || rest.starts_with('\'') {
            let quote = rest.as_bytes()[0] as char;
            let Some(end) = rest[1..].find(quote).map(|i| i + 1) else {
                return Some(Err(CoreError::InvalidQuery("unterminated string".into())));
            };
            self.pos += end + 1;
            return Some(Ok(rest[1..end].to_string()));
        }
        let end = rest
            .find(|c: char| c.is_whitespace() || "==!<>~".contains(c))
            .unwrap_or(rest.len());
        if end == 0 {
            return Some(Err(CoreError::InvalidQuery(format!(
                "unexpected character at position {}",
                self.pos
            ))));
        }
        self.pos += end;
        Some(Ok(rest[..end].to_string()))
    }
}

/// Parse an AimQL-like query string. An empty/whitespace query matches
/// everything.
pub fn parse(input: &str) -> Result<Query> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(Query(vec![]));
    }
    let mut lexer = Lexer::new(input);
    let mut groups: Vec<OrChain> = Vec::new();
    let mut current: Vec<Comparison> = Vec::new();

    loop {
        current.push(parse_comparison(&mut lexer)?);
        let Some(connective) = lexer.next_token() else {
            break;
        };
        match connective?.as_str() {
            "or" => continue, // next comparison joins the current OR-group
            "and" => {
                groups.push(OrChain(std::mem::take(&mut current)));
            }
            other => {
                return Err(CoreError::InvalidQuery(format!(
                    "expected `and` or `or`, got `{other}`"
                )))
            }
        }
    }
    if !current.is_empty() {
        groups.push(OrChain(current));
    }
    Ok(Query(groups))
}

fn parse_comparison(lexer: &mut Lexer<'_>) -> Result<Comparison> {
    let Some(tok) = lexer.next_token() else {
        return Err(CoreError::InvalidQuery("unexpected end of query".into()));
    };
    let path_tok = tok?;
    let path = Path(path_tok.split('.').map(String::from).collect());
    let op_tok = lexer
        .next_token()
        .ok_or_else(|| CoreError::InvalidQuery(format!("expected operator after `{path_tok}`")))?
        .map_err(|e| e)?;
    let op = match op_tok.as_str() {
        "==" => ComparisonOp::Eq,
        "!=" => ComparisonOp::Ne,
        ">" => ComparisonOp::Gt,
        ">=" => ComparisonOp::Ge,
        "<" => ComparisonOp::Lt,
        "<=" => ComparisonOp::Le,
        "~=" => ComparisonOp::Match,
        other => {
            return Err(CoreError::InvalidQuery(format!(
                "unknown operator `{other}`"
            )))
        }
    };
    let literal_tok = lexer
        .next_token()
        .ok_or_else(|| CoreError::InvalidQuery("expected literal".into()))??;
    let literal = parse_literal(&literal_tok)?;
    Ok(Comparison { path, op, literal })
}

fn parse_literal(tok: &str) -> Result<Literal> {
    if tok == "true" {
        return Ok(Literal::Bool(true));
    }
    if tok == "false" {
        return Ok(Literal::Bool(false));
    }
    if let Ok(n) = tok.parse::<f64>() {
        return Ok(Literal::Num(n));
    }
    Ok(Literal::Str(tok.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_comparison() {
        let q = parse("run.name == \"x\"").unwrap();
        assert_eq!(q.0.len(), 1);
        assert_eq!(q.0[0].0[0].path, Path(vec!["run".into(), "name".into()]));
        assert_eq!(q.0[0].0[0].literal, Literal::Str("x".into()));
    }

    #[test]
    fn parses_and_of_two() {
        let q = parse("run.name == \"x\" and metrics.cats_score > 0.9").unwrap();
        assert_eq!(q.0.len(), 2);
        assert_eq!(q.0[1].0[0].path.metric_name(), Some("cats_score"));
    }

    #[test]
    fn or_binds_tighter_than_and() {
        // a or b (three-way or) forms a single OR-group
        let q = parse("run.tags ~= \"a\" or run.name == \"b\" or run.status == \"completed\"")
            .unwrap();
        assert_eq!(q.0.len(), 1);
        assert_eq!(q.0[0].0.len(), 3);

        // x or y and z  ==  (x or y) and z
        let q = parse("run.name == \"x\" or run.name == \"y\" and run.status == \"running\"")
            .unwrap();
        assert_eq!(q.0.len(), 2);
        assert_eq!(q.0[0].0.len(), 2);
        assert_eq!(q.0[1].0.len(), 1);
    }

    #[test]
    fn empty_query_matches_everything() {
        assert!(parse("").unwrap().is_empty());
        assert!(parse("   ").unwrap().is_empty());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("run.name ==").is_err());
        assert!(parse("== 5").is_err());
        assert!(parse("run.name == \"x\" banana").is_err());
    }

    #[test]
    fn roundtrips_display() {
        let src = "run.name == \"x\" and metrics.cats_score > 0.9";
        let q = parse(src).unwrap();
        assert_eq!(q.to_string(), src);
    }
}

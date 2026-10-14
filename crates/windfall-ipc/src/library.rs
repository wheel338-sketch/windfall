//! Browser library protocol and the query grammar shared with the browser simulator.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::BrowserEntry;

pub const MAX_QUERY_BYTES: usize = 512;
pub const MAX_QUERY_TOKENS: usize = 64;
pub const MAX_RESULTS: usize = 500;
pub const MAX_TAGS: usize = 16;
pub const MAX_TAG_CHARS: usize = 32;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct LibraryMetadata {
    pub favorite: bool,
    pub tags: Vec<String>,
}

/// A result's file version and root membership. Never a permission to bypass the loader.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LibraryFileToken {
    pub path: String,
    pub root_path: String,
    pub generation: u32,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LibraryEntry {
    pub entry: BrowserEntry,
    pub relative_path: String,
    pub token: LibraryFileToken,
    pub metadata: LibraryMetadata,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct LibrarySearch {
    pub query: String,
    pub favorites_only: bool,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LibraryStatus {
    Indexing,
    Ready,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LibraryResults {
    pub generation: u32,
    pub status: LibraryStatus,
    pub indexed: u32,
    pub examined: u32,
    pub entries: Vec<LibraryEntry>,
    pub truncated: bool,
    pub results_truncated: bool,
    pub issues: Vec<String>,
    pub available_tags: Vec<String>,
    /// Present in the simulator, which searches fixtures rather than the user's disk.
    pub limitation: Option<String>,
}

pub fn normalize_tags(tags: &[String]) -> Result<Vec<String>, String> {
    if tags.len() > MAX_TAGS {
        return Err(windfall_core::notice!("library.tooManyTags", max = MAX_TAGS).to_string());
    }
    let mut normalized = Vec::new();
    for tag in tags {
        let tag = tag.trim().to_lowercase();
        if tag.is_empty() {
            continue;
        }
        if tag.chars().count() > MAX_TAG_CHARS || tag.chars().any(char::is_control) {
            return Err(
                windfall_core::notice!("library.tagTooLong", max = MAX_TAG_CHARS).to_string(),
            );
        }
        normalized.push(tag);
    }
    normalized.sort();
    normalized.dedup();
    Ok(normalized)
}

#[derive(Debug)]
enum Expr {
    Term(Vec<char>),
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    All,
}

#[derive(Debug, PartialEq)]
enum Token {
    Term(String),
    And,
    Or,
    Not,
    Open,
    Close,
}

/// Case-insensitive path substring terms, `*`/`?`, quotes, parentheses and Boolean operators.
#[derive(Debug)]
pub struct LibraryQuery(Expr);

impl LibraryQuery {
    pub fn parse(query: &str) -> Result<Self, String> {
        if query.len() > MAX_QUERY_BYTES {
            return Err(
                windfall_core::notice!("library.queryTooLong", max = MAX_QUERY_BYTES).to_string(),
            );
        }
        let mut tokens = Vec::new();
        let mut chars = query.chars().peekable();
        while let Some(c) = chars.next() {
            if c.is_whitespace() {
                continue;
            }
            let token = match c {
                '(' => Token::Open,
                ')' => Token::Close,
                '"' => {
                    let mut term = String::new();
                    let mut closed = false;
                    for c in chars.by_ref() {
                        if c == '"' {
                            closed = true;
                            break;
                        }
                        term.push(c);
                    }
                    if !closed || term.is_empty() {
                        return Err(
                            "Close the quotation marks around a nonempty search term.".into()
                        );
                    }
                    Token::Term(term)
                }
                _ => {
                    let mut term = String::from(c);
                    while chars
                        .peek()
                        .is_some_and(|c| !c.is_whitespace() && !matches!(c, '(' | ')' | '"'))
                    {
                        term.push(chars.next().unwrap());
                    }
                    match term.to_ascii_uppercase().as_str() {
                        "AND" => Token::And,
                        "OR" => Token::Or,
                        "NOT" => Token::Not,
                        _ => Token::Term(term),
                    }
                }
            };
            tokens.push(token);
            if tokens.len() > MAX_QUERY_TOKENS {
                return Err(windfall_core::notice!(
                    "library.tooManyTokens",
                    max = MAX_QUERY_TOKENS
                )
                .to_string());
            }
        }
        if tokens.is_empty() {
            return Ok(Self(Expr::All));
        }
        let mut parser = Parser { tokens, at: 0 };
        let expr = parser.or(0)?;
        if parser.at != parser.tokens.len() {
            return Err("Unexpected operator or closing parenthesis in search.".into());
        }
        Ok(Self(expr))
    }

    pub fn matches(&self, path: &str) -> bool {
        let path: Vec<char> = path.replace('\\', "/").to_lowercase().chars().collect();
        fn test(expr: &Expr, path: &[char]) -> bool {
            match expr {
                Expr::All => true,
                Expr::Term(pattern) => glob(pattern, path),
                Expr::Not(a) => !test(a, path),
                Expr::And(a, b) => test(a, path) && test(b, path),
                Expr::Or(a, b) => test(a, path) || test(b, path),
            }
        }
        test(&self.0, &path)
    }
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
}

impl Parser {
    fn or(&mut self, depth: usize) -> Result<Expr, String> {
        let mut expr = self.and(depth)?;
        while self.tokens.get(self.at) == Some(&Token::Or) {
            self.at += 1;
            expr = Expr::Or(Box::new(expr), Box::new(self.and(depth)?));
        }
        Ok(expr)
    }

    fn and(&mut self, depth: usize) -> Result<Expr, String> {
        let mut expr = self.unary(depth)?;
        loop {
            match self.tokens.get(self.at) {
                Some(Token::And) => self.at += 1,
                Some(Token::Term(_) | Token::Open | Token::Not) => {}
                _ => break,
            }
            expr = Expr::And(Box::new(expr), Box::new(self.unary(depth)?));
        }
        Ok(expr)
    }

    fn unary(&mut self, depth: usize) -> Result<Expr, String> {
        if depth > 16 {
            return Err("Search parentheses and NOT are limited to 16 levels.".into());
        }
        let token = self
            .tokens
            .get(self.at)
            .ok_or("Search needs a term after the operator.")?;
        self.at += 1;
        match token {
            Token::Not => Ok(Expr::Not(Box::new(self.unary(depth + 1)?))),
            Token::Term(term) => Ok(Expr::Term(
                format!("*{}*", term.replace('\\', "/").to_lowercase())
                    .chars()
                    .collect(),
            )),
            Token::Open => {
                let expr = self.or(depth + 1)?;
                if self.tokens.get(self.at) != Some(&Token::Close) {
                    return Err("Close the parenthesis in search.".into());
                }
                self.at += 1;
                Ok(expr)
            }
            _ => Err("Search needs a filename or path term here.".into()),
        }
    }
}

// Bounded greedy wildcard matching. `?` consumes a Unicode character; `*` includes separators.
fn glob(pattern: &[char], path: &[char]) -> bool {
    let (mut p, mut s, mut star, mut retry) = (0, 0, None, 0);
    while s < path.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == path[s]) {
            p += 1;
            s += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            p += 1;
            retry = s;
        } else if let Some(last) = star {
            retry += 1;
            s = retry;
            p = last + 1;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }
    p == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_matches_case_paths_wildcards_and_boolean_precedence() {
        for (query, path, expected) in [
            ("drums/k?cks AND *.WAV", "Drums/Kicks/Kick 01.wav", true),
            ("kick snare", "kick.wav", false),
            ("kick OR snare AND NOT tight", "Snare Tight.wav", false),
            ("(kick OR snare) NOT tight", "Kicks/Kick Punch.wav", true),
            ("\"kick punch\"", "Kicks/Kick Punch.wav", true),
            ("KICKS\\KICK", "Drums/Kicks/Kick.wav", true),
            ("ét?", "Été.wav", true),
            ("\"OR\"", "organ.wav", true),
        ] {
            assert_eq!(
                LibraryQuery::parse(query).unwrap().matches(path),
                expected,
                "{query}"
            );
        }
    }

    #[test]
    fn invalid_or_excessive_queries_are_actionable() {
        for query in [
            "AND kick", "kick OR", "()", "(kick", "kick)", "\"kick", "\"\"",
        ] {
            assert!(LibraryQuery::parse(query).is_err(), "{query}");
        }
        assert!(LibraryQuery::parse(&"x".repeat(513)).is_err());
        assert!(LibraryQuery::parse(&"x ".repeat(65)).is_err());
        assert!(LibraryQuery::parse(&format!("{}x{}", "(".repeat(18), ")".repeat(18))).is_err());
        assert!(LibraryQuery::parse("").unwrap().matches("anything"));
    }

    #[test]
    fn tags_are_normalized_and_bounded() {
        assert_eq!(
            normalize_tags(&[" Warm ".into(), "warm".into(), "DRUM".into()]).unwrap(),
            ["drum", "warm"]
        );
        assert!(normalize_tags(&["a".repeat(33)]).is_err());
        assert!(normalize_tags(&["a\n".into()]).is_ok());
        assert!(normalize_tags(&["a\nb".into()]).is_err());
        assert!(normalize_tags(&vec!["x".into(); 17]).is_err());
        assert_eq!(
            serde_json::from_str::<LibraryMetadata>("{}").unwrap(),
            LibraryMetadata::default()
        );
    }
}

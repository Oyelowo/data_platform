//! Simple query parser.

use crate::query::Query;

/// Errors returned by the query parser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ParseError {}

/// Parse a query string into a [`Query`].
pub fn parse(input: &str) -> Result<Query, ParseError> {
    let tokens = tokenize(input)?;
    let mut parser = Parser::new(&tokens);
    let query = parser.parse_or()?;
    parser.expect_eof()?;
    Ok(query)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Word(String),
    Phrase(String),
    Prefix(String),
    Field(String),
    Plus,
    Minus,
    LParen,
    RParen,
    And,
    Or,
    Not,
    Eof,
}

fn tokenize(input: &str) -> Result<Vec<Token>, ParseError> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(&ch) = chars.peek() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }

        match ch {
            '+' => {
                tokens.push(Token::Plus);
                chars.next();
            }
            '-' => {
                tokens.push(Token::Minus);
                chars.next();
            }
            '(' => {
                tokens.push(Token::LParen);
                chars.next();
            }
            ')' => {
                tokens.push(Token::RParen);
                chars.next();
            }
            '"' => {
                chars.next(); // consume opening quote
                let mut text = String::new();
                let mut closed = false;
                for c in chars.by_ref() {
                    if c == '"' {
                        closed = true;
                        break;
                    }
                    text.push(c);
                }
                if !closed {
                    return Err(ParseError("unclosed phrase quote".into()));
                }
                tokens.push(Token::Phrase(text));
            }
            _ => {
                let mut word = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() || matches!(c, '+' | '-' | '(' | ')' | '"') {
                        break;
                    }
                    word.push(c);
                    chars.next();
                }

                if word.eq_ignore_ascii_case("AND") {
                    tokens.push(Token::And);
                } else if word.eq_ignore_ascii_case("OR") {
                    tokens.push(Token::Or);
                } else if word.eq_ignore_ascii_case("NOT") {
                    tokens.push(Token::Not);
                } else if word.ends_with('*') && word.len() > 1 {
                    tokens.push(Token::Prefix(word[..word.len() - 1].to_string()));
                } else if let Some((field, term)) = word.split_once(':') {
                    tokens.push(Token::Field(field.to_string()));
                    tokens.push(Token::Word(term.to_string()));
                } else {
                    tokens.push(Token::Word(word));
                }
            }
        }
    }

    tokens.push(Token::Eof);
    Ok(tokens)
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn next(&mut self) -> Token {
        let token = self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof);
        self.pos += 1;
        token
    }

    fn expect_eof(&self) -> Result<(), ParseError> {
        if self.pos + 1 == self.tokens.len() {
            Ok(())
        } else {
            Err(ParseError("unexpected tokens after query".into()))
        }
    }

    fn parse_or(&mut self) -> Result<Query, ParseError> {
        let mut left = self.parse_and()?;
        while matches!(self.peek(), Token::Or) {
            self.next();
            let right = self.parse_and()?;
            left = combine_or(left, right);
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Query, ParseError> {
        let mut left = self.parse_not()?;
        loop {
            let is_implicit_and = matches!(
                self.peek(),
                Token::Word(_)
                    | Token::Phrase(_)
                    | Token::Prefix(_)
                    | Token::Field(_)
                    | Token::LParen
                    | Token::Plus
                    | Token::Minus
            );
            if !is_implicit_and && !matches!(self.peek(), Token::And) {
                break;
            }
            if matches!(self.peek(), Token::And) {
                self.next();
            }
            let right = self.parse_not()?;
            left = combine_and(left, right);
        }
        Ok(left)
    }
}

fn combine_and(left: Query, right: Query) -> Query {
    match (left, right) {
        (
            Query::Boolean {
                must: l_must,
                should: l_should,
                must_not: l_must_not,
            },
            Query::Boolean {
                must: r_must,
                should: r_should,
                must_not: r_must_not,
            },
        ) if l_should.is_empty() && r_should.is_empty() => Query::Boolean {
            must: {
                let mut m = l_must;
                m.extend(r_must);
                m
            },
            should: Vec::new(),
            must_not: {
                let mut n = l_must_not;
                n.extend(r_must_not);
                n
            },
        },
        (
            Query::Boolean {
                must,
                should,
                must_not,
            },
            other,
        ) if should.is_empty() && must_not.is_empty() => Query::Boolean {
            must: {
                let mut m = must;
                m.push(other);
                m
            },
            should: Vec::new(),
            must_not,
        },
        (
            other,
            Query::Boolean {
                must,
                should,
                must_not,
            },
        ) if should.is_empty() && must_not.is_empty() => Query::Boolean {
            must: {
                let mut m = Vec::new();
                m.push(other);
                m.extend(must);
                m
            },
            should: Vec::new(),
            must_not,
        },
        (a, b) => Query::Boolean {
            must: vec![a, b],
            should: Vec::new(),
            must_not: Vec::new(),
        },
    }
}

fn combine_or(left: Query, right: Query) -> Query {
    match (left, right) {
        (
            Query::Boolean {
                must: l_must,
                should: l_should,
                must_not: l_must_not,
            },
            Query::Boolean {
                must: r_must,
                should: r_should,
                must_not: r_must_not,
            },
        ) if l_must.is_empty() && r_must.is_empty() && l_must_not.is_empty() && r_must_not.is_empty() => {
            Query::Boolean {
                must: Vec::new(),
                should: {
                    let mut s = l_should;
                    s.extend(r_should);
                    s
                },
                must_not: Vec::new(),
            }
        }
        (
            Query::Boolean {
                must,
                should,
                must_not,
            },
            other,
        ) if must.is_empty() && must_not.is_empty() => Query::Boolean {
            must: Vec::new(),
            should: {
                let mut s = should;
                s.push(other);
                s
            },
            must_not,
        },
        (
            other,
            Query::Boolean {
                must,
                should,
                must_not,
            },
        ) if must.is_empty() && must_not.is_empty() => Query::Boolean {
            must: Vec::new(),
            should: {
                let mut s = Vec::new();
                s.push(other);
                s.extend(should);
                s
            },
            must_not,
        },
        (a, b) => Query::Boolean {
            must: Vec::new(),
            should: vec![a, b],
            must_not: Vec::new(),
        },
    }
}

impl<'a> Parser<'a> {
    fn parse_not(&mut self) -> Result<Query, ParseError> {
        if matches!(self.peek(), Token::Not) {
            self.next();
            let inner = self.parse_primary()?;
            return Ok(Query::Boolean {
                must: Vec::new(),
                should: Vec::new(),
                must_not: vec![inner],
            });
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Query, ParseError> {
        match self.peek() {
            Token::Plus => {
                self.next();
                let inner = self.parse_primary()?;
                Ok(Query::Boolean {
                    must: vec![inner],
                    should: Vec::new(),
                    must_not: Vec::new(),
                })
            }
            Token::Minus => {
                self.next();
                let inner = self.parse_primary()?;
                Ok(Query::Boolean {
                    must: Vec::new(),
                    should: Vec::new(),
                    must_not: vec![inner],
                })
            }
            Token::LParen => {
                self.next();
                let query = self.parse_or()?;
                if !matches!(self.next(), Token::RParen) {
                    return Err(ParseError("expected closing parenthesis".into()));
                }
                Ok(query)
            }
            Token::Word(term) => {
                let term = term.clone();
                self.next();
                Ok(Query::Term {
                    field: None,
                    term,
                })
            }
            Token::Phrase(phrase) => {
                let terms: Vec<String> = phrase
                    .split_whitespace()
                    .map(|s| s.to_lowercase())
                    .collect();
                self.next();
                Ok(Query::Phrase {
                    field: None,
                    terms,
                })
            }
            Token::Prefix(prefix) => {
                let prefix = prefix.clone();
                self.next();
                Ok(Query::Prefix {
                    field: None,
                    prefix,
                })
            }
            Token::Field(field) => {
                let field = field.clone();
                self.next();
                match self.next() {
                    Token::Word(term) => Ok(Query::Term {
                        field: Some(field),
                        term: term.to_lowercase(),
                    }),
                    Token::Phrase(phrase) => Ok(Query::Phrase {
                        field: Some(field),
                        terms: phrase
                            .split_whitespace()
                            .map(|s| s.to_lowercase())
                            .collect(),
                    }),
                    Token::Prefix(prefix) => Ok(Query::Prefix {
                        field: Some(field),
                        prefix: prefix.to_lowercase(),
                    }),
                    other => Err(ParseError(format!(
                        "expected term, phrase, or prefix after field, got {other:?}"
                    ))),
                }
            }
            other => Err(ParseError(format!("unexpected token: {other:?}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_term() {
        let q = parse("hello").unwrap();
        assert_eq!(q, Query::term("hello"));
    }

    #[test]
    fn parse_fielded() {
        let q = parse("title:hello").unwrap();
        assert_eq!(q, Query::field_term("title", "hello"));
    }

    #[test]
    fn parse_phrase() {
        let q = parse("\"hello world\"").unwrap();
        assert_eq!(
            q,
            Query::Phrase {
                field: None,
                terms: vec!["hello".into(), "world".into()]
            }
        );
    }

    #[test]
    fn parse_prefix() {
        let q = parse("hel*").unwrap();
        assert_eq!(
            q,
            Query::Prefix {
                field: None,
                prefix: "hel".into()
            }
        );
    }

    #[test]
    fn parse_boolean() {
        let q = parse("hello AND world").unwrap();
        assert_eq!(
            q,
            Query::Boolean {
                must: vec![Query::term("hello"), Query::term("world")],
                should: Vec::new(),
                must_not: Vec::new(),
            }
        );
    }

    #[test]
    fn parse_plus_minus() {
        let q = parse("+hello -world").unwrap();
        match q {
            Query::Boolean { must, must_not, .. } => {
                assert_eq!(must.len(), 1);
                assert_eq!(must_not.len(), 1);
            }
            _ => panic!("expected boolean query"),
        }
    }
}

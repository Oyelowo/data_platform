//! Parsing for the `quote!` template syntax.

use proc_macro::{Delimiter, Spacing, TokenStream, TokenTree};

/// A parsed `quote!` template.
pub struct Template {
    pub fragments: Vec<Fragment>,
}

/// A single piece of the template.
pub enum Fragment {
    /// An identifier token.
    Ident(String),
    /// A punctuation token.
    Punct { ch: char, spacing: Spacing },
    /// A literal token, stored as its source text.
    Lit(String),
    /// A delimited group whose contents have been parsed as a sub-template.
    Group {
        delimiter: Delimiter,
        inner: Vec<Fragment>,
    },
    /// `#expr` interpolation.
    Interpolate { expr: Vec<TokenTree> },
    /// `#( inner ) sep *` repetition.
    Repeat {
        iterable: Vec<TokenTree>,
        inner: Vec<Fragment>,
        separator: Vec<TokenTree>,
    },
}

/// Parse a `quote!` input stream into a template.
pub fn parse(input: TokenStream) -> Result<Template, String> {
    let tokens: Vec<TokenTree> = input.into_iter().collect();
    let mut cursor = Cursor::new(tokens);
    let fragments = parse_fragments(&mut cursor)?;
    if !cursor.is_empty() {
        return Err(format!(
            "unexpected token `{}` after template",
            cursor.peek(0).unwrap()
        ));
    }
    Ok(Template { fragments })
}

struct Cursor {
    tokens: Vec<TokenTree>,
    pos: usize,
}

impl Cursor {
    fn new(tokens: Vec<TokenTree>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn is_empty(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn peek(&self, offset: usize) -> Option<&TokenTree> {
        self.tokens.get(self.pos + offset)
    }

    fn next(&mut self) -> Option<TokenTree> {
        let tt = self.tokens.get(self.pos).cloned()?;
        self.pos += 1;
        Some(tt)
    }

    fn skip(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.tokens.len());
    }
}

fn parse_fragments(cursor: &mut Cursor) -> Result<Vec<Fragment>, String> {
    let mut fragments = Vec::new();
    while !cursor.is_empty() {
        fragments.push(parse_fragment(cursor)?);
    }
    Ok(fragments)
}

fn parse_fragment(cursor: &mut Cursor) -> Result<Fragment, String> {
    match cursor.peek(0) {
        Some(TokenTree::Punct(p)) => {
            let ch = p.as_char();
            let spacing = p.spacing();
            cursor.next();
            if ch == '#' {
                parse_hash(cursor)
            } else {
                Ok(Fragment::Punct { ch, spacing })
            }
        }
        Some(TokenTree::Group(_)) => {
            if let Some(TokenTree::Group(g)) = cursor.next() {
                Ok(Fragment::Group {
                    delimiter: g.delimiter(),
                    inner: parse_fragments(&mut Cursor::new(g.stream().into_iter().collect()))?,
                })
            } else {
                unreachable!()
            }
        }
        Some(TokenTree::Ident(i)) => {
            let text = i.to_string();
            cursor.next();
            Ok(Fragment::Ident(text))
        }
        Some(TokenTree::Literal(l)) => {
            let text = l.to_string();
            cursor.next();
            Ok(Fragment::Lit(text))
        }
        None => Err("unexpected end of input".to_string()),
    }
}

fn parse_hash(cursor: &mut Cursor) -> Result<Fragment, String> {
    match cursor.peek(0) {
        // `##` -> literal `#`.
        Some(TokenTree::Punct(p)) if p.as_char() == '#' => {
            cursor.next();
            Ok(Fragment::Punct {
                ch: '#',
                spacing: Spacing::Alone,
            })
        }
        // `#( ... )` may be interpolation or a repetition.
        Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis => {
            if let Some(TokenTree::Group(g)) = cursor.next() {
                let expr_tokens: Vec<TokenTree> = g.stream().into_iter().collect();
                if is_repetition(cursor) {
                    let separator = take_separator(cursor);
                    // The `*` separator itself is consumed by `take_separator`.
                    let inner = parse_fragments(&mut Cursor::new(expr_tokens.clone()))?;
                    let iterable = extract_single_iterable(&inner)?;
                    Ok(Fragment::Repeat {
                        iterable,
                        inner,
                        separator,
                    })
                } else {
                    Ok(Fragment::Interpolate { expr: expr_tokens })
                }
            } else {
                unreachable!()
            }
        }
        // `#ident` interpolation.
        Some(TokenTree::Ident(_)) => Ok(Fragment::Interpolate {
            expr: vec![cursor.next().unwrap()],
        }),
        Some(other) => Err(format!(
            "expected interpolation expression after `#`, found `{}`",
            other
        )),
        None => Err("expected interpolation expression after `#`".to_string()),
    }
}

/// True if the next tokens form the repetition suffix `*` or `, *`.
fn is_repetition(cursor: &Cursor) -> bool {
    matches!(cursor.peek(0), Some(TokenTree::Punct(p)) if p.as_char() == '*')
        || (matches!(cursor.peek(0), Some(TokenTree::Punct(p)) if p.as_char() == ',')
            && matches!(cursor.peek(1), Some(TokenTree::Punct(p)) if p.as_char() == '*'))
}

/// Consume the separator tokens and the trailing `*` of a repetition.
/// Precondition: `is_repetition(cursor)` is true.
fn take_separator(cursor: &mut Cursor) -> Vec<TokenTree> {
    let mut sep = Vec::new();
    if matches!(cursor.peek(0), Some(TokenTree::Punct(p)) if p.as_char() == '*') {
        cursor.skip(1);
        return sep;
    }
    if let Some(tt) = cursor.next() {
        sep.push(tt);
    }
    // Consume the `*`.
    if let Some(TokenTree::Punct(p)) = cursor.peek(0) {
        if p.as_char() == '*' {
            cursor.skip(1);
        }
    }
    sep
}

fn extract_single_iterable(inner: &[Fragment]) -> Result<Vec<TokenTree>, String> {
    let mut found = None;
    for fragment in inner {
        if let Fragment::Interpolate { expr } = fragment {
            if found.is_some() {
                return Err(
                    "repetition may only contain a single interpolation for the iterable"
                        .to_string(),
                );
            }
            found = Some(expr.clone());
        }
    }
    found.ok_or_else(|| "repetition must contain an interpolation such as `#( #items )*`".into())
}

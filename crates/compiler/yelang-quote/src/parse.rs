//! Parsing for the `quote!` template syntax.

use proc_macro2::{Delimiter, Punct, TokenStream, TokenTree};

/// A parsed `quote!` or `quote_spanned!` template.
pub struct Template {
    pub nodes: Vec<Node>,
}

/// One piece of a template.
pub enum Node {
    /// A literal token that is not a group (ident, punct, literal).
    Literal(TokenTree),
    /// A delimited group whose contents have been parsed for interpolations.
    Group {
        delimiter: Delimiter,
        nodes: Vec<Node>,
    },
    /// `#expr` or `#(expr)` interpolation.
    Interpolate { expr: TokenStream },
    /// `#( body ) sep *` or `#( body ) sep +` repetition.
    Repetition {
        body: Vec<Node>,
        separator: Option<Punct>,
        kind: RepKind,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepKind {
    Star,
    Plus,
}

/// Parse a `quote!` input stream into a template.
pub fn parse(input: TokenStream) -> Result<Template, String> {
    let mut cursor = Cursor::new(input);
    let nodes = parse_nodes(&mut cursor)?;
    if !cursor.is_empty() {
        return Err(format!(
            "unexpected token `{}` after template",
            cursor.peek().unwrap()
        ));
    }
    Ok(Template { nodes })
}

/// Parse a `quote_spanned!(span=> ...)` invocation.
///
/// Returns the span expression and the template body.
pub fn parse_spanned(input: TokenStream) -> Result<(TokenStream, Template), String> {
    let mut cursor = Cursor::new(input);

    // Span expression, terminated by `=>`.
    let mut span_expr = Vec::new();
    loop {
        match cursor.peek() {
            Some(TokenTree::Punct(p)) if p.as_char() == '=' => {
                cursor.next();
                match cursor.next() {
                    Some(TokenTree::Punct(p)) if p.as_char() == '>' => break,
                    Some(other) => {
                        return Err(format!(
                            "expected `=>` after span expression, found `={}`",
                            other
                        ));
                    }
                    None => return Err("expected `=>` after span expression".to_string()),
                }
            }
            Some(_) => span_expr.push(cursor.next().unwrap()),
            None => return Err("expected `=>` after span expression".to_string()),
        }
    }

    if span_expr.is_empty() {
        return Err("missing span expression before `=>`".to_string());
    }

    let span_stream: TokenStream = span_expr.into_iter().collect();
    let nodes = parse_nodes(&mut cursor)?;
    if !cursor.is_empty() {
        return Err(format!(
            "unexpected token `{}` after template",
            cursor.peek().unwrap()
        ));
    }
    Ok((span_stream, Template { nodes }))
}

fn parse_nodes(cursor: &mut Cursor) -> Result<Vec<Node>, String> {
    let mut nodes = Vec::new();
    while !cursor.is_empty() {
        nodes.push(parse_node(cursor)?);
    }
    Ok(nodes)
}

fn parse_node(cursor: &mut Cursor) -> Result<Node, String> {
    match cursor.peek() {
        Some(TokenTree::Punct(p)) if p.as_char() == '#' => {
            let hash = cursor.next().unwrap();
            // `##` escapes to a single literal `#` token.
            if let Some(TokenTree::Punct(next)) = cursor.peek()
                && next.as_char() == '#'
            {
                cursor.next();
                return Ok(Node::Literal(hash));
            }
            parse_after_hash(cursor, hash)
        }
        Some(TokenTree::Group(_)) => {
            let group = cursor.next().unwrap();
            if let TokenTree::Group(g) = group {
                let nodes = parse_nodes(&mut Cursor::new(g.stream()))?;
                Ok(Node::Group {
                    delimiter: g.delimiter(),
                    nodes,
                })
            } else {
                unreachable!()
            }
        }
        Some(_) => {
            let tt = cursor.next().unwrap();
            Ok(Node::Literal(tt))
        }
        None => Err("unexpected end of input".to_string()),
    }
}

fn parse_after_hash(cursor: &mut Cursor, _hash: TokenTree) -> Result<Node, String> {
    match cursor.peek() {
        // `#( ... )` may be interpolation or a repetition.
        Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis => {
            let group = cursor.next().unwrap();
            let body_stream = if let TokenTree::Group(g) = &group {
                g.stream()
            } else {
                unreachable!()
            };

            if let Some((separator, kind)) = parse_repetition_suffix(cursor) {
                let body = parse_nodes(&mut Cursor::new(body_stream))?;
                if body.is_empty() {
                    return Err("repetition body must not be empty".to_string());
                }
                if !has_interpolation(&body) {
                    return Err(
                        "repetition must contain at least one `#...` interpolation".to_string()
                    );
                }
                Ok(Node::Repetition {
                    body,
                    separator,
                    kind,
                })
            } else {
                Ok(Node::Interpolate { expr: body_stream })
            }
        }
        // `#ident` interpolation.
        Some(TokenTree::Ident(_)) => {
            let ident = cursor.next().unwrap();
            let mut expr = TokenStream::new();
            expr.extend([ident]);
            Ok(Node::Interpolate { expr })
        }
        // `#(expr)` where expr starts with something else is still interpolation.
        Some(TokenTree::Group(_)) => {
            let group = cursor.next().unwrap();
            if let TokenTree::Group(g) = &group {
                Ok(Node::Interpolate { expr: g.stream() })
            } else {
                unreachable!()
            }
        }
        Some(other) => Err(format!(
            "expected interpolation expression after `#`, found `{}`",
            other
        )),
        None => Err("expected interpolation expression after `#`".to_string()),
    }
}

/// If the next tokens form a repetition suffix (`*` or `, *` etc.), consume
/// them and return the separator punct and repetition kind.
fn parse_repetition_suffix(cursor: &mut Cursor) -> Option<(Option<Punct>, RepKind)> {
    // Suffix is either `*`/`+` directly, or `<sep> *` / `<sep> +` where <sep>
    // is a single punct token.
    let saved = cursor.pos;

    if let Some(TokenTree::Punct(p)) = cursor.peek() {
        let ch = p.as_char();
        if ch == '*' || ch == '+' {
            cursor.next();
            let kind = if ch == '*' {
                RepKind::Star
            } else {
                RepKind::Plus
            };
            return Some((None, kind));
        }
    }

    // Try <sep> <star/plus>.
    if let Some(TokenTree::Punct(sep)) = cursor.peek().cloned() {
        cursor.next();
        if let Some(TokenTree::Punct(p)) = cursor.peek() {
            let ch = p.as_char();
            if ch == '*' || ch == '+' {
                let kind = if ch == '*' {
                    RepKind::Star
                } else {
                    RepKind::Plus
                };
                cursor.next();
                return Some((Some(sep), kind));
            }
        }
    }

    // Not a repetition suffix; restore position.
    cursor.pos = saved;
    None
}

fn has_interpolation(nodes: &[Node]) -> bool {
    nodes.iter().any(|n| match n {
        Node::Interpolate { .. } => true,
        Node::Repetition { body, .. } => has_interpolation(body),
        Node::Group { nodes, .. } => has_interpolation(nodes),
        Node::Literal(_) => false,
    })
}

struct Cursor {
    tokens: Vec<TokenTree>,
    pos: usize,
}

impl Cursor {
    fn new(stream: TokenStream) -> Self {
        Self {
            tokens: stream.into_iter().collect(),
            pos: 0,
        }
    }

    fn is_empty(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn peek(&self) -> Option<&TokenTree> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<TokenTree> {
        let tt = self.tokens.get(self.pos).cloned()?;
        self.pos += 1;
        Some(tt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proc_macro2::{Group, Ident, Spacing, Span};

    #[test]
    fn parse_double_hash_emits_literal_hash() {
        let mut input = TokenStream::new();
        input.extend([
            TokenTree::Punct(Punct::new('#', Spacing::Alone)),
            TokenTree::Punct(Punct::new('#', Spacing::Alone)),
            TokenTree::Ident(Ident::new("flag", Span::call_site())),
        ]);

        let template = parse(input).expect("parse should succeed");
        assert_eq!(template.nodes.len(), 2);
        assert!(
            matches!(&template.nodes[0], Node::Literal(TokenTree::Punct(p)) if p.as_char() == '#')
        );
        assert!(matches!(
            &template.nodes[1],
            Node::Literal(TokenTree::Ident(_))
        ));
    }

    #[test]
    fn parse_ident_interpolation() {
        let mut input = TokenStream::new();
        input.extend([
            TokenTree::Punct(Punct::new('#', Spacing::Alone)),
            TokenTree::Ident(Ident::new("name", Span::call_site())),
        ]);

        let template = parse(input).expect("parse should succeed");
        assert_eq!(template.nodes.len(), 1);
        assert!(matches!(&template.nodes[0], Node::Interpolate { .. }));
    }

    #[test]
    fn parse_empty_template() {
        let template = parse(TokenStream::new()).expect("empty template should parse");
        assert!(template.nodes.is_empty());
    }

    #[test]
    fn parse_group_recursively() {
        let inner = {
            let mut g = TokenStream::new();
            g.extend([
                TokenTree::Ident(Ident::new("a", Span::call_site())),
                TokenTree::Punct(Punct::new('+', Spacing::Alone)),
                TokenTree::Ident(Ident::new("b", Span::call_site())),
            ]);
            let group = Group::new(Delimiter::Parenthesis, g);
            TokenTree::Group(group)
        };

        let template = parse(std::iter::once(inner).collect()).expect("parse should succeed");
        assert_eq!(template.nodes.len(), 1);
        assert!(matches!(
            &template.nodes[0],
            Node::Group {
                delimiter: Delimiter::Parenthesis,
                ..
            }
        ));
    }

    #[test]
    fn parse_repetition_with_separator() {
        let mut input = TokenStream::new();
        // #( #items ),*
        input.extend([
            TokenTree::Punct(Punct::new('#', Spacing::Alone)),
            TokenTree::Group(Group::new(Delimiter::Parenthesis, {
                let mut b = TokenStream::new();
                b.extend([
                    TokenTree::Punct(Punct::new('#', Spacing::Alone)),
                    TokenTree::Ident(Ident::new("items", Span::call_site())),
                ]);
                b
            })),
            TokenTree::Punct(Punct::new(',', Spacing::Alone)),
            TokenTree::Punct(Punct::new('*', Spacing::Alone)),
        ]);

        let template = parse(input).expect("parse should succeed");
        assert_eq!(template.nodes.len(), 1);
        assert!(matches!(
            &template.nodes[0],
            Node::Repetition {
                separator: Some(_),
                kind: RepKind::Star,
                ..
            }
        ));
    }

    #[test]
    fn parse_spanned_basic() {
        let mut input = TokenStream::new();
        input.extend([
            TokenTree::Ident(Ident::new("span", Span::call_site())),
            TokenTree::Punct(Punct::new('=', Spacing::Joint)),
            TokenTree::Punct(Punct::new('>', Spacing::Alone)),
            TokenTree::Ident(Ident::new("foo", Span::call_site())),
        ]);

        let (span_expr, template) = parse_spanned(input).expect("parse_spanned should succeed");
        assert!(!span_expr.into_iter().collect::<Vec<_>>().is_empty());
        assert_eq!(template.nodes.len(), 1);
    }
}

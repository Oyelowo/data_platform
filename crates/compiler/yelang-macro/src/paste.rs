use yelang_interner::Interner;

use yelang_macro_core::token_tree::{Delimiter, Ident, LitKind, Span, TokenStream, TokenTree};

use crate::error::ExpandError;

/// Concatenate identifier fragments into a single identifier.
///
/// Example: `paste(&["foo", "_", "bar"], span, interner)` -> `foo_bar`.
pub fn paste(parts: &[&str], span: Span, interner: &Interner) -> Ident {
    let mut out = String::new();
    for part in parts {
        out.push_str(part);
    }
    Ident::new(interner.get_or_intern(&out), span)
}

/// Concatenate identifier fragments from a slice of `Ident`s.
pub fn paste_idents(idents: &[Ident], span: Span, interner: &Interner) -> Ident {
    let mut out = String::new();
    for id in idents {
        out.push_str(id.resolve(interner));
    }
    Ident::new(interner.get_or_intern(&out), span)
}

/// Expand the eager `paste!` built-in macro.
///
/// Supported forms:
/// - `paste!(foo, bar)` -> `foobar`
/// - `paste!([foo bar])` -> `foobar`
/// - `paste!([<foo _ bar>])` -> `foo_bar` (Rust-style wrapper)
///
/// Each fragment may be an identifier, a string literal, or the punctuation `_`.
/// Commas are ignored so that both comma-separated and whitespace-separated
/// forms work.
pub fn expand_paste(
    args: &TokenStream,
    span: Span,
    interner: &Interner,
) -> Result<Ident, ExpandError> {
    // If the argument is a single bracket group, expand its contents. This is
    // the conventional `paste!([< ... >])` form.
    let tokens: Vec<_> = args.iter().collect();
    let (source, had_angle_brackets) = if let [TokenTree::Group(group)] = tokens.as_slice() {
        if group.delimiter == Delimiter::Bracket {
            let inner: Vec<_> = group.stream.iter().collect();
            if inner.len() >= 2
                && is_punct(inner.first().copied(), '<')
                && is_punct(inner.last().copied(), '>')
            {
                (inner[1..inner.len() - 1].to_vec(), true)
            } else {
                (inner, false)
            }
        } else {
            (tokens, false)
        }
    } else {
        (tokens, false)
    };

    let mut pieces = Vec::new();
    for tree in source {
        match tree {
            TokenTree::Ident(ident) => pieces.push(ident.resolve(interner).to_string()),
            TokenTree::Literal(lit) => match &lit.kind {
                LitKind::Str { value, .. } => {
                    pieces.push(interner.resolve(value).to_string());
                }
                _ => {
                    return Err(ExpandError::malformed_macro_args(
                        "paste! only accepts string literal fragments".to_string(),
                        yelang_lexer::Span::default(),
                    ));
                }
            },
            TokenTree::Punct(punct) => {
                if punct.ch == ',' {
                    // Allow comma-separated arguments.
                    continue;
                }
                if punct.ch == '_' {
                    pieces.push("_".to_string());
                } else {
                    return Err(ExpandError::malformed_macro_args(
                        format!("paste! cannot use punctuation `{}`", punct.ch),
                        yelang_lexer::Span::default(),
                    ));
                }
            }
            TokenTree::Group(group) => {
                // Flatten nested groups. This lets `paste!([foo [bar]])` work,
                // and supports macro-generated groups.
                let nested = expand_paste(&group.stream, span, interner)?;
                pieces.push(nested.resolve(interner).to_string());
            }
        }
    }

    if pieces.is_empty() {
        return Err(ExpandError::malformed_macro_args(
            "paste! requires at least one identifier fragment".to_string(),
            yelang_lexer::Span::default(),
        ));
    }

    let out = pieces.concat();

    // Validate that the result is a plausible identifier. We deliberately keep
    // this conservative and ASCII-based; later compiler stages will reject
    // anything that is not a legal surface identifier.
    if !is_valid_identifier(&out) {
        return Err(ExpandError::malformed_macro_args(
            format!("`{}` is not a valid identifier", out),
            yelang_lexer::Span::default(),
        ));
    }

    let _ = had_angle_brackets; // the wrapper form is purely syntactic

    Ok(Ident::new(interner.get_or_intern(&out), span))
}

fn is_punct(tree: Option<&TokenTree>, ch: char) -> bool {
    matches!(tree, Some(TokenTree::Punct(p)) if p.ch == ch)
}

fn is_valid_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    let first = match chars.next() {
        Some(c) => c,
        None => return false,
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use yelang_interner::Interner;

    fn paste_str(src: &str) -> String {
        let interner = Interner::new();
        let span = Span::default();
        let mut lex = yelang_ast::TokenKind::tokenize(src, &interner).unwrap();
        let tokens: Vec<_> = std::iter::from_fn(|| lex.advance().cloned()).collect();
        let stream = yelang_ast::expr::convert::from_lexer_tokens(&tokens, &interner);
        expand_paste(&stream, span, &interner)
            .unwrap()
            .resolve(&interner)
            .to_string()
    }

    #[test]
    fn paste_strings() {
        let interner = Interner::new();
        let span = Span::default();
        let id = paste(&["foo", "_", "bar"], span, &interner);
        assert_eq!(id.resolve(&interner), "foo_bar");
    }

    #[test]
    fn paste_idents_test() {
        let interner = Interner::new();
        let span = Span::default();
        let a = Ident::new(interner.get_or_intern("get"), span);
        let b = Ident::new(interner.get_or_intern("_"), span);
        let c = Ident::new(interner.get_or_intern("set"), span);
        let id = paste_idents(&[a, b, c], span, &interner);
        assert_eq!(id.resolve(&interner), "get_set");
    }

    #[test]
    fn paste_comma_separated_idents() {
        assert_eq!(paste_str("foo, bar"), "foobar");
    }

    #[test]
    fn paste_bracket_group() {
        assert_eq!(paste_str("[foo bar]"), "foobar");
    }

    #[test]
    fn paste_angle_bracket_wrapper() {
        assert_eq!(paste_str("[<foo _ bar>]"), "foo_bar");
    }

    #[test]
    fn paste_string_literal_fragment() {
        assert_eq!(paste_str("\"baz\""), "baz");
    }

    #[test]
    fn paste_rejects_invalid_identifier() {
        let interner = Interner::new();
        let span = Span::default();
        let mut lex = yelang_ast::TokenKind::tokenize("123", &interner).unwrap();
        let tokens: Vec<_> = std::iter::from_fn(|| lex.advance().cloned()).collect();
        let stream = yelang_ast::expr::convert::from_lexer_tokens(&tokens, &interner);
        assert!(expand_paste(&stream, span, &interner).is_err());
    }
}

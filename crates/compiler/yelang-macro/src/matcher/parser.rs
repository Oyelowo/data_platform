use yelang_interner::{Interner, Symbol};
use yelang_macro_core::token_tree::{Delimiter, TokenTree};

use super::cursor::TokenCursor;
use super::follow::validate_rule;
use super::types::{
    FragmentKind, MacroKind, MacroRule, MatcherError, MatcherOp, MetavarExpr, RepetitionKind,
    TranscriberOp,
};

/// Parse all rules from the body of a `macro name { ... }` definition.
pub fn parse_rules(
    body: &yelang_macro_core::token_tree::TokenStream,
    interner: &Interner,
) -> Result<Vec<MacroRule>, MatcherError> {
    let mut cursor = TokenCursor::new(body.clone());
    let mut rules = Vec::new();

    while !cursor.is_eof() {
        // Optional leading semicolons between rules.
        if cursor.peek().map(is_semicolon).unwrap_or(false) {
            cursor.advance();
            continue;
        }

        let (is_unsafe, kind, attr_args, matcher) = match cursor.peek() {
            Some(TokenTree::Ident(ident)) => {
                let name = interner.resolve(&ident.sym);
                let (is_unsafe, kind) = match name {
                    "unsafe" => {
                        cursor.advance(); // consume `unsafe`
                        let next = cursor.peek().ok_or(MatcherError::UnexpectedEof)?;
                        match next {
                            TokenTree::Ident(next_ident) => {
                                let next_name = interner.resolve(&next_ident.sym);
                                match next_name {
                                    "attr" => (true, MacroKind::Attribute),
                                    "derive" => (true, MacroKind::Derive),
                                    _ => {
                                        return Err(MatcherError::InvalidMatcher(format!(
                                            "expected `attr` or `derive` after `unsafe`, found `{}`",
                                            next_name
                                        )));
                                    }
                                }
                            }
                            other => {
                                return Err(MatcherError::InvalidMatcher(format!(
                                    "expected `attr` or `derive` after `unsafe`, found {}",
                                    other.render(interner)
                                )));
                            }
                        }
                    }
                    "attr" => (false, MacroKind::Attribute),
                    "derive" => (false, MacroKind::Derive),
                    _ => {
                        // Function-like rule whose matcher happens to start
                        // with an identifier is impossible: matchers are
                        // delimited groups. Treat as an error.
                        return Err(MatcherError::InvalidMatcher(format!(
                            "expected macro rule to start with `attr`, `derive`, `unsafe`, or a delimited group, found identifier `{}`",
                            name
                        )));
                    }
                };

                cursor.advance(); // consume `attr`/`derive`

                let attr_args_group = expect_group(&mut cursor, interner)?;
                let item_matcher_group = expect_group(&mut cursor, interner)?;

                let attr_args =
                    parse_matcher_seq(&mut TokenCursor::new(attr_args_group.stream), interner)?;
                let matcher =
                    parse_matcher_seq(&mut TokenCursor::new(item_matcher_group.stream), interner)?;
                (is_unsafe, kind, attr_args, matcher)
            }
            Some(TokenTree::Group(_)) => {
                let matcher_group = match cursor.advance() {
                    Some(TokenTree::Group(g)) => g,
                    _ => unreachable!(),
                };
                let matcher =
                    parse_matcher_seq(&mut TokenCursor::new(matcher_group.stream), interner)?;
                (false, MacroKind::FunctionLike, Vec::new(), matcher)
            }
            Some(other) => {
                return Err(MatcherError::InvalidMatcher(format!(
                    "expected macro rule to start with `attr`, `derive`, `unsafe`, or a delimited group, found {}",
                    other.render(interner)
                )));
            }
            None => break,
        };

        expect_punct_sequence(&mut cursor, interner, '=', '>')?;

        let transcriber_group = match cursor.advance() {
            Some(TokenTree::Group(g)) => g,
            Some(other) => {
                return Err(MatcherError::InvalidMatcher(format!(
                    "expected transcriber group, found {}",
                    other.render(interner)
                )));
            }
            None => return Err(MatcherError::UnexpectedEof),
        };

        let transcriber =
            parse_transcriber_seq(&mut TokenCursor::new(transcriber_group.stream), interner)?;

        let rule = MacroRule {
            kind,
            is_unsafe,
            attr_args,
            matcher,
            transcriber,
        };
        validate_rule(&rule, interner)?;

        rules.push(rule);

        // Optional trailing semicolon after a rule.
        if cursor.peek().map(is_semicolon).unwrap_or(false) {
            cursor.advance();
        }
    }

    Ok(rules)
}

fn expect_group(
    cursor: &mut TokenCursor,
    interner: &Interner,
) -> Result<yelang_macro_core::token_tree::Group, MatcherError> {
    match cursor.advance() {
        Some(TokenTree::Group(g)) => Ok(g),
        Some(other) => Err(MatcherError::InvalidMatcher(format!(
            "expected group, found {}",
            other.render(interner)
        ))),
        None => Err(MatcherError::UnexpectedEof),
    }
}

fn parse_matcher_seq(
    cursor: &mut TokenCursor,
    interner: &Interner,
) -> Result<Vec<MatcherOp>, MatcherError> {
    let mut ops = Vec::new();

    while !cursor.is_eof() {
        let tree = cursor.peek().unwrap().clone();

        if is_dollar(&tree) {
            cursor.advance(); // consume '$'
            let next = cursor.advance().ok_or(MatcherError::UnexpectedEof)?;
            match next {
                TokenTree::Ident(ident) => {
                    // $name:fragment
                    expect_punct(cursor, interner, ':')?;
                    let frag_ident = expect_ident(cursor)?;
                    let fragment = FragmentKind::from_symbol(interner, frag_ident)
                        .ok_or(MatcherError::UnknownFragmentSpecifier(frag_ident))?;
                    ops.push(MatcherOp::Metavar {
                        name: ident.sym,
                        fragment,
                    });
                }
                TokenTree::Group(group) => {
                    let inner_ops =
                        parse_matcher_seq(&mut TokenCursor::new(group.stream), interner)?;
                    let (sep, kind) = parse_repetition_suffix(cursor)?;
                    ops.push(MatcherOp::Repeat {
                        kind,
                        sep,
                        ops: inner_ops,
                    });
                }
                other => {
                    return Err(MatcherError::InvalidMatcher(format!(
                        "expected identifier or group after `$`, found {}",
                        other.render(interner)
                    )));
                }
            }
        } else if let TokenTree::Group(group) = tree {
            cursor.advance();
            let inner_ops = parse_matcher_seq(&mut TokenCursor::new(group.stream), interner)?;
            ops.push(MatcherOp::Group {
                delimiter: group.delimiter,
                ops: inner_ops,
            });
        } else {
            cursor.advance();
            ops.push(MatcherOp::Terminal(tree));
        }
    }

    Ok(ops)
}

fn parse_transcriber_seq(
    cursor: &mut TokenCursor,
    interner: &Interner,
) -> Result<Vec<TranscriberOp>, MatcherError> {
    let mut ops = Vec::new();

    while !cursor.is_eof() {
        let tree = cursor.peek().unwrap().clone();

        if is_dollar(&tree) {
            cursor.advance(); // consume '$'
            let next = cursor.advance().ok_or(MatcherError::UnexpectedEof)?;
            match next {
                TokenTree::Group(group) if group.delimiter == Delimiter::Brace => {
                    let expr = parse_metavar_expr(&group.stream, interner)?;
                    ops.push(TranscriberOp::MetavarExpr(expr));
                }
                TokenTree::Punct(p) if p.ch == '$' => {
                    ops.push(TranscriberOp::DollarDollar);
                }
                TokenTree::Ident(ident) => {
                    // $name.field is a fragment field access, not a substitution.
                    if let Some(field) = parse_fragment_field(cursor, interner)? {
                        ops.push(TranscriberOp::FragmentField {
                            name: ident.sym,
                            field,
                        });
                    } else {
                        ops.push(TranscriberOp::Subst(ident.sym));
                    }
                }
                TokenTree::Group(group) => {
                    let inner_ops =
                        parse_transcriber_seq(&mut TokenCursor::new(group.stream), interner)?;
                    let (sep, kind) = parse_repetition_suffix(cursor)?;
                    ops.push(TranscriberOp::Repeat {
                        kind,
                        sep,
                        ops: inner_ops,
                    });
                }
                other => {
                    return Err(MatcherError::InvalidTranscriber(format!(
                        "expected identifier, group, or `$$` after `$`, found {}",
                        other.render(interner)
                    )));
                }
            }
        } else if let TokenTree::Group(group) = tree {
            cursor.advance();
            let inner_ops = parse_transcriber_seq(&mut TokenCursor::new(group.stream), interner)?;
            ops.push(TranscriberOp::Group {
                delimiter: group.delimiter,
                ops: inner_ops,
            });
        } else {
            cursor.advance();
            ops.push(TranscriberOp::Terminal(tree));
        }
    }

    Ok(ops)
}

/// Parse a metavariable expression from the contents of a `${ ... }` group.
fn parse_metavar_expr(
    stream: &yelang_macro_core::token_tree::TokenStream,
    interner: &Interner,
) -> Result<MetavarExpr, MatcherError> {
    let mut cursor = TokenCursor::new(stream.clone());

    let name = expect_ident(&mut cursor)?;
    let name_str = interner.resolve(&name);

    let args_group = match cursor.advance() {
        Some(TokenTree::Group(g)) if g.delimiter == Delimiter::Parenthesis => g,
        Some(other) => {
            return Err(MatcherError::InvalidMetavarExpr(format!(
                "expected `(...)` after `{}`, found {}",
                name_str,
                other.render(interner)
            )));
        }
        None => {
            return Err(MatcherError::InvalidMetavarExpr(format!(
                "expected `(...)` after `{}`",
                name_str
            )));
        }
    };

    let mut args = TokenCursor::new(args_group.stream);

    let expr = match name_str {
        "count" => {
            let var = expect_ident(&mut args)?;
            let depth = if args.peek().map(is_comma).unwrap_or(false) {
                args.advance();
                Some(expect_usize_literal(&mut args, interner)?)
            } else {
                None
            };
            MetavarExpr::Count { name: var, depth }
        }
        "index" => {
            let depth = if args.is_eof() {
                None
            } else {
                Some(expect_usize_literal(&mut args, interner)?)
            };
            MetavarExpr::Index { depth }
        }
        "len" => {
            let depth = if args.is_eof() {
                None
            } else {
                Some(expect_usize_literal(&mut args, interner)?)
            };
            MetavarExpr::Len { depth }
        }
        "ignore" => {
            let var = expect_ident(&mut args)?;
            MetavarExpr::Ignore { name: var }
        }
        other => {
            return Err(MatcherError::InvalidMetavarExpr(format!(
                "unknown metavariable expression `{}`",
                other
            )));
        }
    };

    if !args.is_eof() {
        return Err(MatcherError::InvalidMetavarExpr(
            "trailing tokens in metavariable expression arguments".to_string(),
        ));
    }

    if !cursor.is_eof() {
        return Err(MatcherError::InvalidMetavarExpr(
            "trailing tokens in metavariable expression".to_string(),
        ));
    }

    Ok(expr)
}

fn is_comma(tree: &TokenTree) -> bool {
    matches!(tree, TokenTree::Punct(p) if p.ch == ',')
}

/// If the next tokens are `. field_name`, consume them and return the field
/// name. Otherwise return `None` so the caller emits a plain substitution.
fn parse_fragment_field(
    cursor: &mut TokenCursor,
    interner: &Interner,
) -> Result<Option<Symbol>, MatcherError> {
    let mut lookahead = cursor.clone();
    if !matches!(lookahead.advance(), Some(TokenTree::Punct(p)) if p.ch == '.') {
        return Ok(None);
    }
    let field = match lookahead.advance() {
        Some(TokenTree::Ident(ident)) => ident.sym,
        Some(other) => {
            return Err(MatcherError::InvalidTranscriber(format!(
                "expected identifier after `.` in fragment field access, found {}",
                other.render(interner)
            )));
        }
        None => return Err(MatcherError::UnexpectedEof),
    };
    *cursor = lookahead;
    Ok(Some(field))
}

fn expect_usize_literal(
    cursor: &mut TokenCursor,
    interner: &Interner,
) -> Result<usize, MatcherError> {
    match cursor.advance() {
        Some(TokenTree::Literal(l)) => match &l.kind {
            yelang_macro_core::token_tree::LitKind::Int { value, .. } => {
                let s = interner.resolve(value);
                s.parse::<usize>().map_err(|e| {
                    MatcherError::InvalidMetavarExpr(format!("expected usize literal: {}", e))
                })
            }
            _ => Err(MatcherError::InvalidMetavarExpr(format!(
                "expected usize literal, found {}",
                TokenTree::Literal(l.clone()).render(interner)
            ))),
        },
        Some(other) => Err(MatcherError::InvalidMetavarExpr(format!(
            "expected usize literal, found {}",
            other.render(interner)
        ))),
        None => Err(MatcherError::UnexpectedEof),
    }
}

/// Parse the optional separator and required repetition operator after a
/// repetition group.
fn parse_repetition_suffix(
    cursor: &mut TokenCursor,
) -> Result<(Option<TokenTree>, RepetitionKind), MatcherError> {
    // Optional separator: any single token before the operator.
    let sep = cursor
        .peek()
        .cloned()
        .filter(|t| !is_repetition_operator(t));
    if sep.is_some() {
        cursor.advance();
    }

    let op = cursor.advance().ok_or(MatcherError::InvalidRepetition)?;
    let kind = match &op {
        TokenTree::Punct(p) => {
            RepetitionKind::from_char(p.ch).ok_or(MatcherError::InvalidRepetition)?
        }
        _ => return Err(MatcherError::InvalidRepetition),
    };

    Ok((sep, kind))
}

fn is_dollar(tree: &TokenTree) -> bool {
    matches!(tree, TokenTree::Punct(p) if p.ch == '$')
}

fn is_semicolon(tree: &TokenTree) -> bool {
    matches!(tree, TokenTree::Punct(p) if p.ch == ';')
}

fn is_repetition_operator(tree: &TokenTree) -> bool {
    matches!(tree, TokenTree::Punct(p) if matches!(p.ch, '*' | '+' | '?'))
}

fn expect_punct(
    cursor: &mut TokenCursor,
    interner: &Interner,
    expected: char,
) -> Result<(), MatcherError> {
    match cursor.advance() {
        Some(TokenTree::Punct(p)) if p.ch == expected => Ok(()),
        Some(other) => Err(MatcherError::Expected(format!(
            "`{}`, found {}",
            expected,
            other.render(interner)
        ))),
        None => Err(MatcherError::UnexpectedEof),
    }
}

fn expect_punct_sequence(
    cursor: &mut TokenCursor,
    interner: &Interner,
    first: char,
    second: char,
) -> Result<(), MatcherError> {
    expect_punct(cursor, interner, first)?;
    expect_punct(cursor, interner, second)
}

fn expect_ident(cursor: &mut TokenCursor) -> Result<yelang_interner::Symbol, MatcherError> {
    match cursor.advance() {
        Some(TokenTree::Ident(ident)) => Ok(ident.sym),
        _ => Err(MatcherError::Expected("identifier".to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yelang_ast::TokenKind;
    use yelang_interner::Interner;

    fn tokenize_macro_body(
        src: &str,
        interner: &mut Interner,
    ) -> yelang_macro_core::token_tree::TokenStream {
        let mut stream = TokenKind::tokenize(src, interner).unwrap();
        let tokens: Vec<_> = std::iter::from_fn(|| stream.advance().cloned()).collect();
        yelang_ast::expr::convert::from_lexer_tokens(&tokens, interner)
    }

    #[test]
    fn parse_single_rule() {
        let mut interner = Interner::new();
        let body = tokenize_macro_body("($x:expr) => { $x }", &mut interner);
        let rules = parse_rules(&body, &interner).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].matcher.len(), 1);
        assert_eq!(rules[0].transcriber.len(), 1);
    }

    #[test]
    fn parse_multiple_rules_with_semicolons() {
        let mut interner = Interner::new();
        let body = tokenize_macro_body("($x:expr) => { $x }; ($x:ident) => { $x };", &mut interner);
        let rules = parse_rules(&body, &interner).unwrap();
        assert_eq!(rules.len(), 2);
    }

    #[test]
    fn parse_unknown_fragment_errors() {
        let mut interner = Interner::new();
        let body = tokenize_macro_body("($x:unknown) => { $x }", &mut interner);
        assert!(matches!(
            parse_rules(&body, &interner),
            Err(MatcherError::UnknownFragmentSpecifier(_))
        ));
    }

    #[test]
    fn parse_missing_arrow_errors() {
        let mut interner = Interner::new();
        let body = tokenize_macro_body("($x:expr) { $x }", &mut interner);
        assert!(parse_rules(&body, &interner).is_err());
    }

    #[test]
    fn parse_repetition_with_separator() {
        let mut interner = Interner::new();
        let body = tokenize_macro_body("($($x:expr),*) => { [ $($x),* ] }", &mut interner);
        let rules = parse_rules(&body, &interner).unwrap();
        assert_eq!(rules.len(), 1);
        let matcher = &rules[0].matcher;
        assert!(matches!(matcher[0], MatcherOp::Repeat { .. }));
    }

    #[test]
    fn parse_optional_repetition() {
        let mut interner = Interner::new();
        let body = tokenize_macro_body("($x:expr $(, $y:expr)?) => { $x }", &mut interner);
        let rules = parse_rules(&body, &interner).unwrap();
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn parse_attribute_rule() {
        let mut interner = Interner::new();
        let body = tokenize_macro_body("attr()($item:item) => { $item }", &mut interner);
        let rules = parse_rules(&body, &interner).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].kind, MacroKind::Attribute);
        assert!(rules[0].attr_args.is_empty());
        assert!(!rules[0].matcher.is_empty());
    }

    #[test]
    fn parse_attribute_rule_with_args() {
        let mut interner = Interner::new();
        let body = tokenize_macro_body(
            "attr(name = $name:literal)($item:item) => { $item }",
            &mut interner,
        );
        let rules = parse_rules(&body, &interner).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].kind, MacroKind::Attribute);
        assert!(!rules[0].attr_args.is_empty());
    }

    #[test]
    fn parse_derive_rule() {
        let mut interner = Interner::new();
        let body = tokenize_macro_body(
            "derive()(struct $name:ident $_:tt) => { impl Foo for $name {} }",
            &mut interner,
        );
        let rules = parse_rules(&body, &interner).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].kind, MacroKind::Derive);
        assert!(rules[0].attr_args.is_empty());
    }

    #[test]
    fn parse_metavar_expr_count() {
        let mut interner = Interner::new();
        let body = tokenize_macro_body("($x:expr) => ( ${count(x)} )", &mut interner);
        let rules = parse_rules(&body, &interner).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].transcriber.len(), 1);
        assert!(
            matches!(
                rules[0].transcriber[0],
                TranscriberOp::MetavarExpr(MetavarExpr::Count { .. })
            ),
            "got {:?}",
            rules[0].transcriber[0]
        );
    }

    #[test]
    fn parse_metavar_expr_index() {
        let mut interner = Interner::new();
        let body = tokenize_macro_body("($x:expr) => ( ${index()} )", &mut interner);
        let rules = parse_rules(&body, &interner).unwrap();
        assert!(matches!(
            rules[0].transcriber[0],
            TranscriberOp::MetavarExpr(MetavarExpr::Index { .. })
        ));
    }

    #[test]
    fn parse_transcriber_tuple_group() {
        let mut interner = Interner::new();
        let body = tokenize_macro_body("($x:expr) => ( (${index()}, ${len()}) )", &mut interner);
        let rules = parse_rules(&body, &interner).unwrap();
        assert!(!rules[0].transcriber.is_empty());
    }

    #[test]
    fn parse_mixed_rule_kinds() {
        let mut interner = Interner::new();
        let body = tokenize_macro_body(
            "($x:expr) => { $x }; attr()($item:item) => { $item };",
            &mut interner,
        );
        let rules = parse_rules(&body, &interner).unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].kind, MacroKind::FunctionLike);
        assert_eq!(rules[1].kind, MacroKind::Attribute);
    }
}

use yelang_interner::Interner;
#[cfg(test)]
use yelang_macro_core::token_tree::Delimiter;
use yelang_macro_core::token_tree::{TokenStream, TokenTree};

use super::bindings::{Binding, Bindings};
use super::cursor::TokenCursor;
use super::fragment::consume_fragment;
#[cfg(test)]
use super::types::FragmentKind;
use super::types::{MacroRule, MatcherError, MatcherOp, RepetitionKind};

/// Try to match a macro rule against an invocation argument stream.
///
/// Returns the captured bindings on success. The bindings do **not** include
/// captures from repetitions that matched zero times; those names are simply
/// absent.
pub fn try_match_rule(
    rule: &MacroRule,
    args: &TokenStream,
    interner: &Interner,
) -> Result<Bindings, MatcherError> {
    try_match_matcher(&rule.matcher, args, interner)
}

/// Try to match an arbitrary matcher sequence against a token stream.
pub fn try_match_matcher(
    matcher: &[MatcherOp],
    args: &TokenStream,
    interner: &Interner,
) -> Result<Bindings, MatcherError> {
    let mut cursor = TokenCursor::new(args.clone());
    let bindings = match_ops(matcher, &mut cursor, interner)?;
    if !cursor.is_eof() {
        return Err(MatcherError::InvalidMatcher(format!(
            "unexpected trailing tokens: {}",
            TokenStream::from_vec(cursor.remaining().to_vec()).render(interner)
        )));
    }
    Ok(bindings)
}

fn match_ops(
    ops: &[MatcherOp],
    cursor: &mut TokenCursor,
    interner: &Interner,
) -> Result<Bindings, MatcherError> {
    let mut bindings = Bindings::new();

    for op in ops {
        let op_bindings = match_op(op, cursor, interner)?;
        bindings.extend(op_bindings);
    }

    Ok(bindings)
}

fn match_op(
    op: &MatcherOp,
    cursor: &mut TokenCursor,
    interner: &Interner,
) -> Result<Bindings, MatcherError> {
    match op {
        MatcherOp::Terminal(expected) => {
            let actual = cursor.advance().ok_or(MatcherError::UnexpectedEof)?;
            if !trees_equal(expected, &actual) {
                return Err(MatcherError::InvalidMatcher(format!(
                    "expected `{}`, found `{}`",
                    expected.render(interner),
                    actual.render(interner)
                )));
            }
            Ok(Bindings::new())
        }
        MatcherOp::Metavar { name, fragment } => {
            let captured = consume_fragment(cursor, *fragment, interner).map_err(|e| {
                MatcherError::InvalidMatcher(format!(
                    "could not match fragment `{}`: {}",
                    interner.resolve(name),
                    e
                ))
            })?;
            let mut b = Bindings::new();
            b.insert(*name, Binding::Single(captured));
            Ok(b)
        }
        MatcherOp::Group { delimiter, ops } => {
            let group = cursor.advance().ok_or(MatcherError::UnexpectedEof)?;
            match group {
                TokenTree::Group(g) if g.delimiter == *delimiter => {
                    let mut inner = TokenCursor::new(g.stream);
                    let bindings = match_ops(ops, &mut inner, interner)?;
                    if !inner.is_eof() {
                        return Err(MatcherError::InvalidMatcher(
                            "trailing tokens inside group".to_string(),
                        ));
                    }
                    Ok(bindings)
                }
                other => Err(MatcherError::InvalidMatcher(format!(
                    "expected {:?} group, found `{}`",
                    delimiter,
                    other.render(interner)
                ))),
            }
        }
        MatcherOp::Repeat { kind, sep, ops } => match kind {
            RepetitionKind::ZeroOrOne => match match_ops(ops, cursor, interner) {
                Ok(iter_bindings) => {
                    let merged = Bindings::from_repeat_iterations(vec![iter_bindings]);
                    Ok(merged)
                }
                Err(_) => {
                    // Zero iterations: name is absent from bindings.
                    Ok(Bindings::new())
                }
            },
            RepetitionKind::ZeroOrMore | RepetitionKind::OneOrMore => {
                let mut iterations = Vec::new();
                let is_plus = matches!(kind, RepetitionKind::OneOrMore);
                loop {
                    match match_ops(ops, cursor, interner) {
                        Ok(iter_bindings) => iterations.push(iter_bindings),
                        Err(_) => break,
                    }
                    if let Some(sep_tree) = sep {
                        if !cursor
                            .peek()
                            .map(|t| trees_equal(t, sep_tree))
                            .unwrap_or(false)
                        {
                            break;
                        }
                        // Lookahead past the separator to see if another element
                        // follows. If not, this is a trailing separator: consume
                        // it and stop.
                        let mut lookahead = cursor.clone();
                        lookahead.advance();
                        if match_ops(ops, &mut lookahead, interner).is_err() {
                            cursor.advance();
                            break;
                        }
                        cursor.advance();
                    }
                }
                if is_plus && iterations.is_empty() {
                    return Err(MatcherError::InvalidMatcher(
                        "expected at least one repetition".to_string(),
                    ));
                }
                Ok(Bindings::from_repeat_iterations(iterations))
            }
        },
    }
}

/// Structural equality of token trees, ignoring span/id/hygiene context.
fn trees_equal(a: &TokenTree, b: &TokenTree) -> bool {
    match (a, b) {
        (TokenTree::Ident(ai), TokenTree::Ident(bi)) => ai.sym == bi.sym,
        (TokenTree::Literal(al), TokenTree::Literal(bl)) => al.kind == bl.kind,
        (TokenTree::Punct(ap), TokenTree::Punct(bp)) => ap.ch == bp.ch,
        (TokenTree::Group(ag), TokenTree::Group(bg)) => {
            ag.delimiter == bg.delimiter
                && ag.stream.trees().len() == bg.stream.trees().len()
                && ag
                    .stream
                    .trees()
                    .iter()
                    .zip(bg.stream.trees().iter())
                    .all(|(x, y)| trees_equal(x, y))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::MacroKind;
    use super::*;
    use yelang_interner::Interner;
    use yelang_macro_core::token_tree::{Group, Ident, Punct, Spacing, Span, TokenTree};

    fn ident(name: &str, interner: &Interner) -> TokenTree {
        TokenTree::Ident(Ident::new(interner.get_or_intern(name), Span::default()))
    }

    fn paren_rule_with_single_ident(interner: &Interner) -> MacroRule {
        MacroRule {
            kind: MacroKind::FunctionLike,
            attr_args: vec![],
            matcher: vec![MatcherOp::Group {
                delimiter: Delimiter::Parenthesis,
                ops: vec![MatcherOp::Metavar {
                    name: interner.get_or_intern("x"),
                    fragment: FragmentKind::Ident,
                }],
            }],
            transcriber: vec![TranscriberOp::Subst(interner.get_or_intern("x"))],
        }
    }

    use super::super::types::TranscriberOp;

    #[test]
    fn match_simple_ident() {
        let interner = Interner::new();
        let rule = paren_rule_with_single_ident(&interner);
        let args = TokenStream::from_vec(vec![TokenTree::Group(Group::new(
            Delimiter::Parenthesis,
            TokenStream::from_vec(vec![ident("foo", &interner)]),
            Span::default(),
        ))]);
        let bindings = try_match_rule(&rule, &args, &interner).unwrap();
        let b = bindings.get(interner.get_or_intern("x")).unwrap();
        assert_eq!(b.expect_single("x").unwrap().render(&interner), "foo");
    }

    #[test]
    fn match_expr_fragment() {
        let interner = Interner::new();
        let rule = MacroRule {
            kind: MacroKind::FunctionLike,
            attr_args: vec![],
            matcher: vec![MatcherOp::Group {
                delimiter: Delimiter::Parenthesis,
                ops: vec![MatcherOp::Metavar {
                    name: interner.get_or_intern("x"),
                    fragment: FragmentKind::Expr,
                }],
            }],
            transcriber: vec![TranscriberOp::Subst(interner.get_or_intern("x"))],
        };
        let args = TokenStream::from_vec(vec![TokenTree::Group(Group::new(
            Delimiter::Parenthesis,
            TokenStream::from_vec(vec![TokenTree::Literal(
                yelang_macro_core::token_tree::Literal::int(
                    interner.get_or_intern("42"),
                    Span::default(),
                ),
            )]),
            Span::default(),
        ))]);
        let bindings = try_match_rule(&rule, &args, &interner).unwrap();
        let b = bindings.get(interner.get_or_intern("x")).unwrap();
        assert_eq!(b.expect_single("x").unwrap().render(&interner), "42");
    }

    #[test]
    fn match_repetition_star_with_separator() {
        let interner = Interner::new();
        let rule = MacroRule {
            kind: MacroKind::FunctionLike,
            attr_args: vec![],
            matcher: vec![MatcherOp::Group {
                delimiter: Delimiter::Parenthesis,
                ops: vec![MatcherOp::Repeat {
                    kind: RepetitionKind::ZeroOrMore,
                    sep: Some(TokenTree::Punct(Punct::new(
                        ',',
                        Spacing::Alone,
                        Span::default(),
                    ))),
                    ops: vec![MatcherOp::Metavar {
                        name: interner.get_or_intern("x"),
                        fragment: FragmentKind::Expr,
                    }],
                }],
            }],
            transcriber: vec![],
        };
        let args = TokenStream::from_vec(vec![TokenTree::Group(Group::new(
            Delimiter::Parenthesis,
            TokenStream::from_vec(vec![
                TokenTree::Literal(yelang_macro_core::token_tree::Literal::int(
                    interner.get_or_intern("1"),
                    Span::default(),
                )),
                TokenTree::Punct(Punct::new(',', Spacing::Alone, Span::default())),
                TokenTree::Literal(yelang_macro_core::token_tree::Literal::int(
                    interner.get_or_intern("2"),
                    Span::default(),
                )),
            ]),
            Span::default(),
        ))]);
        let bindings = try_match_rule(&rule, &args, &interner).unwrap();
        let b = bindings.get(interner.get_or_intern("x")).unwrap();
        assert_eq!(b.expect_repeat("x").unwrap().len(), 2);
    }

    #[test]
    fn match_optional_repetition_present() {
        let interner = Interner::new();
        let rule = MacroRule {
            kind: MacroKind::FunctionLike,
            attr_args: vec![],
            matcher: vec![MatcherOp::Group {
                delimiter: Delimiter::Parenthesis,
                ops: vec![
                    MatcherOp::Metavar {
                        name: interner.get_or_intern("x"),
                        fragment: FragmentKind::Expr,
                    },
                    MatcherOp::Repeat {
                        kind: RepetitionKind::ZeroOrOne,
                        sep: None,
                        ops: vec![
                            MatcherOp::Terminal(TokenTree::Punct(Punct::new(
                                ',',
                                Spacing::Alone,
                                Span::default(),
                            ))),
                            MatcherOp::Metavar {
                                name: interner.get_or_intern("y"),
                                fragment: FragmentKind::Expr,
                            },
                        ],
                    },
                ],
            }],
            transcriber: vec![],
        };
        let args = TokenStream::from_vec(vec![TokenTree::Group(Group::new(
            Delimiter::Parenthesis,
            TokenStream::from_vec(vec![
                TokenTree::Literal(yelang_macro_core::token_tree::Literal::int(
                    interner.get_or_intern("1"),
                    Span::default(),
                )),
                TokenTree::Punct(Punct::new(',', Spacing::Alone, Span::default())),
                TokenTree::Literal(yelang_macro_core::token_tree::Literal::int(
                    interner.get_or_intern("2"),
                    Span::default(),
                )),
            ]),
            Span::default(),
        ))]);
        let bindings = try_match_rule(&rule, &args, &interner).unwrap();
        assert!(bindings.get(interner.get_or_intern("x")).is_some());
        assert!(bindings.get(interner.get_or_intern("y")).is_some());
    }

    #[test]
    fn match_optional_repetition_absent() {
        let interner = Interner::new();
        let rule = MacroRule {
            kind: MacroKind::FunctionLike,
            attr_args: vec![],
            matcher: vec![MatcherOp::Group {
                delimiter: Delimiter::Parenthesis,
                ops: vec![
                    MatcherOp::Metavar {
                        name: interner.get_or_intern("x"),
                        fragment: FragmentKind::Expr,
                    },
                    MatcherOp::Repeat {
                        kind: RepetitionKind::ZeroOrOne,
                        sep: None,
                        ops: vec![MatcherOp::Metavar {
                            name: interner.get_or_intern("y"),
                            fragment: FragmentKind::Expr,
                        }],
                    },
                ],
            }],
            transcriber: vec![],
        };
        let args = TokenStream::from_vec(vec![TokenTree::Group(Group::new(
            Delimiter::Parenthesis,
            TokenStream::from_vec(vec![TokenTree::Literal(
                yelang_macro_core::token_tree::Literal::int(
                    interner.get_or_intern("1"),
                    Span::default(),
                ),
            )]),
            Span::default(),
        ))]);
        let bindings = try_match_rule(&rule, &args, &interner).unwrap();
        assert!(bindings.get(interner.get_or_intern("x")).is_some());
        assert!(bindings.get(interner.get_or_intern("y")).is_none());
    }

    #[test]
    fn match_terminal_literal() {
        let interner = Interner::new();
        let rule = MacroRule {
            kind: MacroKind::FunctionLike,
            attr_args: vec![],
            matcher: vec![MatcherOp::Group {
                delimiter: Delimiter::Parenthesis,
                ops: vec![MatcherOp::Terminal(TokenTree::Literal(
                    yelang_macro_core::token_tree::Literal::int(
                        interner.get_or_intern("42"),
                        Span::default(),
                    ),
                ))],
            }],
            transcriber: vec![],
        };
        let args = TokenStream::from_vec(vec![TokenTree::Group(Group::new(
            Delimiter::Parenthesis,
            TokenStream::from_vec(vec![TokenTree::Literal(
                yelang_macro_core::token_tree::Literal::int(
                    interner.get_or_intern("42"),
                    Span::default(),
                ),
            )]),
            Span::default(),
        ))]);
        let result = try_match_rule(&rule, &args, &interner);
        assert!(result.is_ok(), "{:?}", result.unwrap_err());
    }

    #[test]
    fn match_terminal_mismatch_fails() {
        let interner = Interner::new();
        let rule = MacroRule {
            kind: MacroKind::FunctionLike,
            attr_args: vec![],
            matcher: vec![MatcherOp::Group {
                delimiter: Delimiter::Parenthesis,
                ops: vec![MatcherOp::Terminal(TokenTree::Literal(
                    yelang_macro_core::token_tree::Literal::int(
                        interner.get_or_intern("42"),
                        Span::default(),
                    ),
                ))],
            }],
            transcriber: vec![],
        };
        let args = TokenStream::from_vec(vec![TokenTree::Group(Group::new(
            Delimiter::Parenthesis,
            TokenStream::from_vec(vec![TokenTree::Literal(
                yelang_macro_core::token_tree::Literal::int(
                    interner.get_or_intern("7"),
                    Span::default(),
                ),
            )]),
            Span::default(),
        ))]);
        assert!(try_match_rule(&rule, &args, &interner).is_err());
    }
}

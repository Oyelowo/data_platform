use crate::expr::parse_macro_args;
use crate::{
    Expr, ExprKind, ExprPath, Ident, Literal, MacroInvocation, Path, RangeExpr, RangeOp, T,
};
use yelang_lexer::{Either, ParseTokenStream, SeparatedList, TokenResult, TokenStream, match_map};

use super::{
    FieldPattern, Mutability, Pattern, PatternKind, RestrictedPattern, SlicePatternElement,
};

#[derive(Debug, Clone, PartialEq)]
struct PatternFields {
    fields: Vec<FieldPattern>,
    rest: bool,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for PatternFields {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        use crate::tokenizer::TokenKind;

        let mut fields = Vec::new();
        let mut rest = false;

        loop {
            match stream.peek().map(|token| token.kind()) {
                Some(TokenKind::CloseBrace) => break,
                Some(TokenKind::DotDot) => {
                    stream.parse::<T![..]>()?;
                    rest = true;
                    let _ = stream.parse::<Option<T![,]>>()?;
                    break;
                }
                Some(_) => {}
                None => {
                    return Err(yelang_lexer::TokenError::UnexpectedEof {
                        expected: "record pattern field or `..`".to_string(),
                        span: stream.current_span(),
                    });
                }
            }

            fields.push(stream.parse::<FieldPattern>()?);

            if stream.parse::<Option<T![,]>>()?.is_none() {
                break;
            }
        }

        Ok(Self { fields, rest })
    }
}

fn parse_path_led_pattern(
    stream: &mut TokenStream<crate::tokenizer::TokenKind>,
) -> TokenResult<PatternKind> {
    use crate::tokenizer::TokenKind;

    let checkpoint = stream.checkpoint();
    let path = stream.parse::<ExprPath>()?.0;

    // Macro invocation in pattern position: `MyPat!(...)`, `MyPat![...]`, `MyPat!{...}`.
    if matches!(
        stream.peek().map(|token| token.kind()),
        Some(TokenKind::Bang)
    ) {
        stream.advance(); // consume `!`
        let args = parse_macro_args(stream)?;
        return Ok(PatternKind::MacroInvocation(MacroInvocation {
            path,
            args,
            span: stream.span_since(checkpoint),
        }));
    }

    if matches!(
        stream.peek().map(|token| token.kind()),
        Some(TokenKind::OpenParen)
    ) {
        let (_, patterns, _) =
            stream.parse::<(T!['('], SeparatedList<Pattern, T![,], true>, T![')'])>()?;
        return Ok(PatternKind::TupleStruct {
            path,
            patterns: patterns.value_owned(),
        });
    }

    if matches!(
        stream.peek().map(|token| token.kind()),
        Some(TokenKind::OpenBrace)
    ) {
        let (_, parsed, _) = stream.parse::<(T!['{'], PatternFields, T!['}'])>()?;
        return Ok(PatternKind::Struct {
            path,
            fields: parsed.fields,
            rest: parsed.rest,
        });
    }

    let subpattern = stream
        .parse::<Option<(T![@], Pattern)>>()?
        .map(|(_, pattern)| Box::new(pattern));

    if path.segments.len() > 1 || path.is_absolute {
        return Ok(PatternKind::Path(path));
    }

    Ok(PatternKind::Binding {
        name: path.segments[0].ident,
        mutability: Mutability::Immutable,
        subpattern,
    })
}

fn parse_mut_binding_pattern(
    stream: &mut TokenStream<crate::tokenizer::TokenKind>,
) -> TokenResult<PatternKind> {
    stream.parse::<T![mut]>()?;
    let name = parse_pattern_binding_name(stream)?;
    let subpattern = stream.parse::<Option<(T![@], Pattern)>>()?;
    Ok(PatternKind::Binding {
        name,
        mutability: Mutability::Mutable,
        subpattern: subpattern.map(|(_, pattern)| Box::new(pattern)),
    })
}

fn parse_binding_pattern(
    stream: &mut TokenStream<crate::tokenizer::TokenKind>,
) -> TokenResult<PatternKind> {
    let name = parse_pattern_binding_name(stream)?;
    let subpattern = stream.parse::<Option<(T![@], Pattern)>>()?;
    Ok(PatternKind::Binding {
        name,
        mutability: Mutability::Immutable,
        subpattern: subpattern.map(|(_, pattern)| Box::new(pattern)),
    })
}

fn parse_pattern_binding_name(
    stream: &mut TokenStream<crate::tokenizer::TokenKind>,
) -> TokenResult<Ident> {
    use crate::tokenizer::TokenKind;

    if let Ok(ident) = stream.parse::<Ident>() {
        return Ok(ident);
    }

    let Some(token) = stream.peek() else {
        return Err(yelang_lexer::TokenError::UnexpectedEof {
            expected: "binding name".to_string(),
            span: stream.current_span(),
        });
    };

    let Some(name) = (match token.kind() {
        TokenKind::Start => Some("start"),
        TokenKind::Limit => Some("limit"),
        TokenKind::Asc => Some("asc"),
        TokenKind::Desc => Some("desc"),
        TokenKind::Order => Some("order"),
        TokenKind::RangeKw => Some("range"),
        TokenKind::HopsKw => Some("hops"),
        _ => None,
    }) else {
        return Err(yelang_lexer::TokenError::UnexpectedToken {
            expected: "binding name".to_string(),
            found: token.to_string(),
            span: token.span(),
        });
    };

    stream.advance();
    Ok(Ident {
        symbol: stream.interner().get_or_intern(name),
        span: stream.span(),
    })
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Mutability {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let muta = stream.parse::<Option<T![mut]>>()?;
        Ok(if muta.is_some() {
            Mutability::Mutable
        } else {
            Mutability::Immutable
        })
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for FieldPattern {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpint = stream.checkpoint();
        match_map!(stream,
            (Ident, T![:], Pattern) => |(name, _, pattern, )| FieldPattern {
                    name,
                    is_placeholder: matches!(pattern.pattern, PatternKind::Wildcard),
                    pattern,
                    is_shorthand: false,
            },
            (Option<T![mut]>, Ident) => |(mutability, name)| FieldPattern {
                name,
                pattern: Pattern {
                    pattern: PatternKind::Binding {
                        name,
                        mutability: if mutability.is_some() {
                            Mutability::Mutable
                        } else {
                            Mutability::Immutable
                        },
                        subpattern: None,
                    },
                    span: stream.span_since(checkpint),
                },
                is_shorthand: true,
                is_placeholder: false,
            }
        )
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for SlicePatternElement {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();

        // Special-case: `..` or `..ident` (slice rest binding).
        if stream.parse::<Option<T![..]>>()?.is_some() {
            let name = stream.parse::<Option<Ident>>()?;
            let span = stream.span_since(checkpoint);
            return Ok(SlicePatternElement(Pattern {
                pattern: PatternKind::Rest { name },
                span,
            }));
        }

        // Otherwise parse a normal pattern.
        let p = stream.parse::<Pattern>()?;
        Ok(SlicePatternElement(p))
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for RestrictedPattern {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();
        type PathOrLiteral = Either<Path, Literal>;

        // Parse non-path-led patterns first. Path-led constructor and binding patterns are
        // disambiguated manually so qualified tuple-struct patterns like `Option::Some(x)`
        // cannot fall through to the plain path fallback before their payload is parsed.
        let pattern_kind = match_map!(stream,
            (T![&], Option<T![mut]>, RestrictedPattern) => |(_, mutability, pattern)| PatternKind::Ref {
                pattern: Box::new(pattern.0),
                is_mut: mutability.is_some(),
            },
            (T!['('], T![')']) => |_| PatternKind::Tuple { patterns: Vec::new() },
            (T!['('], Pattern, T![')']) => |(_, pattern, _)| PatternKind::Grouped(Box::new(pattern)),
            (Option<PathOrLiteral>, RangeOp, Option<PathOrLiteral>) => |r| {
                let path_to_expr = |p| {
                    let span = stream.span_since(checkpoint);
                    let expr = match p {
                        Either::Left(path) => Expr {
                            kind: ExprKind::Path(path),
                            span,
                        },
                        Either::Right(lit) => Expr {
                            kind: ExprKind::Literal(lit),
                            span,
                        },
                    };
                    Box::new(expr)
                };
                let (start, op, end) = r;
                if start.is_none() && end.is_none() {
                    PatternKind::Rest { name: None }
                } else {
                    PatternKind::Range(RangeExpr {
                        start: start.map(path_to_expr),
                        op,
                        end: end.map(path_to_expr),
                    })
                }
            },
            Literal => PatternKind::Literal,
            (Path, T!['{'], PatternFields, T!['}']) => |(path, _, parsed, _)| {
                PatternKind::Struct {
                    path,
                    fields: parsed.fields,
                    rest: parsed.rest,
                }
            },
            (T!['{'], PatternFields, T!['}']) => |(_, parsed, _)| {
                PatternKind::Record {
                    fields: parsed.fields,
                    rest: parsed.rest,
                }
            },
            (Path, T!['('], SeparatedList<Pattern, T![,], true>, T![')']) => |(path, _, patterns, _)| {
                PatternKind::TupleStruct {
                    path,
                    patterns: patterns.value_owned(),
                }
            },
            (T!['('], SeparatedList<Pattern, T![,], true>, T![')']) => |(_, patterns, _)| {
                PatternKind::Tuple {
                    patterns: patterns.value_owned(),
                }
            },
            (T!['['], T![']']) => |(_, _)| PatternKind::Slice {
                patterns: Vec::new(),
            },
            (T!['['], SeparatedList<SlicePatternElement, T![,], true>, T![']']) => |(_, patterns, _)| {
                PatternKind::Slice {
                    patterns: patterns.value_owned().into_iter().map(|p| p.0).collect(),
                }
            },
            T!["_"] => |_| PatternKind::Wildcard,
        )
        .or_else(|_| {
            stream.restore(checkpoint);
            parse_path_led_pattern(stream)
        })
        .or_else(|_| {
            stream.restore(checkpoint);
            parse_mut_binding_pattern(stream)
        })
        .or_else(|_| {
            stream.restore(checkpoint);
            parse_binding_pattern(stream)
        })?;

        let span = stream.span_since(checkpoint);
        Ok(RestrictedPattern(Pattern {
            pattern: pattern_kind,
            span,
        }))
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Pattern {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        // Parse one or more RestrictedPatterns separated by |
        let patterns_list = stream.parse::<SeparatedList<RestrictedPattern, T![|], false>>()?;
        let span = patterns_list.span();
        let mut patterns = patterns_list
            .value_owned()
            .into_iter()
            .map(|p| p.0)
            .collect::<Vec<_>>();

        if patterns.len() == 1 {
            Ok(patterns.pop().unwrap())
        } else {
            Ok(Pattern {
                pattern: PatternKind::Or(patterns),
                span,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::tokenizer::TokenKind;
    use crate::{Interner, Mutability, PatternKind};

    use super::Pattern;

    #[test]
    fn test_pattern_parses_qualified_tuple_struct_constructor() {
        let input = "Option::Some(limit)";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let pattern = stream.parse::<Pattern>().unwrap();

        assert!(matches!(pattern.pattern, PatternKind::TupleStruct { .. }));
    }

    #[test]
    fn test_path_led_pattern_helper_parses_qualified_tuple_struct_constructor() {
        let input = "Option::Some(limit)";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let pattern = super::parse_path_led_pattern(&mut stream).unwrap();

        assert!(matches!(pattern, PatternKind::TupleStruct { .. }));
    }

    #[test]
    fn test_record_pattern_parses_bare_fields() {
        let input = "{ index, value: user, .. }";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let pattern = stream.parse::<Pattern>().unwrap();

        let PatternKind::Record { fields, rest } = pattern.pattern else {
            panic!("expected record pattern, got {pattern:?}");
        };
        assert_eq!(fields.len(), 2);
        assert!(rest);
    }

    #[test]
    fn test_record_pattern_parses_rename_rest_nesting_and_mutability() {
        let input = "{ index: mut i, value: { left, right: renamed, .. }, mut rank, .. }";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let pattern = stream.parse::<Pattern>().unwrap();

        let PatternKind::Record { fields, rest } = pattern.pattern else {
            panic!("expected record pattern, got {pattern:?}");
        };
        assert_eq!(fields.len(), 3);
        assert!(rest);

        let PatternKind::Binding {
            name,
            mutability,
            subpattern,
        } = &fields[0].pattern.pattern
        else {
            panic!(
                "expected renamed mutable binding, got {:?}",
                fields[0].pattern
            );
        };
        assert_eq!(name.symbol, interner.intern("i"));
        assert_eq!(*mutability, Mutability::Mutable);
        assert!(subpattern.is_none());
        assert!(!fields[0].is_shorthand);

        let PatternKind::Record {
            fields: nested,
            rest: nested_rest,
        } = &fields[1].pattern.pattern
        else {
            panic!(
                "expected nested record pattern, got {:?}",
                fields[1].pattern
            );
        };
        assert_eq!(nested.len(), 2);
        assert!(*nested_rest);
        assert!(nested[0].is_shorthand);
        assert_eq!(nested[0].name.symbol, interner.intern("left"));
        assert!(!nested[1].is_shorthand);
        assert_eq!(nested[1].name.symbol, interner.intern("right"));

        assert!(fields[2].is_shorthand);
        let PatternKind::Binding {
            name,
            mutability,
            subpattern,
        } = &fields[2].pattern.pattern
        else {
            panic!(
                "expected shorthand mutable binding for rank, got {:?}",
                fields[2].pattern
            );
        };
        assert_eq!(name.symbol, interner.intern("rank"));
        assert_eq!(*mutability, Mutability::Mutable);
        assert!(subpattern.is_none());
    }
}

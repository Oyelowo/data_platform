/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 21/03/2025
 */

use crate::ParenthesizedArgs;
use crate::{Expr, Ident, Precedence, Restrictions, T, Type};
use std::fmt;
use yelang_lexer::{
    ParseTokenStream, SeparatedList, Span, TokenError, TokenResult, TokenStream, match_map,
};

fn parse_path_ident(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Ident> {
    stream.parse::<Ident>()
}

/// Path with segments for namespaced access
///
/// # Example
/// ```
/// std::collections::HashMap
/// ::std::vec::Vec  
/// math::sum
/// User::from_id
/// ```
#[derive(Clone, PartialEq)]
pub struct Path {
    /// Optional qualified self prefix: `<T as Trait>::Assoc`
    pub qself: Option<Box<QSelf>>,
    /// Path segments
    pub segments: Vec<PathSegment>,
    /// Whether the path is absolute (starts with `::`)
    pub is_absolute: bool,
    /// Span of the entire path
    pub span: Span,
}

impl fmt::Debug for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("Path");
        if let Some(qself) = &self.qself {
            s.field("qself", qself);
        }
        s.field("segments", &self.segments)
            .field("is_absolute", &self.is_absolute)
            .field("span", &self.span)
            .finish()
    }
}

/// Qualified self prefix for fully-qualified paths.
///
/// Examples:
/// - `<Vec<T> as IntoIterator>::Item`
/// - `<Self as Trait>::assoc_fn`
#[derive(Debug, Clone, PartialEq)]
pub struct QSelf {
    pub ty: Type,
    pub as_trait: Option<Box<Path>>,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Path {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();

        // Qualified self paths: `<T as Trait>::Assoc`
        // This is parsed before absolute/relative paths.
        let qself_checkpoint = stream.checkpoint();
        if stream.parse::<Option<T![<]>>()?.is_some() {
            let ty = stream.parse::<Type>()?;

            let as_trait = if stream.parse::<Option<T![as]>>()?.is_some() {
                Some(Box::new(stream.parse::<Path>()?))
            } else {
                None
            };

            stream.parse::<T![>]>()?;
            let qself_span = stream.span_since(qself_checkpoint);

            // Must be followed by ::<segment>
            stream.parse::<T![::]>()?;
            let segments = stream
                .parse::<SeparatedList<PathSegment, T![::], false>>()?
                .value_owned();

            if segments.is_empty() {
                return Err(TokenError::SyntaxError {
                    message: "Expected at least one segment in path".to_string(),
                    span: stream.span_since(checkpoint),
                    source: None,
                });
            }

            return Ok(Path {
                qself: Some(Box::new(QSelf {
                    ty,
                    as_trait,
                    span: qself_span,
                })),
                segments,
                is_absolute: false,
                span: stream.span_since(checkpoint),
            });
        }
        stream.restore(qself_checkpoint);

        // Check for leading :: (absolute path)
        let is_absolute = stream.parse::<Option<T![::]>>()?.is_some();

        let segments = stream
            .parse::<SeparatedList<PathSegment, T![::], false>>()?
            .value_owned();

        if segments.is_empty() {
            return Err(TokenError::SyntaxError {
                message: "Expected at least one segment in path".to_string(),
                span: stream.span_since(checkpoint),
                source: None,
            });
        }

        Ok(Path {
            qself: None,
            segments,
            is_absolute,
            span: stream.span_since(checkpoint),
        })
    }
}

impl Path {
    /// Create a new ExprPath from a single identifier
    pub fn new_single_ident(ident: Ident) -> Self {
        Path {
            qself: None,
            segments: vec![PathSegment { ident, args: None }],
            is_absolute: false,
            span: ident.span(),
        }
    }

    /// Get all segments
    pub fn segments(&self) -> &[PathSegment] {
        &self.segments
    }

    /// Check if this is a single-segment path (simple identifier)
    pub fn is_simple(&self) -> bool {
        self.qself.is_none() && self.segments.len() == 1 && !self.is_absolute
    }

    /// Get the final segment (useful for method/field names)
    pub fn last_segment(&self) -> Option<&PathSegment> {
        self.segments.last()
    }

    /// Get the first segment
    pub fn first_segment(&self) -> Option<&PathSegment> {
        self.segments.first()
    }

    /// If this is a simple single-segment path, get the identifier
    pub fn standalone_ident(&self) -> Option<&Ident> {
        if self.qself.is_none() && self.segments.len() == 1 && !self.is_absolute {
            Some(&self.segments[0].ident)
        } else {
            None
        }
    }

    /// Check if this is an absolute path
    pub fn is_absolute(&self) -> bool {
        self.is_absolute
    }

    /// Returns the span covering the first `prefix_len` segments of this path.
    ///
    /// This is useful when you need a span for the *base* portion of a path like
    /// `Self::Item` (base span = `Self`) or `Vec<T>::Item` (base span = `Vec<T>`),
    /// while `Path::span` continues to represent the entire path.
    pub fn prefix_span(&self, prefix_len: usize) -> Option<Span> {
        if self.qself.is_some() {
            return None;
        }

        if prefix_len == 0 || prefix_len > self.segments.len() {
            return None;
        }

        let mut span = self.segments[0].span();
        for seg in &self.segments[1..prefix_len] {
            span = span.merge(seg.span());
        }
        Some(span)
    }

    pub fn first_segment_span(&self) -> Option<Span> {
        self.prefix_span(1)
    }
}

// maths::round(4.56)
// utils::slugify("Hello World")
// time::format(now(), "YYYY-MM-DD")
// maths.round(4.56)
// utils.slugify("Hello World")
// time.format(now(), "YYYY-MM-DD")
//

/// A single segment in a path
///
/// # Example
/// ```
/// HashMap<K, V>        // ident: "HashMap", generic_args: [K, V]
/// Vec<i32>            // ident: "Vec", generic_args: [i32]
/// Option              // ident: "Option", generic_args: None
/// fn takes_fn_mut<F: FnMut(String) -> i32>(f: F) {}  
///  fn takes_fn_once<F: FnOnce() -> String>(f: F) {}
/// super                // is_super: true
/// self                 // is_self: true
/// crate                // is_crate: true
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct PathSegment {
    /// Segment identifier
    pub ident: Ident,
    /// Optional generic arguments for this segment
    pub args: Option<GenericArgs>,
}

/// A path parsed in expression position.
///
/// In expression contexts, `<` is ambiguous with the less-than operator, so we
/// intentionally do not parse angle-bracketed generic args on path segments.
#[derive(Debug, Clone, PartialEq)]
pub struct ExprPath(pub Path);

/// A single path segment parsed in expression position.
///
/// In expression contexts, angle-bracketed args must be written using Rust-like turbofish
/// `::<...>` to avoid ambiguity with `<` as a comparison operator.
#[derive(Debug, Clone, PartialEq)]
pub struct ExprPathSegment(pub PathSegment);

impl ParseTokenStream<crate::tokenizer::TokenKind> for ExprPathSegment {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let ident = parse_path_ident(stream)?;

        let args = {
            let cp = stream.checkpoint();
            if let Ok((_, ab)) = stream.parse::<(T![::], AngleBracketedArgs)>() {
                Some(GenericArgs::AngleBracketed(ab))
            } else {
                stream.restore(cp);
                None
            }
        };

        Ok(ExprPathSegment(PathSegment { ident, args }))
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for ExprPath {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        // In expression position, treat `<` as a comparison operator, not the
        // start of generic arguments.
        //
        // We *do* allow Rust-like turbofish `::<...>` because the leading `::`
        // removes the ambiguity.
        //
        // We also support qualified self paths `<Type as Trait>::item` because
        // `<` at the start of an atomic expression is unambiguously a qself path.
        let checkpoint = stream.checkpoint();

        // Try qself: `<Type as Trait>::segment` or `<Type>::segment`
        let qself_checkpoint = stream.checkpoint();
        if stream.parse::<Option<T![<]>>()?.is_some() {
            let ty = stream.parse::<Type>()?;

            let as_trait = if stream.parse::<Option<T![as]>>()?.is_some() {
                Some(Box::new(stream.parse::<Path>()?))
            } else {
                None
            };

            stream.parse::<T![>]>()?;
            let qself_span = stream.span_since(qself_checkpoint);

            // Must be followed by ::<segment>
            stream.parse::<T![::]>()?;
            let mut segments: Vec<PathSegment> = Vec::new();
            let first_ident = parse_path_ident(stream)?;
            let first_args = {
                let cp = stream.checkpoint();
                if let Ok((_, ab)) = stream.parse::<(T![::], AngleBracketedArgs)>() {
                    Some(GenericArgs::AngleBracketed(ab))
                } else {
                    stream.restore(cp);
                    None
                }
            };
            segments.push(PathSegment {
                ident: first_ident,
                args: first_args,
            });

            while stream.parse::<Option<T![::]>>()?.is_some() {
                let ident = parse_path_ident(stream)?;
                let args = {
                    let cp = stream.checkpoint();
                    if let Ok((_, ab)) = stream.parse::<(T![::], AngleBracketedArgs)>() {
                        Some(GenericArgs::AngleBracketed(ab))
                    } else {
                        stream.restore(cp);
                        None
                    }
                };
                segments.push(PathSegment { ident, args });
            }

            if segments.is_empty() {
                return Err(TokenError::SyntaxError {
                    message: "Expected at least one segment in path".to_string(),
                    span: stream.span_since(checkpoint),
                    source: None,
                });
            }

            return Ok(ExprPath(Path {
                qself: Some(Box::new(QSelf {
                    ty,
                    as_trait,
                    span: qself_span,
                })),
                segments,
                is_absolute: false,
                span: stream.span_since(checkpoint),
            }));
        }
        stream.restore(qself_checkpoint);

        // Check for leading :: (absolute path)
        let is_absolute = stream.parse::<Option<T![::]>>()?.is_some();

        // Parse segments manually so each can optionally carry `::<...>`.
        let mut segments: Vec<PathSegment> = Vec::new();

        let first_ident = parse_path_ident(stream)?;
        let first_args = {
            let cp = stream.checkpoint();
            if let Ok((_, ab)) = stream.parse::<(T![::], AngleBracketedArgs)>() {
                Some(GenericArgs::AngleBracketed(ab))
            } else {
                stream.restore(cp);
                None
            }
        };
        segments.push(PathSegment {
            ident: first_ident,
            args: first_args,
        });

        while stream.parse::<Option<T![::]>>()?.is_some() {
            let ident = parse_path_ident(stream)?;
            let args = {
                let cp = stream.checkpoint();
                if let Ok((_, ab)) = stream.parse::<(T![::], AngleBracketedArgs)>() {
                    Some(GenericArgs::AngleBracketed(ab))
                } else {
                    stream.restore(cp);
                    None
                }
            };
            segments.push(PathSegment { ident, args });
        }

        if segments.is_empty() {
            return Err(TokenError::SyntaxError {
                message: "Expected at least one segment in path".to_string(),
                span: stream.span_since(checkpoint),
                source: None,
            });
        }

        Ok(ExprPath(Path {
            qself: None,
            segments,
            is_absolute,
            span: stream.span_since(checkpoint),
        }))
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for PathSegment {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let ident = parse_path_ident(stream)?;

        // In non-expression positions, generic args are written as `<...>`.
        // (In expression positions, `::<...>` is required to avoid `<` ambiguity.)
        let args = stream.parse::<Option<GenericArgs>>()?;

        Ok(PathSegment { ident, args })
    }
}

impl PathSegment {
    // Check if this is a keyword segment (super, self, or crate)
    // pub fn is_keyword(&self) -> bool {
    //     self.is_super || self.is_self || self.is_crate
    // }

    /// Check if this segment has generic arguments
    pub fn has_generics(&self) -> bool {
        self.args.is_some()
    }

    pub fn span(&self) -> Span {
        let mut span = self.ident.span();
        if let Some(args) = &self.args {
            span = span.merge(match args {
                GenericArgs::AngleBracketed(a) => a.span,
                GenericArgs::Parenthesized(p) => p.span,
            });
        }
        span
    }
}

/// Generic arguments for a path segment
#[derive(Debug, Clone, PartialEq)]
pub enum GenericArgs {
    /// Angle-bracketed arguments: `Foo<A, B>`
    AngleBracketed(AngleBracketedArgs),

    /// Parenthesized arguments: `Foo(A, B) -> C`
    ///
    /// Parenthesized is just a syntactic sugar for function traits.
    /// i.e `Fn(A, B) -> C` is equivalent to `Fn<(A, B), Output = C>`
    ///
    /// These 3 are equivalent:
    /// fn lowo<T: Fn(i32) -> i32>(m: T) {}
    ///
    /// fn lowo<T: Fn<(i32), Output = i32>>(m: T) {}
    ///
    /// fn lowo<T>(m: T)
    /// where
    ///     T: Fn<(i32), Output = i32> {}
    Parenthesized(ParenthesizedArgs),
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for GenericArgs {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let res = match_map!(
            stream,
            AngleBracketedArgs => GenericArgs::AngleBracketed
            // NOTE: Removed ParenthesizedArgs to allow function calls to be parsed by postfix parser
            //  ParenthesizedArgs => GenericArgs::Parenthesized
        )?;
        Ok(res)
    }
}

/// Angle-bracketed generic arguments
#[derive(Debug, Clone, PartialEq)]
pub struct AngleBracketedArgs {
    pub args: Vec<AngleBracketedArg>,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for AngleBracketedArgs {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let ((_, args, _), span) =
            stream
                .parse_with_span::<(T![<], SeparatedList<AngleBracketedArg, T![,], true>, T![>])>(
                )?;

        Ok(AngleBracketedArgs {
            args: args.value_owned(),
            span,
        })
    }
}

/// Arguments in angle-bracketed generic args
#[derive(Debug, Clone, PartialEq)]
pub enum AngleBracketedArg {
    /// Type argument: `i32`, `Vec<T>`
    Type(Type),
    /// Const argument: `10`, `N + 1`
    Const(Expr),
    /// Associated type binding: `Item = i32`
    AssociatedType { name: Ident, ty: Type },
}

/// Expression parser used for const generic arguments inside angle brackets.
///
/// This treats `>` as a closing delimiter rather than a comparison operator so
/// constructs like `FooTrait<3>` don't get parsed as `FooTrait<(3 > { ... })>`.
#[derive(Debug, Clone, PartialEq)]
struct ConstGenericExpr(Expr);

impl ParseTokenStream<crate::tokenizer::TokenKind> for ConstGenericExpr {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        Expr::parse_pratt(stream, Precedence::None, Restrictions::GENERIC_ARG).map(ConstGenericExpr)
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for AngleBracketedArg {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let res = match_map!(
            stream,
            (Ident, T![=], Type) => |(name, _, ty)| AngleBracketedArg::AssociatedType { name, ty },
            Type => AngleBracketedArg::Type,
            ConstGenericExpr => |e| AngleBracketedArg::Const(e.0)

        )?;
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use crate::Interner;
    use crate::{ExprPath, Path, PathSegment, TokenKind};
    use yelang_lexer::ParseTokenStream;

    #[test]
    fn test_path_does_not_parse_parenthesized_args() {
        let input = "count(posts)";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).expect("Tokenization failed");

        // PathSegment should parse "count" and stop before "(posts)"
        let result = PathSegment::parse(&mut stream);
        assert!(
            result.is_ok(),
            "PathSegment should parse identifier 'count'"
        );

        // Verify that the next token is '('
        let next = stream.peek();
        assert_eq!(
            next.map(|t| t.kind()),
            Some(&TokenKind::OpenParen),
            "PathSegment should not consume parenthesized args"
        );
    }

    #[test]
    fn test_path_stops_before_constructor_payload() {
        let input = "Option::Some(limit)";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).expect("Tokenization failed");

        let result = Path::parse(&mut stream).expect("Path should parse qualified constructor");
        assert_eq!(result.segments.len(), 2);
        assert_eq!(stream.peek().map(|t| t.kind()), Some(&TokenKind::OpenParen));
    }

    #[test]
    fn test_expr_path_parses_contextual_query_keyword_ident() {
        let input = "limit";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).expect("Tokenization failed");

        let result =
            ExprPath::parse(&mut stream).expect("ExprPath should parse contextual keyword ident");
        assert_eq!(result.0.segments.len(), 1);
    }

    #[test]
    fn test_path_parsing() {
        let test_cases = vec![
            ("User", "Single segment path"),
            ("Option::Some", "Qualified constructor path"),
            ("std::collections::HashMap", "Multi-segment path"),
            ("math::utils", "Two segment path"),
            ("::std::vec::Vec", "Absolute path"),
        ];

        for (input, description) in test_cases {
            println!("Testing path: {} - {}", input, description);
            let mut interner = Interner::new();
            let mut stream =
                TokenKind::tokenize(input, &mut interner).expect("Tokenization failed");

            match Path::parse(&mut stream) {
                Ok(parsed_path) => {
                    println!("✓ Successfully parsed path: {:?}", parsed_path);
                    assert!(
                        !parsed_path.segments.is_empty(),
                        "Path should have segments"
                    );

                    // Check segment count matches expected
                    let expected_segments = if input.starts_with("::") {
                        input.split("::").filter(|s| !s.is_empty()).count()
                    } else {
                        input.split("::").count()
                    };
                    assert_eq!(
                        parsed_path.segments.len(),
                        expected_segments,
                        "Expected {} segments, got {}",
                        expected_segments,
                        parsed_path.segments.len()
                    );

                    // Check is_absolute
                    let expected_absolute = input.starts_with("::");
                    assert_eq!(
                        parsed_path.is_absolute, expected_absolute,
                        "Expected is_absolute {}, got {}",
                        expected_absolute, parsed_path.is_absolute
                    );
                }
                Err(e) => {
                    panic!("Failed to parse path '{}': {:?}", input, e);
                }
            }
        }
    }
}

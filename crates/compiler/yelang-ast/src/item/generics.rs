/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 11/12/2025
 */

use crate::{Expr, GenericArgs, Ident, Path, T, Type};
use yelang_lexer::{
    ParseTokenStream, SeparatedList, Span, TokenResult, TokenStream,
    helper_types::{ArrayCreator, Verify},
    match_map,
};

/// Generic parameters and where clause for generic items
///
/// # Example
/// ```
/// fn process<T, U>(x: T, y: U) -> T
/// where
///     T: Clone,
///     U: Display
/// { ... }
/// ```
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Generics {
    /// Generic parameters (type params, lifetimes, const params)
    pub params: Vec<GenericParam>,
    /// Optional where clause
    pub where_clause: Option<WhereClause>,
    /// Span of the entire generics
    pub span: Span,
}

// We do NOT implement ParseTokenStream for Generics directly anymore.
// This forces the caller to construct it manually, ensuring they handle the where-clause.

/// The Parser Helper (The primitive)
/// Parses: < T, U, V >
pub type GenericParamsParser = ArrayCreator<T![<], GenericParam, T![,], T![>]>;

/// Type binder parameters for higher-ranked binders.
///
/// This language currently exposes type and const binders (no lifetimes).
///
/// # Example
/// ```
/// for<T>
/// for<T, U>
/// for<const N: usize>
/// for<T, const N: usize>
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct TypeBinderParams {
    pub params: Vec<TypeBinderParam>,
    pub span: Span,
}

/// A type binder parameter: `T` or `T: Bound + Bound`.
///
/// Note: unlike item-level type params, binder type params do not support defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeBinderTyParam {
    pub name: Ident,
    pub bounds: Vec<TraitBound>,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for TypeBinderTyParam {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();
        let (name, bounds) = stream.parse::<(
            Ident,
            Option<(T![:], SeparatedList<TraitBound, T![+], false>)>,
        )>()?;

        Ok(TypeBinderTyParam {
            name,
            bounds: bounds
                .map(|(_, list)| list.value_owned())
                .unwrap_or_default(),
            span: stream.span_since(checkpoint),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeBinderParam {
    Type(TypeBinderTyParam),
    Const(ConstBinderParam),
}

impl TypeBinderParam {
    pub fn span(&self) -> Span {
        match self {
            TypeBinderParam::Type(p) => p.span,
            TypeBinderParam::Const(c) => c.span,
        }
    }
}

/// Const binder parameter: `const N: usize`.
///
/// Note: unlike item-level const generics, binders do not support defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstBinderParam {
    pub name: Ident,
    pub ty: Type,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for ConstBinderParam {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let ((_const, name, _colon, ty), span) =
            stream.parse_with_span::<(T![const], Ident, T![:], Type)>()?;
        Ok(ConstBinderParam { name, ty, span })
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for TypeBinderParam {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let res = match_map!(
            stream,
            ConstBinderParam => Self::Const,
            TypeBinderTyParam => Self::Type,
        )?;
        Ok(res)
    }
}

impl TypeBinderParams {
    pub fn iter_ty_params(&self) -> impl Iterator<Item = &TypeBinderTyParam> {
        self.params.iter().filter_map(|p| match p {
            TypeBinderParam::Type(p) => Some(p),
            TypeBinderParam::Const(_) => None,
        })
    }

    pub fn iter_const_params(&self) -> impl Iterator<Item = &ConstBinderParam> {
        self.params.iter().filter_map(|p| match p {
            TypeBinderParam::Type(_) => None,
            TypeBinderParam::Const(c) => Some(c),
        })
    }

    pub fn num_ty_params(&self) -> usize {
        self.iter_ty_params().count()
    }

    pub fn num_const_params(&self) -> usize {
        self.iter_const_params().count()
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for TypeBinderParams {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();
        let (_l, params, _r) =
            stream.parse::<(T![<], SeparatedList<TypeBinderParam, T![,], true>, T![>])>()?;
        Ok(TypeBinderParams {
            params: params.items(),
            span: stream.span_since(checkpoint),
        })
    }
}

/// A generic parameter
///
/// Can be a type parameter, lifetime, or const parameter
#[derive(Debug, Clone, PartialEq)]
pub enum GenericParam {
    /// Type parameter: `T`, `T: Clone`, `T = i32`
    ///
    /// # Example
    /// ```
    /// fn foo<T>() { }
    /// fn bar<T: Clone>() { }
    /// fn baz<T = i32>() { }
    /// ```
    Type(TypeParam),

    // /// Lifetime parameter: `'a`, `'a: 'b`
    // ///
    // /// # Example
    // /// ```
    // /// fn foo<'a>(x: &'a str) { }
    // /// fn bar<'a: 'b, 'b>(x: &'a str, y: &'b str) { }
    // /// ```
    // Lifetime(LifetimeParam),
    /// Const parameter: `const N: usize`, `const N: usize = 10`
    ///
    /// # Example
    /// ```
    /// fn foo<const N: usize>() { }
    /// struct Array<T, const N: usize> { ... }
    /// ```
    Const(ConstParam),
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for GenericParam {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        // Try ConstParam first since it has more specific leading token (const keyword)
        let res = match_map!(
                stream,
                ConstParam => Self::Const,
                TypeParam => Self::Type,
        )?;

        Ok(res)
    }
}

/// Type parameter with optional bounds and default
///
/// # Example
/// ```
/// T
/// T: Clone
/// T: Clone + Debug
/// T = i32
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct TypeParam {
    /// Parameter name
    pub name: Ident,
    /// Trait bounds
    pub bounds: Vec<TraitBound>,
    /// Default type
    pub default: Option<Type>,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for TypeParam {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();
        let (_, name, bounds, default) = stream.parse::<(
            // Ensure we're not parsing "const" as a type parameter name
            yelang_lexer::helper_types::PeekNot<T![const]>,
            // T
            Ident,
            // Optional bounds: : Trait + Trait
            Option<(T![:], SeparatedList<TraitBound, T![+], false>)>,
            // Optional default: = Type
            Option<(T![=], Type)>,
        )>()?;

        let span = stream.span_since(checkpoint);

        Ok(TypeParam {
            name,
            bounds: bounds
                .map(|(_, list)| list.value_owned())
                .unwrap_or_default(),
            default: default.map(|(_, ty)| ty),
            span,
        })
    }
}

// /// Lifetime parameter with optional bounds
// ///
// /// # Example
// /// ```
// /// 'a
// /// 'a: 'b
// /// ```
// #[derive(Debug, Clone, PartialEq)]
// pub struct LifetimeParam {
//     /// Lifetime name (e.g., 'a, 'static)
//     pub name: Ident,
//     /// Lifetime bounds
//     pub bounds: Vec<Lifetime>,
//     pub span: Span,
// }

/// Const parameter with type and optional default
///
/// # Example
/// ```
/// const N: usize
/// const N: usize = 10
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ConstParam {
    /// Parameter name
    pub name: Ident,
    /// Type of the const parameter
    pub ty: Type,
    /// Default value
    pub default: Option<Expr>,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for ConstParam {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let ((_const, name, _colon, ty, default), span) = stream.parse_with_span::<(
            T![const],
            Ident,
            T![:],
            Type,
            // Optional default: = AtomicExpr (not full Expr to avoid > ambiguity)
            Option<(T![=], crate::expr::AtomicExpr)>,
        )>()?;

        Ok(ConstParam {
            name,
            ty,
            default: default.map(|(_, atomic)| atomic.as_expr()),
            span,
        })
    }
}

/// Trait bound
///
/// # Example
/// ```
/// Clone
/// Debug + Display
/// Iterator<Item = i32>
/// Iterator (no args)
/// Iterator<Item = T> (angle-bracketed args)
/// Fn(i32) -> bool (parenthesized args)
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct TraitBound {
    /// Optional higher-ranked binder: `for<T> Trait<...>`.
    pub binder: Option<TypeBinderParams>,
    /// Path to the trait
    pub path: Path,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for TraitBound {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let binder = if stream.parse::<Verify<T![for]>>().is_ok() {
            let (_for, params) = stream.parse::<(T![for], TypeBinderParams)>()?;
            Some(params)
        } else {
            None
        };

        // Parse the Path (may include angle-bracketed args) then optionally attach
        // parenthesized args `(...) -> ...` for function traits.
        //
        // IMPORTANT: we do not use `Option<ParenthesizedArgs>` because `Option<T>`
        // swallows *any* parse error. We only parse parenthesized args if `(` is present.
        let checkpoint = stream.checkpoint();
        let mut path = stream.parse::<Path>()?;

        let span = stream.span_since(checkpoint);

        if stream.parse::<Verify<T!['(']>>().is_ok() {
            let paren_args = stream.parse::<ParenthesizedArgs>()?;
            if let Some(last_segment) = path.segments.last_mut() {
                if last_segment.args.is_some() {
                    return Err(yelang_lexer::TokenError::SyntaxError {
                        message: "Path segment cannot have both angle-bracketed and parenthesized arguments"
                            .to_string(),
                        span,
                        source: None,
                    });
                }
                last_segment.args = Some(GenericArgs::Parenthesized(paren_args));
            }
        }
        Ok(TraitBound { binder, path, span })
    }
}

/// Parenthesized generic arguments for function traits
/// This is a syntactic sugar for function traits like `Fn`
/// E.g., the (A, N) -< C in `Fn(A, B) -> C`)
/// # Example
/// ```
/// i32 and String is ins, bool is out for Fn(i32, String) -> bool
/// Fn(i32)
/// Fn() -> Result<T, E>
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ParenthesizedArgs {
    pub ins: Vec<Type>,
    pub out: Option<Type>,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for ParenthesizedArgs {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();
        let (_, inp, _, out) = stream.parse::<(
            T!['('],
            SeparatedList<Type, T![,], true>,
            T![')'],
            Option<(T![->], Type)>,
        )>()?;

        Ok(ParenthesizedArgs {
            ins: inp.items(),
            out: out.map(|(_, ty)| ty),
            span: stream.span_since(checkpoint),
        })
    }
}

/// Where clause with predicates
///
/// # Example
/// ```
/// where
///     T: Clone,
///     U: Display,
///     'a: 'b
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct WhereClause {
    /// List of predicates
    pub predicates: Vec<WherePredicate>,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for WhereClause {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let ((_where, predicates), span) = stream.parse_with_span::<(
            T![where],
            // e.g., T: Clone, U = i32
            SeparatedList<WherePredicate, T![,], true>,
        )>()?;
        Ok(WhereClause {
            predicates: predicates.items(),
            span,
        })
    }
}

impl WhereClause {}

/// A predicate in a where clause
#[derive(Debug, Clone, PartialEq)]
pub enum WherePredicate {
    /// Higher-ranked predicate: `for<T> (predicate)`
    ///
    /// # Example
    /// ```
    /// where for<T> T: Clone
    /// where for<T, U> (T: Into<U>)
    /// ```
    ForAll {
        params: TypeBinderParams,
        predicate: Box<WherePredicate>,
        span: Span,
    },

    /// Trait bound: `T: Clone`
    ///
    /// # Example
    /// ```
    /// where T: Clone + Debug
    /// ```
    TraitBound { ty: Type, bounds: Vec<TraitBound> },

    // NOTE: Probably wont support this at all
    /// Type equality: `T = i32`
    ///
    /// # Example
    /// ```
    /// where T::Item = i32
    /// ```
    TypeEq { lhs: Type, rhs: Type },
    // /// Lifetime bound: `'a: 'b`
    // ///
    // /// # Example
    // /// ```
    // /// where 'a: 'b + 'c
    // /// ```
    // LifetimeBound {
    //     lifetime: Lifetime,
    //     bounds: Vec<Lifetime>,
    // },
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for WherePredicate {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        // Higher-ranked binder form: `for<...> <where-predicate>`.
        // This must be checked first to avoid mis-parsing `for` as a type/path.
        if stream.parse::<Verify<T![for]>>().is_ok() {
            let checkpoint = stream.checkpoint();
            let (_for, params, predicate) =
                stream.parse::<(T![for], TypeBinderParams, WherePredicate)>()?;
            return Ok(WherePredicate::ForAll {
                params,
                predicate: Box::new(predicate),
                span: stream.span_since(checkpoint),
            });
        }

        // Try to parse T: Bounds or T = Type
        // For now, assume T: Bounds
        type TB = (Type, T![:], SeparatedList<TraitBound, T![+], false>);

        let res = match_map!(
            stream,
            TB => |(ty, _, bounds)| {
                    WherePredicate::TraitBound { ty, bounds: bounds.items() }
                },
            (Type, T![=], Type) => |(lhs, _eq, rhs)| WherePredicate::TypeEq { lhs, rhs },
        )?;

        Ok(res)
    }
}

// /// Lifetime identifier
// ///
// /// # Example
// /// ```
// /// 'a
// /// 'static
// /// 'lifetime
// /// ```
// #[derive(Debug, Clone, PartialEq)]
// pub struct Lifetime {
//     /// Lifetime name (e.g., "a" for 'a)
//     pub name: Ident,
//     pub span: Span,
// }

#[cfg(test)]
mod tests {
    use super::*;
    use yelang_interner::Interner;

    #[test]
    fn test_const_param_with_default_parsing() {
        let mut interner = Interner::new();
        let source = "const N: usize = 5";
        let mut stream = crate::tokens::TokenKind::tokenize(source, &mut interner).unwrap();

        match stream.parse::<ConstParam>() {
            Ok(param) => {
                eprintln!("✓ Successfully parsed ConstParam: {:?}", param.name);
                eprintln!("  Type: {:?}", param.ty);
                eprintln!("  Default: {:?}", param.default);
                assert!(param.default.is_some(), "Should have default value");
            }
            Err(e) => {
                eprintln!("✗ Failed to parse ConstParam: {:?}", e);
                panic!("ConstParam parsing failed");
            }
        }
    }

    #[test]
    fn test_generic_param_with_const_default() {
        let mut interner = Interner::new();
        let source = "const SIZE: usize = 1024";
        let mut stream = crate::tokens::TokenKind::tokenize(source, &mut interner).unwrap();

        match stream.parse::<GenericParam>() {
            Ok(param) => {
                eprintln!("✓ Successfully parsed GenericParam");
                assert!(
                    matches!(param, GenericParam::Const(_)),
                    "Should be Const variant"
                );
                // Check what's left in the stream
                if let Some(next_tok) = stream.peek() {
                    eprintln!("  ✗ ERROR: Next token after GenericParam: {:?}", next_tok);
                    panic!(
                        "There should be no tokens left after parsing 'const SIZE: usize = 1024'!"
                    );
                } else {
                    eprintln!("  ✓ All tokens consumed correctly");
                }
            }
            Err(e) => {
                eprintln!("✗ Failed to parse GenericParam: {:?}", e);
                panic!("GenericParam parsing failed");
            }
        }
    }

    #[test]
    fn test_type_parsing_stops_at_equals() {
        let mut interner = Interner::new();
        let source = "usize = 1024";
        let mut stream = crate::tokens::TokenKind::tokenize(source, &mut interner).unwrap();

        match stream.parse::<Type>() {
            Ok(ty) => {
                eprintln!("✓ Successfully parsed Type: {:?}", ty);
                // Check what token is next
                if let Some(next_tok) = stream.peek() {
                    eprintln!("  Next token after Type: {:?}", next_tok);
                } else {
                    eprintln!("  No more tokens after Type");
                }
            }
            Err(e) => {
                eprintln!("✗ Failed to parse Type: {:?}", e);
                panic!("Type parsing failed");
            }
        }
    }

    #[test]
    fn test_type_param_rejects_const_keyword() {
        let mut interner = Interner::new();
        let source = "const SIZE: usize = 1024";
        let mut stream = crate::tokens::TokenKind::tokenize(source, &mut interner).unwrap();

        match stream.parse::<TypeParam>() {
            Ok(param) => {
                eprintln!("✗ TypeParam incorrectly accepted 'const' keyword!");
                eprintln!("  Parsed as: {:?}", param.name);
                panic!("TypeParam should reject const keyword");
            }
            Err(e) => {
                eprintln!("✓ TypeParam correctly rejected 'const' keyword");
                eprintln!("  Error: {:?}", e);
            }
        }
    }

    #[test]
    fn test_expr_parsing_with_greater_than() {
        let mut interner = Interner::new();
        let source = "1024 >";
        let mut stream = crate::tokens::TokenKind::tokenize(source, &mut interner).unwrap();

        match stream.parse::<Expr>() {
            Ok(expr) => {
                eprintln!("✓ Successfully parsed Expr");
                // Check what's left
                if let Some(next_tok) = stream.peek() {
                    eprintln!("  Next token: {:?}", next_tok);
                    if format!("{:?}", next_tok).contains("Token { kind: >") {
                        eprintln!("  ✓ Correctly stopped at '>'");
                    } else {
                        panic!("Expected '>' token but got something else");
                    }
                } else {
                    panic!("No tokens left - Expr consumed the '>'!");
                }
            }
            Err(e) => {
                eprintln!("✗ Failed to parse Expr: {:?}", e);
                panic!("Expr parsing failed");
            }
        }
    }

    #[test]
    fn test_trait_bound_parses_assoc_type_binding_args() {
        let mut interner = Interner::new();
        let source = "Foo<Item = i64>";
        let mut stream = crate::tokens::TokenKind::tokenize(source, &mut interner).unwrap();

        let bound = stream
            .parse::<TraitBound>()
            .expect("expected TraitBound to parse");
        assert_eq!(bound.path.segments.len(), 1);

        assert!(
            stream.is_eof(),
            "expected full consumption, next: {:?}",
            stream.peek()
        );
    }

    #[test]
    fn test_generic_params_parser_with_const_default() {
        let mut interner = Interner::new();
        let source = "<const SIZE: usize = 1024>";
        let mut stream = crate::tokens::TokenKind::tokenize(source, &mut interner).unwrap();

        match stream.parse::<GenericParamsParser>() {
            Ok(params) => {
                eprintln!("✓ Successfully parsed GenericParamsParser");
                eprintln!("  Params count: {}", params.items().len());
                assert_eq!(params.items().len(), 1, "Should have 1 generic param");
            }
            Err(e) => {
                eprintln!("✗ Failed to parse GenericParamsParser: {:?}", e);
                panic!("GenericParamsParser parsing failed");
            }
        }
    }
}

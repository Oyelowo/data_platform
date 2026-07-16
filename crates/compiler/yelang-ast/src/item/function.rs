/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 11/12/2025
 */

use super::generics::GenericParamsParser;
use crate::pattern::RestrictedPattern;
use crate::tokenizer::tokens::TokenKind;
use crate::{BlockExpr, Generics, Ident, Literal, Pattern, T, Type, WhereClause};
use yelang_lexer::{
    ArrayCreator, Either, ParseTokenStream, SeparatedList, Span, TokenResult, TokenStream,
};

/// Parse an optional `extern "ABI"` prefix and return the ABI string.
///
/// If `extern` is present but not followed by a string literal, returns an
/// error. If `extern` is absent, returns `Ok(None)`.
fn parse_optional_abi(stream: &mut TokenStream<TokenKind>) -> TokenResult<Option<String>> {
    let checkpoint = stream.checkpoint();
    if stream.parse::<Option<T![extern]>>()?.is_none() {
        return Ok(None);
    }

    let lit = stream.parse::<Literal>()?;
    if let Literal::Str(s) = lit {
        Ok(Some(stream.interner().resolve(&s.value).to_string()))
    } else {
        Err(yelang_lexer::TokenError::UnexpectedToken {
            expected: "string literal ABI".into(),
            found: "non-string literal".into(),
            span: stream.span_since(checkpoint),
        })
    }
}

/// Function definition
///
/// # Example
/// ```
/// fn add(x: i32, y: i32) -> i32 {
///     x + y
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct FnDef {
    // /// Attributes on the function
    // pub attributes: Vec<Attribute>,
    /// Function name
    pub name: Ident,
    /// Generic parameters
    pub generics: Generics,
    /// Function signature
    pub sig: FnSig,
    /// Function body
    pub body: BlockExpr,

    /// Whether the function is declared `const`.
    pub is_const: bool,
    /// Visibility
    // pub visibility: Visibility,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for FnDef {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();

        // Accept modifiers in either order: `async const fn` or `const async fn`.
        let mut is_async = false;
        let mut is_const = false;
        loop {
            match stream.peek().map(|t| t.kind()) {
                Some(TokenKind::Async) if !is_async => {
                    let _ = stream.parse::<T![async]>()?;
                    is_async = true;
                }
                Some(TokenKind::Const) if !is_const => {
                    let _ = stream.parse::<T![const]>()?;
                    is_const = true;
                }
                _ => break,
            }
        }

        let abi = parse_optional_abi(stream)?;

        let (_fn, name, gen_params, mut sig, where_clause, body) = stream.parse::<(
            T![fn],
            Ident,
            Option<GenericParamsParser>,
            FnSig,
            Option<WhereClause>,
            BlockExpr,
        )>()?;

        // Construct the Generics AST node manually
        let generics = Generics {
            params: gen_params.map(|g| g.items_owned()).unwrap_or_default(),
            where_clause, // In functions, where clause comes from position 5
            span: stream.span_since(checkpoint), // Approximate span
        };

        sig.is_async = is_async;
        sig.abi = abi;

        Ok(FnDef {
            name,
            generics,
            sig,
            body,
            is_const,
            span: stream.span_since(checkpoint),
        })
    }
}

/// Method definition (in trait or impl)
///
/// # Example
/// ```
/// fn process(&self, x: i32) -> bool;
/// fn create() -> Self;
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Method {
    /// Method name
    pub segment: Ident,
    /// Generic parameters
    pub generics: Generics,
    /// Function signature
    pub sig: FnSig,
    /// Optional body (None for trait declarations)
    pub body: Option<BlockExpr>,

    /// Whether the method is declared `const`.
    pub is_const: bool,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Method {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();

        // Keep the surface syntax consistent with `FnDef`: signature first, then optional `where`.
        // This enables Rust-like trait method declarations:
        // `fn foo<T>(...) -> R where T: Bound;`
        // Accept modifiers in either order: `async const fn` or `const async fn`.
        let mut is_async = false;
        let mut is_const = false;
        loop {
            match stream.peek().map(|t| t.kind()) {
                Some(TokenKind::Async) if !is_async => {
                    let _ = stream.parse::<T![async]>()?;
                    is_async = true;
                }
                Some(TokenKind::Const) if !is_const => {
                    let _ = stream.parse::<T![const]>()?;
                    is_const = true;
                }
                _ => break,
            }
        }

        let abi = parse_optional_abi(stream)?;

        let (_fn, segment, gen_params, mut sig, where_clause, body) = stream.parse::<(
            T![fn],
            Ident,
            Option<GenericParamsParser>,
            FnSig,
            Option<WhereClause>,
            Option<BlockExpr>,
        )>()?;

        // Construct the Generics AST node manually
        let generics = Generics {
            params: gen_params.map(|g| g.items_owned()).unwrap_or_default(),
            where_clause,
            span: stream.span_since(checkpoint),
        };

        sig.is_async = is_async;
        sig.abi = abi;

        Ok(Method {
            segment,
            generics,
            sig,
            body,
            is_const,
        })
    }
}

/// Function signature
///
/// # Example
/// ```
/// fn add(x: i32, y: i32) -> i32
/// async fn fetch() -> Result<string, Error>
/// extern "C" fn call_c()
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct FnSig {
    /// Function parameters
    pub params: Vec<Param>,
    /// Return type
    pub return_type: FnRefType,
    /// Whether function is async
    pub is_async: bool,
    /// Whether function accepts variable arguments
    pub is_variadic: bool,
    /// Optional ABI string for `extern "ABI" fn` items and function pointer types.
    pub abi: Option<String>,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for FnSig {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        // For simplicity, parse params in parens, then optional return type
        let (params, ret) = stream.parse::<(
            ArrayCreator<T!['('], Param, T![,], T![')']>,
            Option<(T![->], Type)>,
        )>()?;

        Ok(FnSig {
            params: params.items_owned(),
            return_type: ret
                .map(|(_, ty)| FnRefType::Type(ty))
                .unwrap_or(FnRefType::Default(stream.span())),
            is_async: false,
            is_variadic: false,
            abi: None,
        })
    }
}

/// () for fn, inference for lambda
#[derive(Debug, Clone, PartialEq)]
pub enum FnRefType {
    Type(Type),
    Default(Span),
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for FnRefType {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        type Def = (T!['('], T![')']);
        type RetT = Either<Def, Type>;

        let (ret, span) = stream.parse_with_span::<RetT>()?;
        match ret {
            Either::Left((_, _)) => Ok(FnRefType::Default(span)),
            Either::Right(ty) => Ok(FnRefType::Type(ty)),
        }
    }
}

/// Function parameter
///
/// # Example
/// ```
/// x: i32
/// mut y: string
/// (a, b): (i32, i32)
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    /// Parameter pattern (can be complex for destructuring)
    pub pattern: Pattern,
    /// Parameter type
    pub ty: Type,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Param {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        use yelang_lexer::match_map;

        let (param, span) = stream.parse_with_span::<ParamInner>()?;

        Ok(Param {
            pattern: param.pattern,
            ty: param.ty,
            span,
        })
    }
}

/// Helper type for parsing parameter internals
struct ParamInner {
    pattern: Pattern,
    ty: Type,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SelfParamRef {
    is_ref: bool,
    is_mut: bool,
}

struct SelfNameParam {
    span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for SelfNameParam {
    fn parse(stream: &mut TokenStream<crate::TokenKind>) -> TokenResult<Self> {
        let (self_, span) = stream.parse_with_span::<T![self]>()?;

        Ok(Self { span })
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for ParamInner {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        use yelang_lexer::match_map;

        match_map!(
            stream,
            // [&] [mut] self - all self parameter variations
            (Option<T![&]>, Option<T![mut]>, SelfNameParam) => |(reference, mutability, self_)| {
                let interner = stream.interner();
                let is_mutable = mutability.is_some();
                Self::create_self_param(
                    self_.span,
                    interner,
                    SelfParamRef {
                        is_ref: reference.is_some(),
                        is_mut: is_mutable,
                    },
                    if is_mutable {
                        crate::Mutability::Mutable
                    } else {
                        crate::Mutability::Immutable
                    }
                )
            },
            // Regular parameter: pattern: Type
            (RestrictedPattern, T![:], Type) => |(pattern, _, ty)| {
                ParamInner {
                    pattern: pattern.0,
                    ty,
                }
            },
        )
    }
}

impl ParamInner {
    fn create_self_param(
        span: Span,
        interner: &crate::Interner,
        self_ref: SelfParamRef,
        mutability: crate::Mutability,
    ) -> Self {
        let self_ident = crate::Ident::new(interner.get_or_intern("self"), span);

        let self_pattern = Pattern {
            pattern: crate::PatternKind::Binding {
                name: self_ident,
                mutability,
                subpattern: None,
            },
            span,
        };

        let self_type = Self::create_self_type(span, interner, self_ref);

        ParamInner {
            pattern: self_pattern,
            ty: self_type,
        }
    }

    fn create_self_type(span: Span, interner: &crate::Interner, self_ref: SelfParamRef) -> Type {
        let self_path = crate::Path {
            qself: None,
            segments: vec![crate::PathSegment {
                ident: crate::Ident::new(interner.get_or_intern("Self"), span),
                args: None,
            }],
            is_absolute: false,
            span,
        };

        let self_ty = Type {
            kind: crate::TypeKind::Named(self_path),
            span,
        };

        if self_ref.is_ref {
            Type {
                kind: crate::TypeKind::Ref {
                    ty: Box::new(self_ty),
                    is_mut: self_ref.is_mut,
                },
                span,
            }
        } else {
            self_ty
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Interner, TokenKind};

    // Self parameter parsing is tested in e2e_hir_pipeline_test::test_trait_implementation.
    // This test is narrowly targeted at where-clause parsing with const generic args.
    #[test]
    fn parses_fn_where_trait_bound_with_const_generic_arg() {
        let src = r#"fn main(x: Bar) -> i64
where
    Bar: FooTrait<3>
{
    0
}"#;

        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(src, &mut interner).expect("tokenize");

        let parsed = stream.parse::<FnDef>();
        assert!(parsed.is_ok(), "parse error: {:?}", parsed.err());
        assert!(stream.is_eof(), "expected EOF after parsing FnDef");
    }

    #[test]
    fn parses_extern_abi_function() {
        let src = r#"extern "C-unwind" fn foo(x: i32) -> i32 { x }"#;
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(src, &mut interner).expect("tokenize");
        let func = stream.parse::<FnDef>().expect("parse fn");
        assert_eq!(func.sig.abi.as_deref(), Some("C-unwind"));
        assert!(stream.is_eof());
    }

    #[test]
    fn extern_abi_codegen_renders() {
        let src = r#"extern "C" fn foo(x: i32) -> i32 { x }"#;
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(src, &mut interner).expect("tokenize");
        let func = stream.parse::<FnDef>().expect("parse fn");
        let mut buf = String::new();
        crate::Codegen::codegen(&func, &mut buf, &interner).unwrap();
        assert!(buf.contains("extern \"C\" "), "rendered: {buf}");
        assert!(buf.contains("fn foo"), "rendered: {buf}");
    }
}

// use crate::ast::{StructuralField, TypeOperator};
use crate::Codegen;
use crate::item::TypeBinderParams;
use crate::{FunctionType, StructuralField, TypeOperator};
use crate::{Literal, T};
use crate::{
    Path,
    expr::{Expr, MacroInvocation, parse_macro_args},
};
use yelang_lexer::{ParseTokenStream, SeparatedList, Span, TokenResult, TokenStream, match_map};

#[derive(Debug, Clone, PartialEq)]
pub struct Type {
    pub kind: TypeKind,
    pub span: Span,
}

impl Type {
    pub fn span(&self) -> &Span {
        &self.span
    }
}

/// Type annotations used in signatures, variable declarations, etc.
///
/// Represents the complete type system for the language, including primitives,
/// generics, functions, and advanced type operators.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeKind {
    // /// Primitive types: `i32`, `bool`, `string`
    // ///
    // /// # Example
    // /// ```
    // /// let x: i32 = 42;
    // /// let flag: bool = true;
    // /// ```
    // Primitive(super::PrimitiveType),
    /// Named types with optional generics: `Vec<i32>`, `Result<T, E>`
    ///
    /// Generics are stored in the Path segments themselves (in PathSegment.args),
    /// not duplicated here. This follows rustc and rust-analyzer's HIR design.
    ///
    /// # Example
    /// ```
    /// let items: Vec<i32> = vec![1, 2, 3];
    /// let result: Result<string, Error> = Ok("success");
    /// ```
    Named(Path),

    /// Tuple types: `(i32, bool)`, `(string,)`
    ///
    /// # Example
    /// ```
    /// let pair: (i32, string) = (42, "answer");
    /// ```
    Tuple(Vec<Type>),

    /// Array types: `[i32; 10]`, `[T; N]`
    ///
    /// # Example
    /// ```
    /// let arr: [i32; 5] = [1, 2, 3, 4, 5];
    /// ```
    Array(Box<Type>, Box<Expr>),

    /// Slice types: `[i32]`, `[string]`
    ///
    /// # Example
    /// ```
    /// let slice: [i32] = [1, 2, 3];
    /// ```
    Slice(Box<Type>),

    /// Reference types: `&T`, `&mut T`
    Ref { ty: Box<Type>, is_mut: bool },

    /// Function types: `fn(i32) -> i32`, `fn() -> bool`
    ///
    /// # Example
    /// ```
    /// let add: fn(i32, i32) -> i32 = |a, b| a + b;
    /// ```
    Function(FunctionType),

    /// First-class higher-ranked polymorphic types: `for<T> Type`.
    ///
    /// This is the *type-level* `forall` form (rank-2+). It is distinct from
    /// let-polymorphism (`Scheme`) and from where-clause `for<T> ...` predicates.
    ///
    /// # Example
    /// ```
    /// let id: for<T> fn(T) -> T = identity;
    /// ```
    ForAll {
        params: TypeBinderParams,
        ty: Box<Type>,
    },

    /// Never type: `!`
    ///
    /// Represents computations that never return (e.g., `panic!()`, infinite loops)
    ///
    /// # Example
    /// ```
    /// fn crash() -> ! {
    ///     panic!("This never returns");
    /// }
    /// ```
    Never,

    /// Inference placeholder: `_`
    ///
    /// Allows the type checker to infer the type automatically
    ///
    /// # Example
    /// ```
    /// let x: _ = 42; // Type inferred as i32
    /// ```
    Infer,

    /// String literal type: `"success"`
    ///
    /// This is a *type-level* string literal.
    ///
    /// # Example
    /// ```
    /// let status: "pending" = "pending";
    /// ```
    ///
    /// A union like `"pending" | "active"` is represented as `TypeKind::Union`
    /// containing multiple `TypeKind::Literal` members.
    Literal(Literal),

    /// Structural types: `{ id: i32, name: string }`
    ///
    /// Anonymous record types with named fields
    ///
    /// # Example
    /// ```
    /// let point: { x: i32, y: i32 } = { x: 10, y: 20 };
    /// ```
    Structural(Vec<StructuralField>),

    /// Union types: `i32 | string | bool`
    ///
    /// Unions like `"a" | "b"` are represented as unions of `TypeKind::Literal`.
    Union(Vec<Type>),

    /// Type operators: `typeof expr`, `ReturnType<typeof fn>`
    ///
    /// Advanced type-level operations for type inference and manipulation
    ///
    /// # Example
    /// ```
    /// let x = 42;
    /// let y: typeof x = 100; // y is i32
    /// ```
    Operator(TypeOperator),

    /// Opaque return type: `impl Trait`.
    ///
    /// This denotes a single, hidden concrete type chosen by the function body
    /// (per-function, per-occurrence), but only exposes that it implements the
    /// specified trait.
    ImplTrait(Path),

    /// Trait object type: `dyn Trait`.
    DynTrait(Path),

    /// Macro invocation in type position: `MyType!()`.
    MacroInvocation(super::super::expr::MacroInvocation),

    /// Error type for parser recovery
    Error,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Type {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();

        fn parse_type_atom(
            stream: &mut TokenStream<crate::tokenizer::TokenKind>,
        ) -> TokenResult<Type> {
            let checkpoint = stream.checkpoint();

            let ref_checkpoint = stream.checkpoint();
            if stream.parse::<Option<T![&]>>()?.is_some() {
                let is_mut = stream.parse::<Option<T![mut]>>()?.is_some();
                let inner = parse_type_atom(stream)?;
                return Ok(Type {
                    kind: TypeKind::Ref {
                        ty: Box::new(inner),
                        is_mut,
                    },
                    span: stream.span_since(ref_checkpoint),
                });
            }
            stream.restore(ref_checkpoint);

            // String literal types: `"a"`
            // (Other literal kinds are not valid in type position.)
            let literal_checkpoint = stream.checkpoint();
            if let Ok(lit) = stream.parse::<Literal>() {
                if matches!(lit, Literal::Str(_)) {
                    return Ok(Type {
                        kind: TypeKind::Literal(lit),
                        span: stream.span_since(checkpoint),
                    });
                }
            }
            stream.restore(literal_checkpoint);

            // Parenthesized type grouping: `(T)` is just `T` (span includes parens).
            // Tuples are written `(T, U)` or `(T,)`.
            let paren_checkpoint = stream.checkpoint();
            if stream.parse::<Option<T!['(']>>()?.is_some() {
                let list = stream.parse::<SeparatedList<Type, T![,], true>>()?;
                stream.parse::<T![')']>()?;

                let outer_span = stream.span_since(paren_checkpoint);
                let sep_count = list.separator_count();
                let mut items = list.value_owned();

                if items.len() == 1 && sep_count == 0 {
                    let mut inner = items.remove(0);
                    inner.span = outer_span;
                    return Ok(inner);
                }

                return Ok(Type {
                    kind: TypeKind::Tuple(items),
                    span: outer_span,
                });
            }
            stream.restore(paren_checkpoint);

            let kind = match_map!(stream,
                (T![for], TypeBinderParams, Type) => |(_for, params, ty)| TypeKind::ForAll { params, ty: Box::new(ty) },
                (T!['{'], SeparatedList<StructuralField, T![,], true>, T!['}']) => |(_l, fields, _r)| TypeKind::Structural(fields.value_owned()),
                (T!['['], Type, T![;], Expr, T![']']) => |(_l, ty, _s, expr, _r)| TypeKind::Array(Box::new(ty), Box::new(expr)),
                (T!['['], Type, T![']']) => |(_l, ty, _r)| TypeKind::Slice(Box::new(ty)),
                FunctionType => TypeKind::Function,
                T![!] => |_| TypeKind::Never,
                T!["_"] => |_| TypeKind::Infer,
                TypeOperator => TypeKind::Operator,
                (T![impl], Path) => |(_, path)| TypeKind::ImplTrait(path),
                (T![dyn], Path) => |(_, path)| TypeKind::DynTrait(path),
                Path => |path| TypeKind::Named(path)
            )?;

            // A path followed by `!` is a type-position macro invocation.
            let kind = if let TypeKind::Named(path) = kind {
                if stream.peek().map(|t| t.kind()) == Some(&crate::tokenizer::TokenKind::Bang) {
                    stream.advance(); // consume `!`
                    let args = parse_macro_args(stream)?;
                    TypeKind::MacroInvocation(MacroInvocation {
                        path,
                        args,
                        span: stream.span_since(checkpoint),
                    })
                } else {
                    TypeKind::Named(path)
                }
            } else {
                kind
            };

            Ok(Type {
                kind,
                span: stream.span_since(checkpoint),
            })
        }

        // General unions: `T | U | V`
        // Parse as a sequence of type atoms separated by `|`.
        let first = parse_type_atom(stream)?;
        let sep_checkpoint = stream.checkpoint();
        if stream.parse::<T![|]>().is_ok() {
            let mut types = vec![first];
            loop {
                let next = parse_type_atom(stream)?;
                types.push(next);

                let next_sep_checkpoint = stream.checkpoint();
                if stream.parse::<T![|]>().is_ok() {
                    continue;
                }
                stream.restore(next_sep_checkpoint);
                break;
            }

            return Ok(Self {
                kind: TypeKind::Union(types),
                span: stream.span_since(checkpoint),
            });
        }
        stream.restore(sep_checkpoint);

        Ok(Self {
            kind: first.kind,
            span: stream.span_since(checkpoint),
        })
    }
}

/// Parses a single type atom (i.e. a type without top-level `|` unions).
///
/// This is useful in contexts where `|` is a delimiter (e.g. lambda parameter lists)
/// and must not be consumed as a union-type operator.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeAtom(pub Type);

impl ParseTokenStream<crate::tokenizer::TokenKind> for TypeAtom {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();

        let ref_checkpoint = stream.checkpoint();
        if stream.parse::<Option<T![&]>>()?.is_some() {
            let is_mut = stream.parse::<Option<T![mut]>>()?.is_some();
            let inner = stream.parse::<TypeAtom>()?;
            return Ok(TypeAtom(Type {
                kind: TypeKind::Ref {
                    ty: Box::new(inner.0),
                    is_mut,
                },
                span: stream.span_since(ref_checkpoint),
            }));
        }
        stream.restore(ref_checkpoint);

        // String literal types: `"a"`
        // (Other literal kinds are not valid in type position.)
        let literal_checkpoint = stream.checkpoint();
        if let Ok(lit) = stream.parse::<Literal>() {
            if matches!(lit, Literal::Str(_)) {
                return Ok(TypeAtom(Type {
                    kind: TypeKind::Literal(lit),
                    span: stream.span_since(checkpoint),
                }));
            }
        }
        stream.restore(literal_checkpoint);

        // Parenthesized type grouping: `(T)` is just `T` (span includes parens).
        // Tuples are written `(T, U)` or `(T,)`.
        let paren_checkpoint = stream.checkpoint();
        if stream.parse::<Option<T!['(']>>()?.is_some() {
            let list = stream.parse::<SeparatedList<Type, T![,], true>>()?;
            stream.parse::<T![')']>()?;

            let outer_span = stream.span_since(paren_checkpoint);
            let sep_count = list.separator_count();
            let mut items = list.value_owned();

            if items.len() == 1 && sep_count == 0 {
                let mut inner = items.remove(0);
                inner.span = outer_span;
                return Ok(TypeAtom(inner));
            }

            return Ok(TypeAtom(Type {
                kind: TypeKind::Tuple(items),
                span: outer_span,
            }));
        }
        stream.restore(paren_checkpoint);

        let kind = match_map!(stream,
            (T![for], TypeBinderParams, Type) => |(_for, params, ty)| TypeKind::ForAll { params, ty: Box::new(ty) },
            (T!['{'], SeparatedList<StructuralField, T![,], true>, T!['}']) => |(_l, fields, _r)| TypeKind::Structural(fields.value_owned()),
            (T!['['], Type, T![;], Expr, T![']']) => |(_l, ty, _s, expr, _r)| TypeKind::Array(Box::new(ty), Box::new(expr)),
            (T!['['], Type, T![']']) => |(_l, ty, _r)| TypeKind::Slice(Box::new(ty)),
            FunctionType => TypeKind::Function,
            T![!] => |_| TypeKind::Never,
            T!["_"] => |_| TypeKind::Infer,
            TypeOperator => TypeKind::Operator,
            (T![impl], Path) => |(_, path)| TypeKind::ImplTrait(path),
            (T![dyn], Path) => |(_, path)| TypeKind::DynTrait(path),
            Path => |path| TypeKind::Named(path)
        )?;

        // A path followed by `!` is a type-position macro invocation.
        let kind = if let TypeKind::Named(path) = kind {
            if stream.peek().map(|t| t.kind()) == Some(&crate::tokenizer::TokenKind::Bang) {
                stream.advance(); // consume `!`
                let args = parse_macro_args(stream)?;
                TypeKind::MacroInvocation(MacroInvocation {
                    path,
                    args,
                    span: stream.span_since(checkpoint),
                })
            } else {
                TypeKind::Named(path)
            }
        } else {
            kind
        };

        Ok(TypeAtom(Type {
            kind,
            span: stream.span_since(checkpoint),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Interner;
    use crate::tokenizer::tokens::TokenKind;

    #[test]
    fn parses_parenthesized_qself_projection_type() {
        let src = "(<T as Trait>::Item<U>)";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(src, &mut interner).expect("tokenize");
        let ty = stream.parse::<Type>().expect("parse type");

        match ty.kind {
            TypeKind::Named(path) => {
                assert!(path.qself.is_some());
                assert_eq!(path.segments.len(), 1);
                assert_eq!(path.segments[0].ident.as_str(&interner), "Item");
                assert!(path.segments[0].args.is_some());
            }
            other => panic!("expected Named(Path), got: {other:?}"),
        }

        assert!(stream.is_eof());
    }
}

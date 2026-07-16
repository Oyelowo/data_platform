/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 11/12/2025
 */

use crate::{Ident, T, expr::Expr};
use yelang_lexer::{
    ParseTokenStream, SeparatedList, Span, TokenResult, TokenStream, match_map_res,
};

/// Attribute/decorator
///
/// # Example
/// ```
/// #[derive(Debug, Clone)]
/// #[table(name = "users")]
/// #[index(fields = ["email"])]
/// @derive(Debug, Clone)
/// @options("fast", "secure")
/// @table(name = "users")
/// @index(fields = ['email'])
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    /// Attribute path
    pub path: Vec<Ident>,
    /// Whether the path is absolute (starts with `::`).
    pub is_absolute: bool,
    /// Attribute arguments
    pub args: AttributeArgs,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Attribute {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        type Segs = SeparatedList<Ident, T![::], false>;

        let start = stream.checkpoint();
        stream.parse::<T![@]>()?;
        let is_absolute = stream.optional::<T![::]>().is_ok();
        let path = stream.parse::<Segs>()?.value_owned();

        // SeparatedList can yield empty if the parser is in recovery mode.
        // Keep behavior deterministic: require at least one segment.
        if path.is_empty() {
            return Err(yelang_lexer::TokenError::SyntaxError {
                message: "Expected at least one segment in attribute path".to_string(),
                span: stream.span_since(start),
                source: None,
            });
        }

        let special_path = path
            .first()
            .map(|id| stream.interner().resolve(&id.symbol))
            .map(|s| s == "unsafe" || s == "derive")
            .unwrap_or(false);

        let args = if stream
            .peek()
            .map(|t| t.kind() == &crate::tokenizer::TokenKind::OpenParen)
            .unwrap_or(false)
        {
            stream.advance();
            let args = if special_path {
                type SepExpr = SeparatedList<Expr, T![,], true>;
                AttributeArgs::Positional(stream.parse::<SepExpr>()?.value_owned())
            } else {
                stream.parse::<AttributeArgs>()?
            };
            stream.consume(crate::tokenizer::TokenKind::CloseParen)?;
            args
        } else {
            AttributeArgs::Empty
        };

        let span = stream.span_since(start);
        Ok(Attribute {
            path,
            is_absolute,
            args,
            span,
        })
    }
}

/// Arguments to an attribute
#[derive(Debug, Clone, PartialEq)]
pub enum AttributeArgs {
    /// No arguments: `#[derive]`
    // @hidden
    Empty,
    /// @options("fast", "secure")
    /// Positional arguments: `#[allow(unused)]`
    Positional(Vec<Expr>),
    /// @table(name = "users")
    /// Named arguments: `#[table(name = "users")]`
    // this is still an expression i.e assign_eq expr
    Named(Vec<NamedArg>),
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for AttributeArgs {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        type Sep<T> = SeparatedList<T, T![,], true>;

        // Back-compat: keep the old split behavior if the input is *purely* named or *purely*
        // positional. If the list mixes named and positional items, desugar to positional args
        // with a trailing object literal.
        //
        // Additionally, support Rust-like nested meta items inside attribute args:
        // - `auth(role = 1, op = 2)` becomes a named arg `auth = { role: 1, op: 2 }`.
        //
        // This keeps the public AST stable (`AttributeArgs` remains {Empty, Positional, Named})
        // while enabling mixed/nested attribute UX.

        let start = stream.checkpoint();
        let items = stream.parse::<Sep<AttrArgItem>>()?.value_owned();
        if items.is_empty() {
            return Ok(AttributeArgs::Empty);
        }

        let mut positional: Vec<Expr> = Vec::new();
        let mut named: Vec<NamedArg> = Vec::new();

        for item in items {
            match item {
                AttrArgItem::Positional(expr) => positional.push(expr),
                AttrArgItem::Named(arg) => named.push(arg),
                AttrArgItem::Nested { name, args, span } => {
                    let value = attribute_args_to_expr(args, span);
                    named.push(NamedArg { name, value });
                }
            }
        }

        if named.is_empty() {
            return Ok(AttributeArgs::Positional(positional));
        }

        if positional.is_empty() {
            return Ok(AttributeArgs::Named(named));
        }

        // Mixed positional + named: append a synthetic object literal representing the named
        // args. This makes mixed usage compatible with the existing schema-validation strategy
        // (single-object positional) and enables schema-defined positional contracts.
        let mixed_span = stream.span_since(start);
        positional.push(named_args_to_object_expr(&named, mixed_span));
        Ok(AttributeArgs::Positional(positional))
    }
}

#[derive(Debug, Clone, PartialEq)]
enum AttrArgItem {
    /// `key = expr`
    Named(NamedArg),
    /// `key(expr, ...)` (Rust-like nested meta item)
    Nested {
        name: Ident,
        args: AttributeArgs,
        span: Span,
    },
    /// Any expression, including literals, objects, arrays, tuples, paths, etc.
    Positional(Expr),
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for AttrArgItem {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();
        match_map_res!(
            stream,
            (Ident, T![=], Expr) => |(name, _, value)| Ok(AttrArgItem::Named(NamedArg { name, value })),
            (Ident, T!['('], AttributeArgs, T![')']) => |(name, _, args, _)| {
                // `key(...)` nested meta item.
                // Note: This intentionally shadows the ability to pass a call expression as a
                // positional arg. If you need a call expression, write it explicitly as an
                // expression value, e.g. `@attr({ key: foo(1) })`.
                let span = stream.span_since(checkpoint);
                Ok(AttrArgItem::Nested { name, args, span })
            },
            Expr => |expr| Ok(AttrArgItem::Positional(expr)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Interner;
    use crate::{ExprKind, Literal, TokenKind};

    fn parse_single_attribute(src: &str) -> (Attribute, Interner) {
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(src, &mut interner).unwrap();
        let attr = stream.parse::<Attribute>().unwrap();
        (attr, interner)
    }

    fn assert_int(expr: &Expr, interner: &Interner, expected: &str) {
        match &expr.kind {
            ExprKind::Literal(Literal::Int(i)) => {
                assert_eq!(interner.resolve(&i.value), expected);
            }
            other => panic!("expected int literal {expected}, got {other:?}"),
        }
    }

    #[test]
    fn mixed_positional_and_named_desugars_to_positional_plus_object() {
        let (attr, interner) = parse_single_attribute("@permission(1, op = 2)");

        let AttributeArgs::Positional(args) = attr.args else {
            panic!(
                "expected positional args after desugaring, got {:?}",
                attr.args
            );
        };

        assert_eq!(args.len(), 2);
        assert_int(&args[0], &interner, "1");

        let ExprKind::Object(obj) = &args[1].kind else {
            panic!("expected trailing object literal, got {:?}", args[1].kind);
        };

        assert_eq!(obj.fields().len(), 1);
        let f = &obj.fields()[0];
        assert_eq!(f.key().as_str(&interner), "op");
        assert_int(f.value(), &interner, "2");
    }

    #[test]
    fn nested_meta_item_parses_as_named_object_value() {
        let (attr, interner) = parse_single_attribute("@http(auth(role = 1, op = 2))");

        let AttributeArgs::Named(named) = attr.args else {
            panic!("expected named args, got {:?}", attr.args);
        };

        assert_eq!(named.len(), 1);
        assert_eq!(named[0].name.as_str(&interner), "auth");

        let ExprKind::Object(auth_obj) = &named[0].value.kind else {
            panic!(
                "expected auth value to be object, got {:?}",
                named[0].value.kind
            );
        };

        // Order is not important; check presence.
        let mut saw_role = false;
        let mut saw_op = false;

        for f in auth_obj.fields() {
            match f.key().as_str(&interner) {
                "role" => {
                    saw_role = true;
                    assert_int(f.value(), &interner, "1");
                }
                "op" => {
                    saw_op = true;
                    assert_int(f.value(), &interner, "2");
                }
                other => panic!("unexpected nested key: {other}"),
            }
        }

        assert!(saw_role && saw_op);
    }

    #[test]
    fn positional_plus_nested_meta_item_desugars_to_trailing_object() {
        let (attr, interner) = parse_single_attribute("@http(1, auth(role = 2))");

        let AttributeArgs::Positional(args) = attr.args else {
            panic!("expected positional args, got {:?}", attr.args);
        };

        assert_eq!(args.len(), 2);
        assert_int(&args[0], &interner, "1");

        let ExprKind::Object(obj) = &args[1].kind else {
            panic!("expected trailing object literal, got {:?}", args[1].kind);
        };

        assert_eq!(obj.fields().len(), 1);
        let f = &obj.fields()[0];
        assert_eq!(f.key().as_str(&interner), "auth");

        let ExprKind::Object(auth_obj) = &f.value().kind else {
            panic!("expected auth object, got {:?}", f.value().kind);
        };

        assert_eq!(auth_obj.fields().len(), 1);
        assert_eq!(auth_obj.fields()[0].key().as_str(&interner), "role");
        assert_int(auth_obj.fields()[0].value(), &interner, "2");
    }
}

fn named_args_to_object_expr(named: &[NamedArg], span: Span) -> Expr {
    use crate::expr::{ExprKind, Object, ObjectField};
    Expr {
        kind: ExprKind::Object(Object {
            fields: named
                .iter()
                .map(|a| ObjectField::new(a.name.clone(), a.value.clone()))
                .collect(),
            span,
        }),
        span,
    }
}

fn attribute_args_to_expr(args: AttributeArgs, span: Span) -> Expr {
    use crate::expr::{ExprKind, Object};
    match args {
        AttributeArgs::Empty => Expr {
            kind: ExprKind::Object(Object {
                fields: Vec::new(),
                span,
            }),
            span,
        },
        AttributeArgs::Named(named) => named_args_to_object_expr(&named, span),
        AttributeArgs::Positional(positional) => {
            if positional.is_empty() {
                Expr {
                    kind: ExprKind::Object(Object {
                        fields: Vec::new(),
                        span,
                    }),
                    span,
                }
            } else if positional.len() == 1 {
                positional[0].clone()
            } else {
                Expr {
                    kind: ExprKind::Tuple(positional),
                    span,
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NamedArg {
    pub name: Ident,
    pub value: Expr,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for NamedArg {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let (name, _, value) = stream.parse::<(Ident, T![=], Expr)>()?;
        Ok(NamedArg { name, value })
    }
}

impl Attribute {
    /// Check if this is a #[lang = "..."] attribute and extract the value.
    pub fn lang_value_str(&self, interner: &crate::Interner) -> Option<String> {
        if !self.is_absolute
            && self.path.len() == 1
            && self.path[0].symbol == interner.intern("lang")
        {
            match &self.args {
                AttributeArgs::Positional(args) if args.len() == 1 => {
                    if let crate::ExprKind::Literal(crate::Literal::Str(s)) = &args[0].kind {
                        Some(interner.resolve(&s.value).to_string())
                    } else {
                        None
                    }
                }
                _ => None,
            }
        } else {
            None
        }
    }

    /// Check if this is an `@intrinsic("...")` attribute and extract the string argument.
    pub fn intrinsic_value_str(&self, interner: &crate::Interner) -> Option<String> {
        if !self.is_absolute
            && self.path.len() == 1
            && self.path[0].symbol == interner.intern("intrinsic")
        {
            match &self.args {
                AttributeArgs::Positional(args) if args.len() == 1 => {
                    if let crate::ExprKind::Literal(crate::Literal::Str(s)) = &args[0].kind {
                        Some(interner.resolve(&s.value).to_string())
                    } else {
                        None
                    }
                }
                _ => None,
            }
        } else {
            None
        }
    }
}

/// A list of attributes
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AttributesList(pub Vec<Attribute>);

impl ParseTokenStream<crate::tokenizer::TokenKind> for AttributesList {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let attributes = stream
            .parse::<yelang_lexer::RepeatMin<0, Attribute>>()?
            .value_owned();
        Ok(AttributesList(attributes))
    }
}

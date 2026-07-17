/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 11/12/2025
 */

use crate::{Path, TokenKind};
use yelang_lexer::{ParseTokenStream, Span, TokenResult, TokenStream, match_map};

use super::*;
use crate::Codegen;

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub kind: ItemKind,
    pub attributes: Vec<Attribute>,
    pub visibility: Visibility,
    pub span: Span,
}

impl Item {}

/// Top-level item (module-level declaration)
///
/// # Example
/// ```
/// mod utils { ... }
/// fn main() { ... }
/// struct Point { ... }
/// const PI: f64 = 3.14;
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum ItemKind {
    Module(ModDef),
    /// struct Point { x: i32, y: i32 }
    Struct(Struct),
    /// enum Option<T> { Some(T), None }
    Enum(Enum),
    /// type Result<T> = std::result::Result<T, Error>;
    TypeAlias(Box<TypeAlias>),
    /// trait Display { ... }
    Trait(Box<Trait>),
    /// Function definition
    Fn(Box<FnDef>),
    /// Constant declaration
    Const(Box<Const>),
    /// Static declaration
    Static(Box<Static>),
    /// Implementation block
    Impl(Box<Impl>),
    /// Use declaration
    Use(Use),
    /// Declarative macro definition: `macro name { ... }`
    MacroDef(Box<MacroDef>),

    /// Macro invocation in item position: `my_macro! { ... }`.
    MacroInvocation(crate::expr::MacroInvocation),
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for ItemKind {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        if let Some(tok) = stream.peek() {
            match tok.kind() {
                TokenKind::DefaultKw => {
                    if stream
                        .peek_ahead(1)
                        .is_some_and(|next| matches!(next.kind(), TokenKind::Impl))
                    {
                        return stream.parse::<Impl>().map(|i| ItemKind::Impl(Box::new(i)));
                    }
                }
                TokenKind::Struct => return stream.parse::<Struct>().map(ItemKind::Struct),
                TokenKind::Enum => return stream.parse::<Enum>().map(ItemKind::Enum),
                TokenKind::Trait => {
                    return stream
                        .parse::<Trait>()
                        .map(|t| ItemKind::Trait(Box::new(t)));
                }
                TokenKind::Const => {
                    // `const` can start either a const item (`const X: T = ...;`) or a const
                    // function (`const fn f(...) { ... }`). Disambiguate via lookahead.
                    let is_const_fn = stream
                        .peek_ahead(1)
                        .is_some_and(|t| matches!(t.kind(), TokenKind::Fn | TokenKind::Async));

                    if is_const_fn {
                        return stream.parse::<FnDef>().map(|f| ItemKind::Fn(Box::new(f)));
                    }

                    return stream
                        .parse::<Const>()
                        .map(|c| ItemKind::Const(Box::new(c)));
                }
                TokenKind::Static => {
                    return stream
                        .parse::<Static>()
                        .map(|s| ItemKind::Static(Box::new(s)));
                }
                TokenKind::Use => return stream.parse::<Use>().map(ItemKind::Use),
                TokenKind::Macro => {
                    return stream
                        .parse::<MacroDef>()
                        .map(|m| ItemKind::MacroDef(Box::new(m)));
                }
                TokenKind::Mod => return stream.parse::<ModDef>().map(ItemKind::Module),
                TokenKind::Impl => {
                    return stream.parse::<Impl>().map(|i| ItemKind::Impl(Box::new(i)));
                }
                TokenKind::TypeToken => {
                    return stream
                        .parse::<TypeAlias>()
                        .map(|t| ItemKind::TypeAlias(Box::new(t)));
                }
                // `async fn ...` items start with `async`.
                TokenKind::Fn | TokenKind::Async => {
                    return stream.parse::<FnDef>().map(|f| ItemKind::Fn(Box::new(f)));
                }
                _ => {}
            }
        }

        // Rust-compatible `macro_rules! name { ... }` macro definition.
        if let Some(tok) = stream.peek()
            && let TokenKind::Ident(ident) = tok.kind()
            && stream.interner().resolve(&ident.symbol) == "macro_rules"
            && stream
                .peek_ahead(1)
                .is_some_and(|t| matches!(t.kind(), TokenKind::Bang))
        {
            return stream
                .parse::<MacroDef>()
                .map(|m| ItemKind::MacroDef(Box::new(m)));
        }

        // Macro invocation in item position: `my_macro! { ... }`.
        // A bare Path is not otherwise a valid item, so any `path!` here is a macro call.
        let checkpoint = stream.checkpoint();
        if let Ok(path) = stream.parse::<Path>() {
            if stream.peek().map(|t| t.kind()) == Some(&TokenKind::Bang) {
                stream.advance(); // consume `!`
                let args = crate::expr::parse_macro_args(stream)?;
                return Ok(ItemKind::MacroInvocation(crate::expr::MacroInvocation {
                    path,
                    args,
                    span: stream.span_since(checkpoint),
                }));
            }
        }
        stream.restore(checkpoint);

        // Fallback (keeps behavior for any future/odd item forms).
        match_map!(
            stream,
            Struct => ItemKind::Struct,
            FnDef => |f| ItemKind::Fn(Box::new(f)),
            Enum => ItemKind::Enum,
            Trait => |t| ItemKind::Trait(Box::new(t)),
            Const => |c| ItemKind::Const(Box::new(c)),
            Static => |s| ItemKind::Static(Box::new(s)),
            TypeAlias => |t| ItemKind::TypeAlias(Box::new(t)),
            Use => |u| ItemKind::Use(u),
            ModDef => ItemKind::Module,
            Impl => |i| ItemKind::Impl(Box::new(i)),
        )
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Item {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let (item, span) = stream.parse_with_span::<(AttributesList, Visibility, ItemKind)>()?;
        let (AttributesList(attributes), visibility, kind) = item;
        Ok(Item {
            kind,
            attributes,
            visibility,
            span,
        })
    }
}

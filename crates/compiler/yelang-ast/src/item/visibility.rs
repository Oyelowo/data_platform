/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 11/12/2025
 */

use crate::Codegen;
use crate::Interner;
use crate::{Path, T};
use std::fmt::{self, Write};
use yelang_lexer::{OneOf4, ParseTokenStream, Span, TokenResult, TokenStream};

/// Visibility modifier
///
/// # Example
/// ```
/// pub fn foo() { }              // Public
/// fn bar() { }                  // Private (default)
/// pub(crate) fn baz() { }       // Public within crate
/// pub(super) fn qux() { }       // Public to parent module
/// pub(self) fn local() { }      // Public to current module (same as private)
/// pub(in path::to::module) fn internal() { } // Public within specific path
/// ```
#[derive(Default, Debug, Clone, PartialEq)]
pub enum Visibility {
    /// Private (default): no modifier
    #[default]
    Private,
    /// Public: `pub`
    Public(Span),
    /// Public within crate: `pub(crate)`
    PublicCrate(Span),
    /// Public to parent module: `pub(super)`
    PublicSuper(Span),
    /// Public to current module: `pub(self)` (equivalent to private)
    PublicSelf(Span),
    /// Public within specific path: `pub(in path::to::module)`
    PublicIn { path: Path, span: Span },
}

impl Visibility {
    /// Get the span of this visibility modifier
    pub fn span(&self) -> Option<Span> {
        match self {
            Visibility::Public(span)
            | Visibility::PublicCrate(span)
            | Visibility::PublicSuper(span)
            | Visibility::PublicSelf(span) => Some(*span),
            Visibility::PublicIn { span, .. } => Some(*span),
            Visibility::Private => None,
        }
    }

    /// Check if this visibility is public (any form)
    pub fn is_public(&self) -> bool {
        !matches!(self, Visibility::Private)
    }

    /// Check if this is the default private visibility
    pub fn is_private(&self) -> bool {
        matches!(self, Visibility::Private)
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Visibility {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();
        let Ok(_) = stream.parse::<T![pub]>() else {
            return Ok(Visibility::Private);
        };
        let res = stream.parse::<(
            T!['('],
            OneOf4<
                T![crate],
                T![super],
                T![self],
                //
                (T![in], Path),
            >,
            T![')'],
        )>();

        let span = stream.span_since(checkpoint);
        match res {
            Ok((_, modifier, _)) => match modifier {
                OneOf4::_1(_) => Ok(Visibility::PublicCrate(span)),
                OneOf4::_2(_) => Ok(Visibility::PublicSuper(span)),
                OneOf4::_3(_) => Ok(Visibility::PublicSelf(span)),
                OneOf4::_4((_, path)) => Ok(Visibility::PublicIn { path, span }),
            },
            Err(_) => Ok(Visibility::Public(span)),
        }
    }
}

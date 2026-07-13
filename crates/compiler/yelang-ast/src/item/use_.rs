/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 11/12/2025
 */

use crate::Codegen;
use crate::Interner;
use crate::{Ident, Path, T};
use std::fmt::{self, Write};
use yelang_lexer::{
    OneOf3, ParseTokenStream, SeparatedList, Span, TokenResult, TokenStream, match_map,
};

/// Complete use statement
#[derive(Debug, Clone, PartialEq)]
pub struct Use {
    pub tree: UseTree,
    pub span: Span,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for Use {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let ((_use, tree, _), span) = stream.parse_with_span::<(T![use], UseTree, T![;])>()?;
        Ok(Use { tree, span })
    }
}

///
/// # Examples
/// ```
/// use std::collections;                    // Simple
/// use std::collections::HashMap as Map;    // Rename  
/// use std::collections::*;                 // Glob
/// use std::{collections, io};              // Nested
/// use std::io::{self, Read, Write};        // Nested with self
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum UseTree {
    /// Simple path import: `std::collections`
    Simple { path: Path, span: Span },
    /// Renamed import: `std::collections::HashMap as Map`
    Rename {
        path: Path,
        alias: Ident,
        span: Span,
    },
    /// Glob import: `std::collections::*`
    Glob { path: Path, span: Span },
    /// Nested imports: `std::{collections, io}`
    Nested {
        prefix: Path,
        items: Vec<UseTree>,
        span: Span,
    },
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for UseTree {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        type Tree = OneOf3<
            (T![as], Ident),
            (T![::], T![*]),
            (
                T![::],
                T!['{'],
                SeparatedList<UseTree, T![,], true>,
                T!['}'],
            ),
        >;

        let ((path, tree), span) = stream.parse_with_span::<(Path, Option<Tree>)>()?;

        let res = match tree {
            Some(tree) => match tree {
                OneOf3::_1((_, alias)) => UseTree::Rename { path, alias, span },
                OneOf3::_2((_, _)) => UseTree::Glob { path, span },
                OneOf3::_3((_, _, nested, _)) => UseTree::Nested {
                    prefix: path,
                    items: nested.value_owned(),
                    span,
                },
            },
            None => UseTree::Simple { path, span },
        };
        Ok(res)
    }
}

// Helper methods for common patterns
impl UseTree {
    /// Check if this is a simple path import
    pub fn is_simple(&self) -> bool {
        matches!(self, UseTree::Simple { .. })
    }

    /// Check if this is a glob import  
    pub fn is_glob(&self) -> bool {
        matches!(self, UseTree::Glob { .. })
    }

    /// Check if this is a nested import
    pub fn is_nested(&self) -> bool {
        matches!(self, UseTree::Nested { .. })
    }

    /// Get the span of this use tree
    pub fn span(&self) -> Span {
        match self {
            UseTree::Simple { span, .. }
            | UseTree::Rename { span, .. }
            | UseTree::Glob { span, .. }
            | UseTree::Nested { span, .. } => *span,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Interner;
    use crate::tokenizer::tokens::TokenKind;

    #[test]
    fn test_parse_simple_use() {
        let input = "use std::collections::HashMap";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let _use = stream.parse::<T![use]>().unwrap();
        let use_tree = stream.parse::<UseTree>().unwrap();

        match use_tree {
            UseTree::Simple { .. } => {}
            _ => panic!("Expected simple use"),
        }
    }
}

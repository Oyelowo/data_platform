/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2024 Oyelowo Oyedayo
 * Date 31/12/2024
 */

use crate::{Expr, Pattern, T};
use yelang_lexer::{ParseTokenStream, RepeatMin, TokenResult, TokenStream};

// Supporting structs for expression variants
/// Comprehension expression for list/array generation
///
/// # Example
/// ```
/// [x * 2 for x in items if x > 0]
/// [user.name for user in users]
/// [(x, y) for x in xs for y in ys if x != y]
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ComprehensionExpr {
    /// The element expression: `x * 2`
    pub element: Box<Expr>,
    /// Variable bindings: `for x in items, for y in others`
    pub variables: Vec<ComprehensionVar>,
    /// Optional filter condition: `if x > 0`
    pub condition: Option<Box<Expr>>,
}

/// A variable binding in a comprehension
///
/// # Example
/// ```
/// for x in items
/// for (key, value) in map
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ComprehensionVar {
    /// Pattern for destructuring: `x`, `(key, value)`
    pub pattern: Pattern,
    /// Source collection to iterate over
    pub source: Box<Expr>,
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for ComprehensionExpr {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let (_, element, vars, cond, _) = stream.parse::<(
            T!['['],
            Expr,
            RepeatMin<1, ComprehensionVar>,
            Option<(T![if], Expr)>,
            T![']'],
        )>()?;

        Ok(Self {
            element: Box::new(element),
            variables: vars.value_owned(),
            condition: cond.map(|(_, expr)| Box::new(expr)),
        })
    }
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for ComprehensionVar {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        type For = (T![for], Pattern, T![in], Expr);
        let (_for, pattern, _in, source) = stream.parse::<For>()?;
        Ok(Self {
            pattern,
            source: Box::new(source),
        })
    }
}

impl ComprehensionExpr {}

impl ComprehensionVar {}

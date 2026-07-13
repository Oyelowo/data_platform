use super::*;
use crate::T;
use crate::expr::{Expr, Precedence, Restrictions};
use yelang_lexer::{ParseTokenStream, TokenResult, TokenStream, match_map_res};

/// Type-level operators for advanced type manipulation
#[derive(Debug, Clone, PartialEq)]
pub enum TypeOperator {
    /// Get the type of an expression: `typeof expr`
    ///
    /// # Example
    /// ```
    /// let x = 42;
    /// let y: typeof x = 100;
    /// ```
    TypeOf(Box<Expr>),

    /// Extract return type of a function: `ReturnType<typeof fn>`
    ///
    /// # Example
    /// ```
    /// fn foo() -> i32 { 42 }
    /// let x: ReturnType<typeof foo> = 100;
    /// ```
    ReturnType(Box<Type>),

    /// Extract parameter types: `Parameters<typeof fn>`
    ///
    /// # Example
    /// ```
    /// fn foo(x: i32, y: string) {}
    /// let params: Parameters<typeof foo>; // (i32, string)
    /// ```
    Parameters(Box<Type>),

    /// Pick a subset of fields from a structural type: `Pick<T, "a" | "b">`
    Pick(Box<Type>, Box<Type>),

    /// Omit a subset of fields from a structural type: `Omit<T, "a" | "b">`
    Omit(Box<Type>, Box<Type>),
}

impl ParseTokenStream<crate::tokenizer::TokenKind> for TypeOperator {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        // FIXME: type and variable are in different namesapce and can coexist. so
        // maybe we dont have to make ReturnType and Parameters keywords? and maybe even typeof?
        // type M = i32;
        // let M = 5;
        // let m = M as M;
        match_map_res!(stream,
            (T![typeof]) => |_typeof| {
                // IMPORTANT: `typeof` appears in type position (e.g. `let x: typeof y = 1;`).
                // If we parse a full expression here, it can accidentally consume the `= 1`
                // initializer as an assignment (`y = 1`).
                //
                // Disallow assignment operators by starting Pratt parsing at `LogicalOr`.
                // This still allows postfix chains (member/index/call) but will stop before `=`.
                let expr = Expr::parse_pratt(stream, Precedence::LogicalOr, Restrictions::NONE)?;
                Ok(TypeOperator::TypeOf(Box::new(expr)))
            },
            (T![ReturnType], T![<], Type, T![>]) => |(_ret, _l, ty, _r)| Ok(TypeOperator::ReturnType(Box::new(ty))),
            (T![Parameters], T![<], Type, T![>]) => |(_param, _l, ty, _r)| Ok(TypeOperator::Parameters(Box::new(ty))),
            (T![Pick], T![<], Type, T![,], Type, T![>]) => |(_pick, _l, base, _comma, keys, _r)| {
                Ok(TypeOperator::Pick(Box::new(base), Box::new(keys)))
            },
            (T![Omit], T![<], Type, T![,], Type, T![>]) => |(_omit, _l, base, _comma, keys, _r)| {
                Ok(TypeOperator::Omit(Box::new(base), Box::new(keys)))
            }
        )
    }
}

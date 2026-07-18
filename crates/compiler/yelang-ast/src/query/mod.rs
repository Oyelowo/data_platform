/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2025 Oyelowo Oyedayo
 * Date 09/03/2025
 */

mod create;
mod delete;
mod link;
mod parse;
mod select;
mod types;
mod unlink;
mod update;
mod upsert;

pub use create::*;
pub use delete::*;
pub use link::*;
pub use select::*;
pub use unlink::*;
pub use update::*;
pub use upsert::*;

pub use types::*;

use crate::{Expr, T};
use yelang_lexer::{ParseTokenStream, TokenResult, TokenStream};

/// Parse an optional query tail expression in block form.
///
/// Mutation queries do not use a `return` clause; the value produced by the
/// block is introduced by `; <expr>` so that `return` remains reserved for
/// function-level early returns.
///
/// Returns `None` if no tail expression is present.
pub(crate) fn parse_query_tail(
    stream: &mut TokenStream<crate::tokenizer::TokenKind>,
) -> TokenResult<Option<Expr>> {
    if stream.parse::<Option<T![;]>>()?.is_some() {
        return Ok(Some(stream.parse::<Expr>()?));
    }
    Ok(None)
}

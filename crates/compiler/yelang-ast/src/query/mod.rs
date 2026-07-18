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
/// Accepts either `return <expr>` or, for compatibility with earlier block
/// syntax, `; <expr>`. Returns `None` if neither form is present.
pub(crate) fn parse_query_tail(
    stream: &mut TokenStream<crate::tokenizer::TokenKind>,
) -> TokenResult<Option<Expr>> {
    if stream.parse::<Option<T![return]>>()?.is_some() {
        return Ok(Some(stream.parse::<Expr>()?));
    }
    if stream.parse::<Option<T![;]>>()?.is_some() {
        return Ok(Some(stream.parse::<Expr>()?));
    }
    Ok(None)
}

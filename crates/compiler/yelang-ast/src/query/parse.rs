/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2025 Oyelowo Oyedayo
 * Date 09/03/2025
 */

use super::{Query, QueryKind};
use crate::T;
use yelang_lexer::{ParseTokenStream, TokenError, TokenResult, TokenStream, Verify};

impl ParseTokenStream<crate::tokenizer::TokenKind> for Query {
    fn parse(stream: &mut TokenStream<crate::tokenizer::TokenKind>) -> TokenResult<Self> {
        let checkpoint = stream.checkpoint();

        // IMPORTANT: query keywords uniquely determine the parse.
        // We must not use backtracking across query kinds because it can swallow real
        // parse errors (e.g. `select ... group by ...` diagnostics) and replace them with
        // unrelated errors from later alternatives.
        let qk = if stream.parse::<Verify<T![select]>>().is_ok() {
            QueryKind::Select(Box::new(stream.parse::<super::SelectQ>()?))
        } else if stream.parse::<Verify<T![create]>>().is_ok() {
            QueryKind::Create(stream.parse::<super::CreateQ>()?)
        } else if stream.parse::<Verify<T![update]>>().is_ok() {
            QueryKind::Update(stream.parse::<super::UpdateQ>()?)
        } else if stream.parse::<Verify<T![upsert]>>().is_ok() {
            QueryKind::Upsert(stream.parse::<super::UpsertQ>()?)
        } else if stream.parse::<Verify<T![link]>>().is_ok() {
            QueryKind::Link(stream.parse::<super::LinkQ>()?)
        } else if stream.parse::<Verify<T![unlink]>>().is_ok() {
            QueryKind::Unlink(stream.parse::<super::UnlinkQ>()?)
        } else if stream.parse::<Verify<T![delete]>>().is_ok() {
            QueryKind::Delete(stream.parse::<super::DeleteQ>()?)
        } else {
            let found = stream
                .peek()
                .map(|t| format!("{:?}", t.kind()))
                .unwrap_or_else(|| "EOF".to_string());
            return Err(TokenError::UnexpectedToken {
                expected: "query keyword (select/create/update/upsert/link/unlink/delete)".into(),
                found,
                span: stream.span_since(checkpoint),
            });
        };

        let span = stream.span_since(checkpoint);
        Ok(Query { kind: qk, span })
    }
}

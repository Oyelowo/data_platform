/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2025 Oyelowo Oyedayo
 * Date 09/03/2025
 */

use yelang_lexer::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub kind: QueryKind,
    pub span: Span,
}

impl Query {
    pub fn span(&self) -> Span {
        self.span
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum QueryKind {
    Select(Box<super::SelectQ>),
    Create(super::CreateQ),
    Update(super::UpdateQ),
    Upsert(super::UpsertQ),
    Link(super::LinkQ),
    Unlink(super::UnlinkQ),
    Delete(super::DeleteQ),
}

impl QueryKind {}

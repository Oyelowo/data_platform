use yelang_util::SyntaxContextId;

/// A region of source code plus hygiene context.
///
/// Every token carries a `Span`. It is used for diagnostics and for hygiene:
/// identifiers with the same textual name but different `SyntaxContextId`
/// values do not collide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    /// Inclusive low byte offset in the source file.
    pub lo: u32,
    /// Exclusive high byte offset in the source file.
    pub hi: u32,
    /// Source file ID.
    pub file: yelang_lexer::FileId,
    /// Hygiene context.
    pub ctx: SyntaxContextId,
}

impl Span {
    pub fn new(lo: u32, hi: u32, file: yelang_lexer::FileId, ctx: SyntaxContextId) -> Self {
        Self { lo, hi, file, ctx }
    }

    pub fn with_ctx(self, ctx: SyntaxContextId) -> Self {
        Self { ctx, ..self }
    }

    pub fn with_file(self, file: yelang_lexer::FileId) -> Self {
        Self { file, ..self }
    }

    pub fn merged(self, other: Self) -> Self {
        Self {
            lo: self.lo.min(other.lo),
            hi: self.hi.max(other.hi),
            file: self.file,
            ctx: self.ctx,
        }
    }
}

impl From<yelang_lexer::Span> for Span {
    fn from(span: yelang_lexer::Span) -> Self {
        let start = span.start().absolute as u32;
        let end = span.end().absolute as u32;
        Self::new(start, end, span.file_id(), SyntaxContextId::default())
    }
}

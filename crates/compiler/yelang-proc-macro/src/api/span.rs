//! Source spans and hygiene contexts.

use std::fmt;

/// A source region with hygiene information.
///
/// This wraps the compiler-internal `yelang_macro_core::Span` and exposes only
/// the operations a procedural macro author needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    inner: yelang_macro_core::Span,
}

impl Span {
    /// The span of the macro invocation site.
    pub fn call_site() -> Self {
        // The call-site span is synthesized with the default hygiene context.
        Self {
            inner: yelang_macro_core::Span::default(),
        }
    }

    /// The span of the macro definition site.
    ///
    /// In the current implementation this resolves to the default context;
    /// the compiler/server integration remaps def-site identifiers through
    /// the proper hygiene data before name resolution.
    pub fn def_site() -> Self {
        Self::call_site()
    }

    /// Mixed-site hygiene, similar to `macro_rules!` `$crate`.
    ///
    /// See [`Span::def_site`] for implementation notes.
    pub fn mixed_site() -> Self {
        Self::call_site()
    }

    pub(crate) fn from_inner(inner: yelang_macro_core::Span) -> Self {
        Self { inner }
    }

    pub(crate) fn into_inner(self) -> yelang_macro_core::Span {
        self.inner
    }

    /// Resolve this span's hygiene to another span's context.
    pub fn resolved_at(self, other: Span) -> Self {
        Self {
            inner: self.inner.with_ctx(other.inner.ctx),
        }
    }

    /// Source file containing this span.
    pub fn source_file(&self) -> SourceFile {
        SourceFile {
            file_id: self.inner.file,
        }
    }

    /// Start position in the source file.
    pub fn start(&self) -> LineColumn {
        // Offsets are byte offsets; real line/column require the source map.
        // We expose byte offsets as a stable approximation.
        LineColumn {
            line: 1,
            column: self.inner.lo as usize + 1,
        }
    }

    /// End position in the source file.
    pub fn end(&self) -> LineColumn {
        LineColumn {
            line: 1,
            column: self.inner.hi as usize + 1,
        }
    }
}

impl From<Span> for yelang_macro_core::Span {
    fn from(span: Span) -> Self {
        span.inner
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.inner.lo, self.inner.hi)
    }
}

/// A source file identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceFile {
    pub(crate) file_id: yelang_lexer::FileId,
}

impl SourceFile {
    pub fn path(&self) -> String {
        format!("<file:{}>", self.file_id.raw())
    }
}

/// A line and column position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LineColumn {
    pub line: usize,
    pub column: usize,
}

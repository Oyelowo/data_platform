//! Source spans and hygiene contexts.

use std::cell::RefCell;
use std::fmt;

/// A source region with hygiene information.
///
/// This wraps the compiler-internal `yelang_macro_core::Span` and exposes only
/// the operations a procedural macro author needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    inner: yelang_macro_core::Span,
}

thread_local! {
    /// The span of the current macro invocation, set by the proc-macro server
    /// before the user-provided macro body runs.
    static CALL_SITE: RefCell<Option<Span>> = const { RefCell::new(None) };

    /// The span of the macro definition site, set by the proc-macro server.
    static DEF_SITE: RefCell<Option<Span>> = const { RefCell::new(None) };

    /// The mixed-site hygiene span, set by the proc-macro server.
    ///
    /// Mixed-site behaves like the call site for local bindings and like the
    /// definition site for items/types, mirroring declarative macro hygiene.
    static MIXED_SITE: RefCell<Option<Span>> = const { RefCell::new(None) };
}

impl Span {
    /// The span of the macro invocation site.
    pub fn call_site() -> Self {
        CALL_SITE.with(|c| *c.borrow()).unwrap_or_else(|| Self {
            inner: yelang_macro_core::Span::default(),
        })
    }

    /// The span of the macro definition site.
    ///
    /// Returns the definition-site span supplied by the compiler, falling back
    /// to [`Self::call_site`] if none was provided.
    pub fn def_site() -> Self {
        DEF_SITE
            .with(|c| *c.borrow())
            .unwrap_or_else(Self::call_site)
    }

    /// Mixed-site hygiene, similar to declarative macro `$crate`.
    ///
    /// Returns the mixed-site span supplied by the compiler, falling back to
    /// [`Self::call_site`] if none was provided.
    pub fn mixed_site() -> Self {
        MIXED_SITE
            .with(|c| *c.borrow())
            .unwrap_or_else(Self::call_site)
    }

    pub(crate) fn from_inner(inner: yelang_macro_core::Span) -> Self {
        Self { inner }
    }

    pub(crate) fn into_inner(self) -> yelang_macro_core::Span {
        self.inner
    }

    /// Set the span that `Span::call_site()` will return for the current thread.
    ///
    /// This is used by the proc-macro server to pass the invocation site into
    /// the macro. It is not part of the stable public API.
    #[doc(hidden)]
    pub fn set_call_site(span: Span) {
        CALL_SITE.with(|c| *c.borrow_mut() = Some(span));
    }

    /// Set the span that `Span::def_site()` will return for the current thread.
    #[doc(hidden)]
    pub fn set_def_site(span: Span) {
        DEF_SITE.with(|c| *c.borrow_mut() = Some(span));
    }

    /// Set the span that `Span::mixed_site()` will return for the current thread.
    #[doc(hidden)]
    pub fn set_mixed_site(span: Span) {
        MIXED_SITE.with(|c| *c.borrow_mut() = Some(span));
    }

    /// Clear the thread-local call-site span.
    #[doc(hidden)]
    pub fn clear_call_site() {
        CALL_SITE.with(|c| *c.borrow_mut() = None);
    }

    /// Clear the thread-local definition-site span.
    #[doc(hidden)]
    pub fn clear_def_site() {
        DEF_SITE.with(|c| *c.borrow_mut() = None);
    }

    /// Clear the thread-local mixed-site span.
    #[doc(hidden)]
    pub fn clear_mixed_site() {
        MIXED_SITE.with(|c| *c.borrow_mut() = None);
    }

    /// Clear all thread-local site spans at once.
    #[doc(hidden)]
    pub fn clear_sites() {
        Self::clear_call_site();
        Self::clear_def_site();
        Self::clear_mixed_site();
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

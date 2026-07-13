use yelang_lexer::{FileId, Span};

/// A stable (within one parse snapshot) pointer to a specific AST *occurrence*.
///
/// This is intentionally distinct from `Span`:
/// - `Span` is primarily diagnostics/source-location.
/// - `AstPtr` is a canonical key for resolver/lowering/typeck lookup tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AstPtr {
    pub file_id: FileId,
    pub start: u32,
    pub end: u32,
    pub kind: AstKind,
    pub disambiguator: u32,
}

impl AstPtr {
    pub fn new(span: Span, kind: AstKind) -> Self {
        let file_id = span.file_id();
        let start = span.start().absolute;
        let end = span.end().absolute;

        // Keep this as a debug assertion so error recovery can still proceed in release.
        debug_assert!(
            start <= end,
            "invalid span range for AstPtr: start={start} end={end}"
        );

        // We use u32 for compact, deterministic hashing across architectures.
        // If a file ever exceeds 4GiB of source, we can revisit this.
        let start_u32 = u32::try_from(start).unwrap_or(u32::MAX);
        let end_u32 = u32::try_from(end).unwrap_or(u32::MAX);

        Self {
            file_id,
            start: start_u32,
            end: end_u32,
            kind,
            disambiguator: 0,
        }
    }

    pub fn is_default_like(&self) -> bool {
        self.start == 0 && self.end == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AstKind {
    Ident,
    Path,
    Type,
    Pattern,
    Expr,
    Stmt,
    Item,
    Import,
    Other,
}

pub trait HasAstPtr {
    fn ast_ptr(&self) -> AstPtr;
}

macro_rules! impl_has_ptr {
    ($ty:path, $kind:expr, $span:expr) => {
        impl HasAstPtr for $ty {
            fn ast_ptr(&self) -> AstPtr {
                AstPtr::new($span(self), $kind)
            }
        }
    };
}

impl_has_ptr!(crate::Ident, AstKind::Ident, |s: &crate::Ident| s.span());

impl_has_ptr!(crate::Path, AstKind::Path, |s: &crate::Path| s.span);

impl_has_ptr!(crate::Type, AstKind::Type, |s: &crate::Type| s.span);

impl_has_ptr!(crate::Pattern, AstKind::Pattern, |s: &crate::Pattern| s
    .span);

impl_has_ptr!(crate::Expr, AstKind::Expr, |s: &crate::Expr| s.span);

impl_has_ptr!(crate::Stmt, AstKind::Stmt, |s: &crate::Stmt| s.span);

// Root/container nodes
impl_has_ptr!(crate::Program, AstKind::Item, |s: &crate::Program| s.span);

// Common wrapper node (used throughout the AST)
impl<T> HasAstPtr for super::common::Node<T> {
    fn ast_ptr(&self) -> AstPtr {
        AstPtr::new(self.span, AstKind::Other)
    }
}

// Item/import nodes
impl_has_ptr!(crate::Item, AstKind::Item, |s: &crate::Item| s.span);
impl_has_ptr!(crate::item::Use, AstKind::Import, |s: &crate::item::Use| s
    .span);
impl_has_ptr!(
    crate::item::UseTree,
    AstKind::Import,
    |s: &crate::item::UseTree| s.span()
);

// Attribute/decorator applications
impl_has_ptr!(
    crate::item::Attribute,
    AstKind::Other,
    |s: &crate::item::Attribute| s.span
);

// Path-ish helper nodes
impl_has_ptr!(
    crate::expr::PathSegment,
    AstKind::Path,
    |s: &crate::expr::PathSegment| s.span()
);
impl_has_ptr!(
    crate::expr::ExprPath,
    AstKind::Path,
    |s: &crate::expr::ExprPath| s.0.span
);
impl_has_ptr!(
    crate::expr::ExprPathSegment,
    AstKind::Path,
    |s: &crate::expr::ExprPathSegment| s.0.span()
);
impl_has_ptr!(
    crate::expr::QSelf,
    AstKind::Type,
    |s: &crate::expr::QSelf| s.span
);

// Query nodes
impl_has_ptr!(crate::Query, AstKind::Stmt, |s: &crate::Query| s.span);

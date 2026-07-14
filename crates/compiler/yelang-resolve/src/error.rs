use thiserror::Error;
use yelang_interner::Symbol;
use yelang_lexer::Span;
use yelang_util::DefId;

use crate::namespaces::Namespace;

#[derive(Error, Debug, Clone)]
pub enum ResolutionError {
    #[error("cannot find `{name:?}` in this scope")]
    NotFound {
        name: Symbol,
        span: Span,
    },

    #[error("`{name:?}` is ambiguous")]
    Ambiguous {
        name: Symbol,
        span: Span,
        candidates: Vec<DefId>,
    },

    #[error("`{name:?}` is a {found}, expected a {expected}")]
    WrongNamespace {
        name: Symbol,
        found: Namespace,
        expected: Namespace,
        span: Span,
    },

    #[error("circular import")]
    CircularImport {
        span: Span,
    },

    #[error("`{name:?}` defined multiple times")]
    DuplicateDefinition {
        name: Symbol,
        span: Span,
        original_span: Span,
    },
}

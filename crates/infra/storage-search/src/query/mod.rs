//! Query AST and execution.

pub mod exec;
pub mod parser;
pub mod scorer;

pub use exec::{execute, SearchResult};
pub use parser::{parse, ParseError};
pub use scorer::{Bm25Scorer, score_document};

/// Query AST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Query {
    /// A single term, optionally scoped to a field.
    Term {
        /// Field name, or `None` for all fields.
        field: Option<String>,
        /// Term text (already analyzed).
        term: String,
    },
    /// A phrase, optionally scoped to a field.
    Phrase {
        /// Field name, or `None` for all fields.
        field: Option<String>,
        /// Phrase terms in order.
        terms: Vec<String>,
    },
    /// A prefix query, optionally scoped to a field.
    Prefix {
        /// Field name, or `None` for all fields.
        field: Option<String>,
        /// Prefix text.
        prefix: String,
    },
    /// Boolean combination.
    Boolean {
        /// Must match.
        must: Vec<Query>,
        /// Should match (at least one unless `must` is empty).
        should: Vec<Query>,
        /// Must not match.
        must_not: Vec<Query>,
    },
}

impl Query {
    /// Create a term query over all fields.
    pub fn term(term: impl Into<String>) -> Self {
        Self::Term {
            field: None,
            term: term.into(),
        }
    }

    /// Create a fielded term query.
    pub fn field_term(field: impl Into<String>, term: impl Into<String>) -> Self {
        Self::Term {
            field: Some(field.into()),
            term: term.into(),
        }
    }

    /// Create a boolean query with `must` clauses.
    pub fn must(clauses: Vec<Query>) -> Self {
        Self::Boolean {
            must: clauses,
            should: Vec::new(),
            must_not: Vec::new(),
        }
    }

    /// Create a boolean query with `should` clauses.
    pub fn should(clauses: Vec<Query>) -> Self {
        Self::Boolean {
            must: Vec::new(),
            should: clauses,
            must_not: Vec::new(),
        }
    }

    /// Create a boolean query with `must_not` clauses.
    pub fn must_not(clauses: Vec<Query>) -> Self {
        Self::Boolean {
            must: Vec::new(),
            should: Vec::new(),
            must_not: clauses,
        }
    }
}

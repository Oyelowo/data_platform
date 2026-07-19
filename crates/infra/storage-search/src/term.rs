//! Term type and encoding for `storage-search`.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A term is a `(field, token)` pair.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Term {
    /// Field name.
    pub field: String,
    /// Token string.
    pub token: String,
}

impl Term {
    /// Create a new term.
    pub fn new(field: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            token: token.into(),
        }
    }

    /// Encode a term to a byte string suitable for dictionary ordering.
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(self.field.len() + self.token.len() + 4);
        buf.extend_from_slice(self.field.as_bytes());
        buf.push(0u8);
        buf.extend_from_slice(self.token.as_bytes());
        buf
    }
}

impl fmt::Display for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.field, self.token)
    }
}

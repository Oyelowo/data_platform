//! Schema and field definitions for `storage-search`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A field value stored in a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldValue {
    /// UTF-8 text.
    Text(String),
    /// Opaque byte payload.
    Bytes(Vec<u8>),
}

impl FieldValue {
    /// Return `true` if this value is text.
    pub fn is_text(&self) -> bool {
        matches!(self, FieldValue::Text(_))
    }

    /// Return the text value, if any.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            FieldValue::Text(t) => Some(t),
            FieldValue::Bytes(_) => None,
        }
    }

    /// Return the byte value, if any.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            FieldValue::Text(_) => None,
            FieldValue::Bytes(b) => Some(b),
        }
    }
}

/// Options controlling how a field is indexed and stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldOptions {
    /// Keep the original value retrievable.
    pub stored: bool,
    /// Add terms to the inverted index.
    pub indexed: bool,
    /// Tokenize text before indexing.
    pub tokenize: bool,
    /// Apply English Porter stemming.
    pub stem: bool,
    /// Store term positions for phrase queries.
    pub with_positions: bool,
}

impl FieldOptions {
    /// A stored field that is not indexed.
    pub fn stored() -> Self {
        Self {
            stored: true,
            indexed: false,
            tokenize: false,
            stem: false,
            with_positions: false,
        }
    }

    /// A full-text indexed field with positions.
    pub fn text() -> Self {
        Self {
            stored: true,
            indexed: true,
            tokenize: true,
            stem: true,
            with_positions: true,
        }
    }

    /// An indexed field that is not tokenized (keyword/identifier).
    pub fn keyword() -> Self {
        Self {
            stored: true,
            indexed: true,
            tokenize: false,
            stem: false,
            with_positions: false,
        }
    }
}

impl Default for FieldOptions {
    fn default() -> Self {
        Self::text()
    }
}

/// Schema describing the fields known to a search engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schema {
    /// Field name to options.
    pub fields: BTreeMap<String, FieldOptions>,
}

impl Schema {
    /// Create an empty schema.
    pub fn new() -> Self {
        Self {
            fields: BTreeMap::new(),
        }
    }

    /// Add a field with options.
    pub fn with_field(mut self, name: impl Into<String>, options: FieldOptions) -> Self {
        self.fields.insert(name.into(), options);
        self
    }

    /// Return the options for a field, if it exists.
    pub fn get(&self, name: &str) -> Option<&FieldOptions> {
        self.fields.get(name)
    }

    /// Validate that a document only contains known fields.
    pub fn validate_document(&self, fields: &BTreeMap<String, FieldValue>) -> crate::Result<()> {
        for name in fields.keys() {
            if !self.fields.contains_key(name) {
                return Err(crate::Error::invalid_argument(format!(
                    "unknown field: {name}"
                )));
            }
        }
        Ok(())
    }
}

impl Default for Schema {
    fn default() -> Self {
        Self::new()
    }
}

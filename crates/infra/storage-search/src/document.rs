//! Document model for `storage-search`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::schema::{FieldValue, Schema};

/// Document identifier: an opaque byte string provided by the caller.
pub type DocId = Vec<u8>;

/// A single document stored in the search engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    /// Field name to value.
    pub fields: BTreeMap<String, FieldValue>,
}

impl Document {
    /// Create an empty document.
    pub fn new() -> Self {
        Self {
            fields: BTreeMap::new(),
        }
    }

    /// Add a text field.
    pub fn with_text(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.fields
            .insert(name.into(), FieldValue::Text(value.into()));
        self
    }

    /// Add a byte field.
    pub fn with_bytes(mut self, name: impl Into<String>, value: impl Into<Vec<u8>>) -> Self {
        self.fields
            .insert(name.into(), FieldValue::Bytes(value.into()));
        self
    }

    /// Validate the document against a schema.
    pub fn validate(&self, schema: &Schema) -> crate::Result<()> {
        schema.validate_document(&self.fields)
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper to encode a stored document to bytes.
pub fn encode_document(doc: &Document) -> crate::Result<Vec<u8>> {
    serde_json::to_vec(doc).map_err(crate::Error::corruption)
}

/// Helper to decode a stored document from bytes.
pub fn decode_document(bytes: &[u8]) -> crate::Result<Document> {
    serde_json::from_slice(bytes).map_err(crate::Error::corruption)
}

//! Record format for values stored in `storage-index`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Current record format version byte.
pub const RECORD_VERSION: u8 = 0x01;

/// A columnar value that can participate in secondary indexes.
///
/// Values written through `IndexEngine::put` are encoded as `Record`. Values
/// written by foreign callers that do not start with [`RECORD_VERSION`] are
/// stored as opaque primary records and ignored by the index machinery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// Column name → raw column bytes.
    pub columns: HashMap<String, Vec<u8>>,
}

impl Record {
    /// Create an empty record.
    pub fn new() -> Self {
        Self {
            columns: HashMap::new(),
        }
    }

    /// Add a column.
    pub fn with_column(mut self, name: impl Into<String>, value: impl Into<Vec<u8>>) -> Self {
        self.columns.insert(name.into(), value.into());
        self
    }

    /// Encode a record to bytes, prefixing the payload with the record version.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + bincode::serialized_size(self).unwrap_or(0) as usize);
        out.push(RECORD_VERSION);
        // `unwrap` is safe: the type is serializable by construction.
        bincode::serialize_into(&mut out, self).unwrap();
        out
    }

    /// Decode a record from bytes. Returns `None` if the bytes do not represent
    /// a record (wrong version byte or malformed payload).
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.first()? != &RECORD_VERSION {
            return None;
        }
        bincode::deserialize(&bytes[1..]).ok()
    }
}

impl Default for Record {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let record = Record::new()
            .with_column("id", b"123")
            .with_column("name", b"alice");
        let encoded = record.encode();
        let decoded = Record::decode(&encoded).unwrap();
        assert_eq!(record, decoded);
    }

    #[test]
    fn opaque_value_rejected() {
        assert!(Record::decode(b"not a record").is_none());
    }
}

//! Terminal leaf node.

/// A leaf stores the complete key and value as opaque byte sequences.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Leaf {
    /// The complete key.
    pub key: Box<[u8]>,
    /// The value associated with the key.
    pub value: Box<[u8]>,
}

impl Leaf {
    /// Create a new leaf.
    pub fn new(key: Box<[u8]>, value: Box<[u8]>) -> Self {
        Self { key, value }
    }

    /// Key bytes.
    pub fn key(&self) -> &[u8] {
        &self.key
    }

    /// Value bytes.
    pub fn value(&self) -> &[u8] {
        &self.value
    }
}

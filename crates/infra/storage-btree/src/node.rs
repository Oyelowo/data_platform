//! In-memory structured representation of B+ tree nodes.

use bytes::{Buf, Bytes, BytesMut};

use crate::error::{Error, Result};
use crate::page::{NULL_PAGE_ID, PAGE_MAGIC, Page, PageHeader, PageId, PageType};

/// Value stored in a leaf entry. Overflow values are stored in a chain of
/// overflow pages starting at `head`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Value {
    /// Inline value bytes.
    Inline(Bytes),
    /// Head page id of an overflow chain.
    Overflow(PageId),
}

impl Value {
    /// Return the serialized size of this value.
    pub fn serialized_size(&self) -> usize {
        match self {
            Value::Inline(bytes) => 1 + 4 + bytes.len(),
            Value::Overflow(_) => 1 + 4 + 8,
        }
    }
}

/// An in-memory B+ tree node.
#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub id: PageId,
    pub kind: NodeKind,
}

/// The payload of a node.
#[derive(Clone, Debug)]
pub(crate) enum NodeKind {
    /// Leaf node storing sorted key/value pairs.
    Leaf {
        /// Page id of the next leaf at the same level, or `NULL_PAGE_ID`.
        next_leaf: PageId,
        /// Sorted `(key, value)` entries.
        entries: Vec<(Bytes, Value)>,
    },
    /// Internal node storing sorted separator keys and child page ids.
    ///
    /// Invariant: `entries[0].0` is always empty and `entries[0].1` is the
    /// leftmost child. For `i > 0`, `entries[i].0` is the separator key and
    /// `entries[i].1` is the child covering keys `>= entries[i].0` and
    /// `< entries[i + 1].0` (or unbounded for the last child).
    Internal { entries: Vec<(Bytes, PageId)> },
}

impl Node {
    /// Create a new empty leaf node.
    pub fn empty_leaf(id: PageId) -> Self {
        Self {
            id,
            kind: NodeKind::Leaf {
                next_leaf: NULL_PAGE_ID,
                entries: Vec::new(),
            },
        }
    }

    /// Create a new empty internal node with a single leftmost child.
    pub fn empty_internal(id: PageId, leftmost_child: PageId) -> Self {
        Self {
            id,
            kind: NodeKind::Internal {
                entries: vec![(Bytes::new(), leftmost_child)],
            },
        }
    }

    /// True if this is a leaf node.
    pub fn is_leaf(&self) -> bool {
        matches!(self.kind, NodeKind::Leaf { .. })
    }

    /// Deserialize a page into a node.
    pub fn from_page(page: &Page) -> Result<Self> {
        let header = page.header()?;
        let mut view = &page.data[PageHeader::SIZE..];
        let kind = match header.page_type {
            PageType::Leaf => {
                let next_leaf = view.get_u64_le();
                let mut entries = Vec::with_capacity(header.entry_count as usize);
                for _ in 0..header.entry_count {
                    let key_len = view.get_u16_le() as usize;
                    let key = Bytes::copy_from_slice(&view[..key_len]);
                    view.advance(key_len);
                    let value_type = view.get_u8();
                    let value = match value_type {
                        0 => {
                            let value_len = view.get_u32_le() as usize;
                            let bytes = Bytes::copy_from_slice(&view[..value_len]);
                            view.advance(value_len);
                            Value::Inline(bytes)
                        }
                        1 => {
                            let _padding = view.get_u32_le();
                            let head = view.get_u64_le();
                            Value::Overflow(head)
                        }
                        _ => {
                            return Err(Error::Corruption(format!(
                                "unknown leaf value type {value_type}"
                            )));
                        }
                    };
                    entries.push((key, value));
                }
                NodeKind::Leaf { next_leaf, entries }
            }
            PageType::Internal => {
                let mut entries = Vec::with_capacity(header.entry_count as usize);
                for _ in 0..header.entry_count {
                    let key_len = view.get_u16_le() as usize;
                    let key = Bytes::copy_from_slice(&view[..key_len]);
                    view.advance(key_len);
                    let child = view.get_u64_le();
                    entries.push((key, child));
                }
                NodeKind::Internal { entries }
            }
            PageType::Overflow => {
                return Err(Error::Corruption(
                    "overflow pages are not represented as nodes".into(),
                ));
            }
        };
        Ok(Self { id: page.id, kind })
    }

    /// Serialize this node into a page body buffer.
    ///
    /// `body` is resized to `page_size - 4` and the checksum is not included.
    pub fn serialize(&self, body: &mut BytesMut, page_size: usize) -> Result<()> {
        body.resize(page_size - 4, 0);
        let mut offset = 0usize;

        // Common header fields.
        body[offset..offset + 4].copy_from_slice(&PAGE_MAGIC.to_le_bytes());
        offset += 4;
        body[offset..offset + 2].copy_from_slice(&crate::page::PAGE_FORMAT_VERSION.to_le_bytes());
        offset += 2;

        match &self.kind {
            NodeKind::Leaf { next_leaf, entries } => {
                body[offset] = PageType::Leaf.encode();
                offset += 2; // skip flags byte
                body[offset..offset + 2].copy_from_slice(&(entries.len() as u16).to_le_bytes());
                offset += 2;
                let used = 8 + entries
                    .iter()
                    .map(|(k, v)| 2 + k.len() + v.serialized_size())
                    .sum::<usize>();
                let free_space = page_size
                    .saturating_sub(PageHeader::SIZE)
                    .saturating_sub(used)
                    .min(u16::MAX as usize) as u16;
                body[offset..offset + 2].copy_from_slice(&free_space.to_le_bytes());
                offset += 2;

                body[offset..offset + 8].copy_from_slice(&next_leaf.to_le_bytes());
                offset += 8;
                for (key, value) in entries {
                    body[offset..offset + 2].copy_from_slice(&(key.len() as u16).to_le_bytes());
                    offset += 2;
                    body[offset..offset + key.len()].copy_from_slice(key);
                    offset += key.len();
                    match value {
                        Value::Inline(bytes) => {
                            body[offset] = 0;
                            offset += 1;
                            body[offset..offset + 4]
                                .copy_from_slice(&(bytes.len() as u32).to_le_bytes());
                            offset += 4;
                            body[offset..offset + bytes.len()].copy_from_slice(bytes);
                            offset += bytes.len();
                        }
                        Value::Overflow(head) => {
                            body[offset] = 1;
                            offset += 1;
                            body[offset..offset + 4].copy_from_slice(&0u32.to_le_bytes());
                            offset += 4;
                            body[offset..offset + 8].copy_from_slice(&head.to_le_bytes());
                            offset += 8;
                        }
                    }
                }
            }
            NodeKind::Internal { entries } => {
                body[offset] = PageType::Internal.encode();
                offset += 2; // skip flags byte
                body[offset..offset + 2].copy_from_slice(&(entries.len() as u16).to_le_bytes());
                offset += 2;
                let used = entries.iter().map(|(k, _)| 2 + k.len() + 8).sum::<usize>();
                let free_space = page_size
                    .saturating_sub(PageHeader::SIZE)
                    .saturating_sub(used)
                    .min(u16::MAX as usize) as u16;
                body[offset..offset + 2].copy_from_slice(&free_space.to_le_bytes());
                offset += 2;

                for (key, child) in entries {
                    body[offset..offset + 2].copy_from_slice(&(key.len() as u16).to_le_bytes());
                    offset += 2;
                    body[offset..offset + key.len()].copy_from_slice(key);
                    offset += key.len();
                    body[offset..offset + 8].copy_from_slice(&child.to_le_bytes());
                    offset += 8;
                }
            }
        }
        Ok(())
    }

    /// Return the size required to store an entry in a leaf node.
    pub fn leaf_entry_size(key_len: usize, value: &Value) -> usize {
        2 + key_len + value.serialized_size()
    }

    /// Return the size required to store an entry in an internal node.
    pub fn internal_entry_size(key_len: usize) -> usize {
        2 + key_len + 8
    }
}

/// Build an overflow page body.
pub(crate) fn build_overflow_body(next: PageId, data: &[u8], page_size: usize) -> Result<BytesMut> {
    let mut body = BytesMut::with_capacity(page_size - 4);
    body.resize(page_size - 4, 0);
    body[0..4].copy_from_slice(&PAGE_MAGIC.to_le_bytes());
    body[4..6].copy_from_slice(&crate::page::PAGE_FORMAT_VERSION.to_le_bytes());
    body[6] = PageType::Overflow.encode();
    body[8..10].copy_from_slice(&0u16.to_le_bytes()); // entry_count not used
    body[10..12].copy_from_slice(&0u16.to_le_bytes()); // free_space not used
    let mut offset = PageHeader::SIZE;
    body[offset..offset + 8].copy_from_slice(&next.to_le_bytes());
    offset += 8;
    body[offset..offset + 4].copy_from_slice(&(data.len() as u32).to_le_bytes());
    offset += 4;
    let data_len = data.len().min(body.len() - offset);
    body[offset..offset + data_len].copy_from_slice(&data[..data_len]);
    Ok(body)
}

/// Decode an overflow page: returns `(next_page_id, payload_bytes)`.
pub(crate) fn decode_overflow_page(page: &Page) -> Result<(PageId, Bytes)> {
    let header = page.header()?;
    if header.page_type != PageType::Overflow {
        return Err(Error::Corruption("expected overflow page".into()));
    }
    let mut view = &page.data[PageHeader::SIZE..];
    let next = view.get_u64_le();
    let data_len = view.get_u32_le() as usize;
    let payload = Bytes::copy_from_slice(&view[..data_len]);
    Ok((next, payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaf_node_roundtrip() {
        let node = Node {
            id: 1,
            kind: NodeKind::Leaf {
                next_leaf: 42,
                entries: vec![
                    (
                        Bytes::from_static(b"a"),
                        Value::Inline(Bytes::from_static(b"1")),
                    ),
                    (Bytes::from_static(b"b"), Value::Overflow(99)),
                ],
            },
        };
        let mut body = BytesMut::new();
        node.serialize(&mut body, 4096).unwrap();
        let page = Page::build(1, body, 4096).unwrap();
        let decoded = Node::from_page(&page).unwrap();
        assert_eq!(decoded.id, 1);
        match decoded.kind {
            NodeKind::Leaf { next_leaf, entries } => {
                assert_eq!(next_leaf, 42);
                assert_eq!(entries.len(), 2);
                assert_eq!(entries[0].0, Bytes::from_static(b"a"));
                assert_eq!(entries[0].1, Value::Inline(Bytes::from_static(b"1")));
                assert_eq!(entries[1].1, Value::Overflow(99));
            }
            _ => panic!("expected leaf"),
        }
    }

    #[test]
    fn internal_node_roundtrip() {
        let node = Node {
            id: 2,
            kind: NodeKind::Internal {
                entries: vec![
                    (Bytes::new(), 5),
                    (Bytes::from_static(b"m"), 10),
                    (Bytes::from_static(b"t"), 20),
                ],
            },
        };
        let mut body = BytesMut::new();
        node.serialize(&mut body, 4096).unwrap();
        let page = Page::build(2, body, 4096).unwrap();
        let decoded = Node::from_page(&page).unwrap();
        match decoded.kind {
            NodeKind::Internal { entries } => {
                assert_eq!(entries.len(), 3);
                assert_eq!(entries[0].1, 5);
                assert_eq!(entries[2].1, 20);
            }
            _ => panic!("expected internal"),
        }
    }
}

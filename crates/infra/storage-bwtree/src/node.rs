//! Delta-chain logic: search, consolidation, and size accounting.

use bytes::Bytes;

use crate::options::BwTreeOptions;
use crate::page::{
    BaseNode, DeltaKind, InnerBase, LeafBase, NULL_PID, NodeHeader, PageState, Payload, Pid, Value,
};

/// Result of searching a leaf chain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeafSearchResult {
    /// Key found with this value.
    Found(Value),
    /// Key explicitly deleted at this point in the chain.
    Deleted,
    /// No decision in the chain; fall back to base node search.
    NeedBase,
}

/// Search a leaf delta chain for `key`. Deltas are applied from newest to
/// oldest; the first matching insert or delete wins.
pub fn search_leaf_chain(state: &PageState, key: &[u8]) -> LeafSearchResult {
    let mut current = Some(state);
    while let Some(page) = current {
        match &page.payload {
            Payload::Base(BaseNode::Leaf(base)) => {
                return search_leaf_base(base, key);
            }
            Payload::Base(_) => {
                return LeafSearchResult::NeedBase;
            }
            Payload::Delta(DeltaKind::Insert { key: k, value }) if k.as_ref() == key => {
                return LeafSearchResult::Found(value.clone());
            }
            Payload::Delta(DeltaKind::Delete { key: k }) if k.as_ref() == key => {
                return LeafSearchResult::Deleted;
            }
            Payload::Delta(DeltaKind::Split { .. })
            | Payload::Delta(DeltaKind::Merge { .. })
            | Payload::Delta(DeltaKind::Remove { .. })
            | Payload::Delta(DeltaKind::Separator { .. })
            | Payload::Delta(DeltaKind::Abort)
            | Payload::Delta(DeltaKind::Insert { .. })
            | Payload::Delta(DeltaKind::Delete { .. }) => {
                // Continue walking.
            }
        }
        current = unsafe { page.next.as_ref() };
    }
    LeafSearchResult::NeedBase
}

fn search_leaf_base(base: &LeafBase, key: &[u8]) -> LeafSearchResult {
    match base.entries.binary_search_by(|(k, _)| k.as_ref().cmp(key)) {
        Ok(idx) => LeafSearchResult::Found(base.entries[idx].1.clone()),
        Err(_) => LeafSearchResult::Deleted,
    }
}

/// Build the logical view of a leaf chain as a sorted vector of entries.
///
/// Deltas are applied newest-to-oldest: the first delta seen for a key wins,
/// so a delete followed by a newer insert leaves the key present, while an
/// insert followed by a newer delete removes it.
pub fn logical_leaf_entries(state: &PageState) -> Vec<(Bytes, Value)> {
    let mut overrides: std::collections::BTreeMap<Bytes, Option<Value>> =
        std::collections::BTreeMap::new();
    let mut base: Option<&LeafBase> = None;

    let mut current = Some(state);
    while let Some(page) = current {
        match &page.payload {
            Payload::Base(BaseNode::Leaf(leaf)) => {
                base = Some(leaf);
                break;
            }
            Payload::Base(BaseNode::Inner(_)) => break,
            Payload::Delta(DeltaKind::Insert { key, value }) => {
                overrides
                    .entry(key.clone())
                    .or_insert_with(|| Some(value.clone()));
            }
            Payload::Delta(DeltaKind::Delete { key }) => {
                overrides.entry(key.clone()).or_insert_with(|| None);
            }
            _ => {}
        }
        current = unsafe { page.next.as_ref() };
    }

    if let Some(base) = base {
        for (key, value) in &base.entries {
            overrides
                .entry(key.clone())
                .or_insert_with(|| Some(value.clone()));
        }
    }

    overrides
        .into_iter()
        .filter_map(|(key, value)| value.map(|v| (key, v)))
        .collect()
}

/// Build the logical view of an inner chain as a sorted vector of separator
/// entries. The first entry always has an empty separator and is the leftmost
/// child.
pub fn logical_inner_entries(state: &PageState) -> Vec<(Bytes, Pid)> {
    let mut separators: Vec<(Bytes, Pid)> = Vec::new();
    let mut deletes: Vec<Bytes> = Vec::new();
    let mut base: Option<&InnerBase> = None;

    let mut current = Some(state);
    while let Some(page) = current {
        match &page.payload {
            Payload::Base(BaseNode::Inner(inner)) => {
                base = Some(inner);
                break;
            }
            Payload::Base(BaseNode::Leaf(_)) => break,
            Payload::Delta(DeltaKind::Separator {
                separator_key,
                new_child,
                next_separator_key,
            }) => {
                separators.push((separator_key.clone(), *new_child));
                if !next_separator_key.is_empty() {
                    deletes.push(next_separator_key.clone());
                }
            }
            Payload::Delta(DeltaKind::Split {
                split_key,
                new_right_sibling,
            }) => {
                // Split delta on an inner node is not expected in this
                // simplified protocol; treat as separator.
                separators.push((split_key.clone(), *new_right_sibling));
            }
            _ => {}
        }
        current = unsafe { page.next.as_ref() };
    }

    separators.sort_by(|a, b| a.0.cmp(&b.0));
    let mut deduped: Vec<(Bytes, Pid)> = Vec::new();
    for (key, child) in separators {
        if let Some(last) = deduped.last_mut()
            && last.0 == key
        {
            // Newest mapping is already present; drop the older duplicate.
            let _ = child;
            continue;
        }
        deduped.push((key, child));
    }

    let mut result = Vec::new();
    if let Some(base) = base {
        let base_iter = base.entries.iter();
        let mut sep_iter = deduped.iter().peekable();
        let mut del_iter = deletes.iter().peekable();

        // The leftmost child is stored separately in InnerBase.
        result.push((Bytes::new(), base.leftmost_child));

        for (key, child) in base_iter {
            // Apply separator deletes.
            while let Some(del) = del_iter.peek() {
                if del.as_ref() < key.as_ref() {
                    del_iter.next();
                } else {
                    break;
                }
            }
            if del_iter.peek() == Some(&key) {
                del_iter.next();
                continue;
            }
            // Merge separators that belong before this base separator.
            while let Some((sep_key, sep_child)) = sep_iter.peek() {
                if sep_key.as_ref() < key.as_ref() {
                    result.push(((*sep_key).clone(), *sep_child));
                    sep_iter.next();
                } else {
                    break;
                }
            }
            if let Some((sep_key, sep_child)) = sep_iter.peek()
                && sep_key.as_ref() == key.as_ref()
            {
                result.push(((*sep_key).clone(), *sep_child));
                sep_iter.next();
                continue;
            }
            result.push((key.clone(), *child));
        }
        for (sep_key, sep_child) in sep_iter {
            result.push((sep_key.clone(), *sep_child));
        }
    } else {
        for (sep_key, sep_child) in deduped {
            if !deletes.iter().any(|d| d.as_ref() == sep_key.as_ref()) {
                result.push((sep_key, sep_child));
            }
        }
    }

    // Ensure the leftmost separator is empty.
    if !result.is_empty() && !result[0].0.is_empty() {
        result.insert(0, (Bytes::new(), result[0].1));
    }
    result
}

/// Consolidate a delta chain into a new base node, returning the new head and
/// the old head. The caller is responsible for retiring the old head after a
/// successful CAS.
pub fn consolidate(state: &PageState, _options: &BwTreeOptions) -> (PageState, *mut PageState) {
    match state.payload {
        Payload::Base(BaseNode::Leaf(_)) | Payload::Base(BaseNode::Inner(_)) => {
            // Already a base node; nothing to do.
            // Return a copy of the state (caller will not use it for CAS).
            let copy = copy_state(state);
            let old = state as *const PageState as *mut PageState;
            (copy, old)
        }
        _ => {
            let old = state as *const PageState as *mut PageState;
            let depth = state.header.depth;
            if depth == 0 {
                let entries = logical_leaf_entries(state);
                let header = NodeHeader {
                    low_key: state.header.low_key.clone(),
                    high_key: state.header.high_key.clone(),
                    right_sibling: state.header.right_sibling,
                    item_count: entries.len() as u32,
                    depth: 0,
                    delta_chain_length: 0,
                };
                let base = BaseNode::Leaf(LeafBase { entries });
                (
                    PageState::new(header, Payload::Base(base), std::ptr::null_mut(), state.lsn),
                    old,
                )
            } else {
                let entries = logical_inner_entries(state);
                let leftmost_child = entries.first().map(|(_, c)| *c).unwrap_or(NULL_PID);
                let header = NodeHeader {
                    low_key: state.header.low_key.clone(),
                    high_key: state.header.high_key.clone(),
                    right_sibling: state.header.right_sibling,
                    item_count: entries.len().saturating_sub(1) as u32,
                    depth,
                    delta_chain_length: 0,
                };
                let base = BaseNode::Inner(InnerBase {
                    entries: entries.into_iter().skip(1).collect(),
                    leftmost_child,
                });
                (
                    PageState::new(header, Payload::Base(base), std::ptr::null_mut(), state.lsn),
                    old,
                )
            }
        }
    }
}

fn copy_state(state: &PageState) -> PageState {
    let payload = match &state.payload {
        Payload::Base(BaseNode::Leaf(leaf)) => Payload::Base(BaseNode::Leaf(LeafBase {
            entries: leaf.entries.clone(),
        })),
        Payload::Base(BaseNode::Inner(inner)) => Payload::Base(BaseNode::Inner(InnerBase {
            entries: inner.entries.clone(),
            leftmost_child: inner.leftmost_child,
        })),
        Payload::Delta(_delta) => Payload::Base(BaseNode::Leaf(LeafBase {
            entries: Vec::new(),
        })),
    };
    PageState::new(
        NodeHeader {
            low_key: state.header.low_key.clone(),
            high_key: state.header.high_key.clone(),
            right_sibling: state.header.right_sibling,
            item_count: state.header.item_count,
            depth: state.header.depth,
            delta_chain_length: state.header.delta_chain_length,
        },
        payload,
        std::ptr::null_mut(),
        state.lsn,
    )
}

/// Compute the serialized size of a logical leaf node.
pub fn leaf_size(entries: &[(Bytes, Value)]) -> usize {
    // Overhead for the base node: next_leaf pointer + per-entry length prefix.
    let mut size = 8usize;
    for (key, value) in entries {
        size += 2 + key.len() + value.serialized_size();
    }
    size
}

/// Compute the serialized size of a logical inner node.
#[allow(dead_code)]
pub fn inner_size(entries: &[(Bytes, Pid)]) -> usize {
    // First entry has empty separator; count all entries including leftmost.
    entries.iter().map(|(k, _)| 2 + k.len() + 8).sum::<usize>()
}

/// Return true if the leaf node should split.
pub fn leaf_needs_split(entries: &[(Bytes, Value)], options: &BwTreeOptions) -> bool {
    leaf_size(entries) > options.node_size_threshold()
}

/// Return true if the leaf node is below the minimum fill ratio.
#[allow(dead_code)]
pub fn leaf_needs_merge(entries: &[(Bytes, Value)], options: &BwTreeOptions) -> bool {
    leaf_size(entries) < options.min_node_size() && entries.len() > 1
}

/// Return true if the inner node is below the minimum fill ratio.
#[allow(dead_code)]
pub fn inner_needs_merge(entries: &[(Bytes, Pid)], options: &BwTreeOptions) -> bool {
    inner_size(entries) < options.min_node_size() && entries.len() > 1
}

/// Return true if the inner node should split.
pub fn inner_needs_split(entries: &[(Bytes, Pid)], options: &BwTreeOptions) -> bool {
    inner_size(entries) > options.node_size_threshold()
}

/// Pick the child PID for `key` from an inner node's logical entries.
pub fn child_for_key(entries: &[(Bytes, Pid)], key: &[u8]) -> Pid {
    let mut child = entries.first().map(|(_, c)| *c).unwrap_or(NULL_PID);
    for (sep, cid) in entries.iter().skip(1) {
        if key < sep.as_ref() {
            return child;
        }
        child = *cid;
    }
    child
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_leaf_base(entries: Vec<(Bytes, Value)>) -> PageState {
        PageState::new(
            NodeHeader::default(),
            Payload::Base(BaseNode::Leaf(LeafBase { entries })),
            std::ptr::null_mut(),
            0,
        )
    }

    fn make_leaf_delta(kind: DeltaKind, next: *mut PageState) -> PageState {
        PageState::new(NodeHeader::default(), Payload::Delta(kind), next, 1)
    }

    #[test]
    fn search_leaf_base_found() {
        let state = make_leaf_base(vec![
            (
                Bytes::from_static(b"a"),
                Value::Inline(Bytes::from_static(b"1")),
            ),
            (
                Bytes::from_static(b"b"),
                Value::Inline(Bytes::from_static(b"2")),
            ),
        ]);
        assert_eq!(
            search_leaf_chain(&state, b"a"),
            LeafSearchResult::Found(Value::Inline(Bytes::from_static(b"1")))
        );
        assert_eq!(
            search_leaf_chain(&state, b"b"),
            LeafSearchResult::Found(Value::Inline(Bytes::from_static(b"2")))
        );
        assert_eq!(search_leaf_chain(&state, b"c"), LeafSearchResult::Deleted);
    }

    #[test]
    fn search_leaf_delta_wins() {
        let base = Box::into_raw(Box::new(make_leaf_base(vec![(
            Bytes::from_static(b"a"),
            Value::Inline(Bytes::from_static(b"1")),
        )])));
        let head = Box::into_raw(Box::new(make_leaf_delta(
            DeltaKind::Insert {
                key: Bytes::from_static(b"b"),
                value: Value::Inline(Bytes::from_static(b"3")),
            },
            base,
        )));
        assert_eq!(
            search_leaf_chain(unsafe { &*head }, b"b"),
            LeafSearchResult::Found(Value::Inline(Bytes::from_static(b"3")))
        );

        let head2 = Box::into_raw(Box::new(make_leaf_delta(
            DeltaKind::Delete {
                key: Bytes::from_static(b"a"),
            },
            head,
        )));
        assert_eq!(
            search_leaf_chain(unsafe { &*head2 }, b"a"),
            LeafSearchResult::Deleted
        );
        assert_eq!(
            search_leaf_chain(unsafe { &*head2 }, b"b"),
            LeafSearchResult::Found(Value::Inline(Bytes::from_static(b"3")))
        );

        unsafe {
            let _ = Box::from_raw(head2);
        }
    }

    #[test]
    fn logical_leaf_merge_and_delete() {
        let base = Box::into_raw(Box::new(make_leaf_base(vec![
            (
                Bytes::from_static(b"a"),
                Value::Inline(Bytes::from_static(b"1")),
            ),
            (
                Bytes::from_static(b"c"),
                Value::Inline(Bytes::from_static(b"3")),
            ),
        ])));
        let head = Box::into_raw(Box::new(make_leaf_delta(
            DeltaKind::Insert {
                key: Bytes::from_static(b"b"),
                value: Value::Inline(Bytes::from_static(b"2")),
            },
            base,
        )));
        let head2 = Box::into_raw(Box::new(make_leaf_delta(
            DeltaKind::Delete {
                key: Bytes::from_static(b"a"),
            },
            head,
        )));
        let entries = logical_leaf_entries(unsafe { &*head2 });
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0.as_ref(), b"b");
        assert_eq!(entries[1].0.as_ref(), b"c");
        unsafe {
            let _ = Box::from_raw(head2);
        }
    }

    #[test]
    fn consolidate_leaf() {
        let base = Box::into_raw(Box::new(make_leaf_base(vec![(
            Bytes::from_static(b"a"),
            Value::Inline(Bytes::from_static(b"1")),
        )])));
        let head = Box::into_raw(Box::new(make_leaf_delta(
            DeltaKind::Insert {
                key: Bytes::from_static(b"b"),
                value: Value::Inline(Bytes::from_static(b"2")),
            },
            base,
        )));
        let options = BwTreeOptions::default();
        let (new_state, _) = consolidate(unsafe { &*head }, &options);
        match &new_state.payload {
            Payload::Base(BaseNode::Leaf(leaf)) => {
                assert_eq!(leaf.entries.len(), 2);
            }
            _ => panic!("expected leaf base"),
        }
        unsafe {
            let _ = Box::from_raw(head);
        }
    }

    #[test]
    fn consolidate_delete() {
        let base = Box::into_raw(Box::new(make_leaf_base(vec![
            (
                Bytes::from_static(b"a"),
                Value::Inline(Bytes::from_static(b"1")),
            ),
            (
                Bytes::from_static(b"b"),
                Value::Inline(Bytes::from_static(b"2")),
            ),
            (
                Bytes::from_static(b"c"),
                Value::Inline(Bytes::from_static(b"3")),
            ),
        ])));
        let head = Box::into_raw(Box::new(make_leaf_delta(
            DeltaKind::Delete {
                key: Bytes::from_static(b"b"),
            },
            base,
        )));
        let options = BwTreeOptions::default();
        let (new_state, _) = consolidate(unsafe { &*head }, &options);
        match &new_state.payload {
            Payload::Base(BaseNode::Leaf(leaf)) => {
                assert_eq!(leaf.entries.len(), 2);
                assert_eq!(leaf.entries[0].0.as_ref(), b"a");
                assert_eq!(leaf.entries[1].0.as_ref(), b"c");
            }
            _ => panic!("expected leaf base"),
        }
        unsafe {
            let _ = Box::from_raw(head);
        }
    }

    #[test]
    fn consolidate_insert_then_delete() {
        let base = Box::into_raw(Box::new(make_leaf_base(vec![])));
        let head = Box::into_raw(Box::new(make_leaf_delta(
            DeltaKind::Insert {
                key: Bytes::from_static(b"a"),
                value: Value::Inline(Bytes::from_static(b"1")),
            },
            base,
        )));
        let head = Box::into_raw(Box::new(make_leaf_delta(
            DeltaKind::Insert {
                key: Bytes::from_static(b"b"),
                value: Value::Inline(Bytes::from_static(b"2")),
            },
            head,
        )));
        let head = Box::into_raw(Box::new(make_leaf_delta(
            DeltaKind::Insert {
                key: Bytes::from_static(b"c"),
                value: Value::Inline(Bytes::from_static(b"3")),
            },
            head,
        )));
        let head = Box::into_raw(Box::new(make_leaf_delta(
            DeltaKind::Delete {
                key: Bytes::from_static(b"b"),
            },
            head,
        )));
        let options = BwTreeOptions::default();
        let (new_state, _) = consolidate(unsafe { &*head }, &options);
        match &new_state.payload {
            Payload::Base(BaseNode::Leaf(leaf)) => {
                assert_eq!(leaf.entries.len(), 2);
                assert_eq!(leaf.entries[0].0.as_ref(), b"a");
                assert_eq!(leaf.entries[1].0.as_ref(), b"c");
            }
            _ => panic!("expected leaf base"),
        }
        unsafe {
            let _ = Box::from_raw(head);
        }
    }
}

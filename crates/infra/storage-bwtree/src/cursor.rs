//! Ordered iteration over key-value pairs in a Bw-Tree.

use std::sync::Arc;

use bytes::Bytes;
use crossbeam_epoch::{self as epoch};

use crate::engine::EngineInner;
use crate::error::{Error, Result};
use crate::node::logical_leaf_entries;
use crate::page::{NULL_PID, Pid, Value};

/// Cursor over a key range in the Bw-Tree.
///
/// The cursor captures the root PID at creation time and traverses immutable
/// chain snapshots, so it presents a stable view even if the engine is modified
/// concurrently.
pub struct BwTreeCursor {
    inner: Arc<EngineInner>,
    root: Pid,
    end: Option<Bytes>,
    current_pid: Pid,
    current_entries: Vec<(Bytes, Value)>,
    pos: usize,
    exhausted: bool,
}

impl BwTreeCursor {
    pub(crate) fn new(
        inner: Arc<EngineInner>,
        root: Pid,
        start: Option<Bytes>,
        end: Option<Bytes>,
    ) -> Result<Self> {
        let mut cursor = Self {
            inner,
            root,
            end,
            current_pid: NULL_PID,
            current_entries: Vec::new(),
            pos: 0,
            exhausted: root == NULL_PID,
        };
        if root != NULL_PID {
            cursor.seek_to(root, start.as_deref())?;
        }
        Ok(cursor)
    }

    fn seek_to(&mut self, root: Pid, target: Option<&[u8]>) -> Result<()> {
        let guard = epoch::pin();
        let result = self.seek_recursive(root, target, &guard);
        drop(guard);
        result
    }

    // `guard` is held across recursive calls to keep the epoch pin alive.
    #[allow(clippy::only_used_in_recursion)]
    fn seek_recursive(
        &mut self,
        pid: Pid,
        target: Option<&[u8]>,
        guard: &epoch::Guard,
    ) -> Result<()> {
        if pid == NULL_PID {
            self.exhausted = true;
            return Ok(());
        }
        let state_ptr = self
            .inner
            .mapping_table
            .load(pid)
            .ok_or_else(|| Error::Corruption(format!("missing page {pid}")))?;
        let state = unsafe { &*state_ptr };

        if state.header.depth == 0 {
            // Leaf.
            let entries = logical_leaf_entries(state);
            let idx = target.map_or(0, |t| entries.partition_point(|(k, _)| k.as_ref() < t));
            self.current_pid = pid;
            self.current_entries = entries;
            self.pos = idx;
            self.exhausted =
                state.header.right_sibling.is_none() && self.pos >= self.current_entries.len();
            Ok(())
        } else {
            let entries = crate::node::logical_inner_entries(state);
            let child = crate::node::child_for_key(&entries, target.unwrap_or(&[]));
            if child == NULL_PID {
                self.exhausted = true;
                return Ok(());
            }
            self.seek_recursive(child, target, guard)
        }
    }

    fn advance_leaf(&mut self) -> Result<bool> {
        if self.current_pid == NULL_PID {
            return Ok(false);
        }
        let guard = epoch::pin();
        let state_ptr = self
            .inner
            .mapping_table
            .load(self.current_pid)
            .ok_or_else(|| Error::Corruption(format!("missing page {}", self.current_pid)))?;
        let state = unsafe { &*state_ptr };
        if let Some(right) = state.header.right_sibling {
            let result = self.seek_recursive(right, None, &guard);
            drop(guard);
            result?;
            Ok(!self.exhausted)
        } else {
            self.exhausted = true;
            drop(guard);
            Ok(false)
        }
    }

    fn resolve_value(&self, value: Value) -> Result<Bytes> {
        match value {
            Value::Inline(bytes) => Ok(bytes),
            Value::Overflow(offset) => self.inner.overflow.read(offset),
        }
    }
}

impl Iterator for BwTreeCursor {
    type Item = Result<(Bytes, Bytes)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.exhausted {
            return None;
        }
        loop {
            if self.pos >= self.current_entries.len() {
                match self.advance_leaf() {
                    Ok(true) => continue,
                    Ok(false) => return None,
                    Err(e) => return Some(Err(e)),
                }
            }
            let (key, value) = &self.current_entries[self.pos];
            if let Some(ref end) = self.end
                && key.as_ref() >= end.as_ref()
            {
                self.exhausted = true;
                return None;
            }
            let key = key.clone();
            let value = match self.resolve_value(value.clone()) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            self.pos += 1;
            return Some(Ok((key, value)));
        }
    }
}

impl storage_traits::Cursor for BwTreeCursor {
    type Error = Error;

    fn seek(&mut self, target: &[u8]) -> Result<()> {
        self.exhausted = false;
        if self.root == NULL_PID {
            self.exhausted = true;
            return Ok(());
        }
        self.seek_to(self.root, Some(target))
    }
}

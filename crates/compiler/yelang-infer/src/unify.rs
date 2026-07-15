/*! Core union-find unification table with rollback.
 *
 * This is Yelang's equivalent of `ena`: a generic union-find data structure
 * that supports speculative exploration via snapshots and rollback.
 */

use std::fmt::Debug;

/// A key into the unification table.
pub trait UnifyKey: Copy + Eq + Debug + 'static {
    fn index(self) -> u32;
    fn from_index(index: u32) -> Self;
}

/// Data stored for each variable in the union-find table.
#[derive(Clone, Debug)]
struct VarData<V: Clone + Debug> {
    /// The parent of this variable in the union-find tree.
    /// `None` means this variable is a root.
    parent: Option<u32>,
    /// Rank for union-by-rank.
    rank: u32,
    /// The known value for this equivalence class (only meaningful for roots).
    value: V,
}

/// An entry in the undo log, recording a change so it can be reverted.
#[derive(Clone, Debug)]
enum UndoEntry<V: Clone + Debug> {
    /// A new variable was created.
    NewVar,
    /// The parent of `var` was changed.
    Parent { var: u32, old_parent: Option<u32> },
    /// The rank of `var` was changed.
    Rank { var: u32, old_rank: u32 },
    /// The value of root `var` was changed.
    Value { var: u32, old_value: V },
}

/// A generic union-find table with rollback support.
///
/// # Type Parameters
/// - `K`: The key type (e.g., `TyVid`). Must implement `UnifyKey`.
/// - `V`: The value type stored for each equivalence class.
pub struct UnificationTable<K: UnifyKey, V: Clone + Debug + PartialEq> {
    values: Vec<VarData<V>>,
    undo_log: Vec<UndoEntry<V>>,
    _marker: std::marker::PhantomData<K>,
}

/// A snapshot of the unification table state.
///
/// Create with `UnificationTable::snapshot()`, rollback with
/// `UnificationTable::rollback_to(snapshot)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    /// The length of the undo log at the time of the snapshot.
    pub(crate) undo_len: usize,
    /// The number of variables at the time of the snapshot.
    pub(crate) num_vars: usize,
}

impl<K: UnifyKey, V: Clone + Debug + PartialEq> UnificationTable<K, V> {
    pub fn new(_init_value: V) -> Self {
        Self {
            values: Vec::new(),
            undo_log: Vec::new(),
            _marker: std::marker::PhantomData,
        }
    }

    /// Create a new variable with the given initial value.
    pub fn new_var(&mut self, init_value: V) -> K {
        let index = self.values.len() as u32;
        self.values.push(VarData {
            parent: None,
            rank: 0,
            value: init_value,
        });
        self.undo_log.push(UndoEntry::NewVar);
        K::from_index(index)
    }

    /// Find the root of `key` with path compression.
    pub fn find(&mut self, key: K) -> K {
        let index = key.index();
        let parent = self.values[index as usize].parent;
        match parent {
            None => key,
            Some(p) => {
                let root = self.find(K::from_index(p));
                if root.index() != p {
                    // Path compression: update parent.
                    let old_parent = self.values[index as usize].parent;
                    self.values[index as usize].parent = Some(root.index());
                    self.undo_log.push(UndoEntry::Parent {
                        var: index,
                        old_parent,
                    });
                }
                root
            }
        }
    }

    /// Find the root without path compression (for read-only operations).
    pub fn find_without_compression(&self, key: K) -> K {
        let mut current = key;
        loop {
            let index = current.index();
            match self.values[index as usize].parent {
                None => return current,
                Some(p) => current = K::from_index(p),
            }
        }
    }

    /// Get the value of the root of `key`.
    pub fn probe_value(&mut self, key: K) -> &V {
        let root = self.find(key);
        let index = root.index();
        &self.values[index as usize].value
    }

    /// Get the value of the root without compression.
    pub fn probe_value_no_compression(&self, key: K) -> &V {
        let root = self.find_without_compression(key);
        let index = root.index();
        &self.values[index as usize].value
    }

    /// Set the value of the root of `key`.
    pub fn set_value(&mut self, key: K, value: V) {
        let root = self.find(key);
        let index = root.index();
        let old_value = self.values[index as usize].value.clone();
        self.undo_log.push(UndoEntry::Value {
            var: index,
            old_value,
        });
        self.values[index as usize].value = value;
    }

    /// Unify two variables, returning `Ok(())` on success.
    /// The caller must check values if both roots have known values.
    pub fn union(&mut self, a: K, b: K) -> Result<(), ()> {
        let root_a = self.find(a);
        let root_b = self.find(b);
        if root_a.index() == root_b.index() {
            return Ok(());
        }

        let index_a = root_a.index();
        let index_b = root_b.index();

        // Union by rank: attach the shorter tree under the taller one.
        let rank_a = self.values[index_a as usize].rank;
        let rank_b = self.values[index_b as usize].rank;

        let (child, parent) = if rank_a < rank_b {
            (root_a, root_b)
        } else if rank_a > rank_b {
            (root_b, root_a)
        } else {
            // Equal ranks: pick one as parent and increment its rank.
            (root_b, root_a)
        };

        let child_index = child.index();
        let parent_index = parent.index();

        // Record undo entries.
        let old_child_parent = self.values[child_index as usize].parent;
        self.undo_log.push(UndoEntry::Parent {
            var: child_index,
            old_parent: old_child_parent,
        });
        self.values[child_index as usize].parent = Some(parent_index);

        if rank_a == rank_b && parent.index() == index_a {
            let old_rank = self.values[parent_index as usize].rank;
            self.undo_log.push(UndoEntry::Rank {
                var: parent_index,
                old_rank,
            });
            self.values[parent_index as usize].rank += 1;
        }

        Ok(())
    }

    /// Create a snapshot of the current state.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            undo_len: self.undo_log.len(),
            num_vars: self.values.len(),
        }
    }

    /// Rollback to a previous snapshot.
    pub fn rollback_to(&mut self, snapshot: Snapshot) {
        assert!(
            snapshot.undo_len <= self.undo_log.len(),
            "cannot rollback to a future snapshot"
        );
        assert!(
            snapshot.num_vars <= self.values.len(),
            "cannot rollback to a snapshot with more variables"
        );

        // Replay undo log backwards.
        while self.undo_log.len() > snapshot.undo_len {
            let entry = self.undo_log.pop().unwrap();
            match entry {
                UndoEntry::NewVar => {
                    self.values.pop();
                }
                UndoEntry::Parent { var, old_parent } => {
                    self.values[var as usize].parent = old_parent;
                }
                UndoEntry::Rank { var, old_rank } => {
                    self.values[var as usize].rank = old_rank;
                }
                UndoEntry::Value { var, old_value } => {
                    self.values[var as usize].value = old_value;
                }
            }
        }

        // Truncate variables if needed.
        self.values.truncate(snapshot.num_vars);
    }

    /// Commit changes since a snapshot (just drops the undo log prefix).
    pub fn commit(&mut self, snapshot: Snapshot) {
        // In a full implementation, we'd drop undo entries before the snapshot.
        // For now, we just leave them; memory overhead is acceptable.
        let _ = snapshot;
    }
}

impl<K: UnifyKey, V: Clone + Debug + PartialEq + Default> Default for UnificationTable<K, V> {
    fn default() -> Self {
        Self::new(V::default())
    }
}

// ---------------------------------------------------------------------------
// Concrete key implementations
// ---------------------------------------------------------------------------

use yelang_ty::ty::{FloatVid, IntVid, TyVid};

impl UnifyKey for TyVid {
    fn index(self) -> u32 {
        self.0
    }
    fn from_index(index: u32) -> Self {
        TyVid(index)
    }
}

impl UnifyKey for IntVid {
    fn index(self) -> u32 {
        self.0
    }
    fn from_index(index: u32) -> Self {
        IntVid(index)
    }
}

impl UnifyKey for FloatVid {
    fn index(self) -> u32 {
        self.0
    }
    fn from_index(index: u32) -> Self {
        FloatVid(index)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Default)]
    enum TestValue {
        Known(i32),
        #[default]
        Unknown,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    struct TestVid(u32);

    impl UnifyKey for TestVid {
        fn index(self) -> u32 {
            self.0
        }
        fn from_index(index: u32) -> Self {
            TestVid(index)
        }
    }

    #[test]
    fn new_var() {
        let mut table: UnificationTable<TestVid, TestValue> = UnificationTable::default();
        let v1 = table.new_var(TestValue::Unknown);
        let v2 = table.new_var(TestValue::Unknown);
        assert_eq!(v1.index(), 0);
        assert_eq!(v2.index(), 1);
    }

    #[test]
    fn union_and_find() {
        let mut table: UnificationTable<TestVid, TestValue> = UnificationTable::default();
        let v1 = table.new_var(TestValue::Unknown);
        let v2 = table.new_var(TestValue::Unknown);
        table.union(v1, v2).unwrap();
        assert_eq!(table.find(v1), table.find(v2));
    }

    #[test]
    fn set_and_probe_value() {
        let mut table: UnificationTable<TestVid, TestValue> = UnificationTable::default();
        let v1 = table.new_var(TestValue::Unknown);
        table.set_value(v1, TestValue::Known(42));
        assert_eq!(*table.probe_value(v1), TestValue::Known(42));
    }

    #[test]
    fn union_preserves_value() {
        let mut table: UnificationTable<TestVid, TestValue> = UnificationTable::default();
        let v1 = table.new_var(TestValue::Known(10));
        let v2 = table.new_var(TestValue::Unknown);
        table.union(v1, v2).unwrap();
        // After union, the root should still have its value.
        let root = table.find(v1);
        assert_eq!(*table.probe_value(root), TestValue::Known(10));
    }

    #[test]
    fn snapshot_and_rollback() {
        let mut table: UnificationTable<TestVid, TestValue> = UnificationTable::default();
        let v1 = table.new_var(TestValue::Unknown);
        let snap = table.snapshot();
        let v2 = table.new_var(TestValue::Unknown);
        table.union(v1, v2).unwrap();
        table.set_value(v1, TestValue::Known(99));

        // Rollback
        table.rollback_to(snap);

        // v2 should no longer exist
        assert_eq!(table.snapshot().num_vars, 1);
        // v1 should still be Unknown
        assert_eq!(*table.probe_value(v1), TestValue::Unknown);
    }

    #[test]
    fn rollback_restores_parent() {
        let mut table: UnificationTable<TestVid, TestValue> = UnificationTable::default();
        let v1 = table.new_var(TestValue::Unknown);
        let v2 = table.new_var(TestValue::Unknown);
        let snap = table.snapshot();
        table.union(v1, v2).unwrap();
        assert_eq!(table.find(v1), table.find(v2));

        table.rollback_to(snap);
        // After rollback, v1 and v2 should be separate again.
        assert_ne!(
            table.find_without_compression(v1),
            table.find_without_compression(v2)
        );
    }

    #[test]
    fn path_compression() {
        let mut table: UnificationTable<TestVid, TestValue> = UnificationTable::default();
        let v1 = table.new_var(TestValue::Unknown);
        let v2 = table.new_var(TestValue::Unknown);
        let v3 = table.new_var(TestValue::Unknown);
        table.union(v1, v2).unwrap();
        table.union(v2, v3).unwrap();

        // Before find, v1's parent might be v2.
        // After find, v1's parent should be the root.
        let root = table.find(v1);
        assert_eq!(table.find(v2), root);
        assert_eq!(table.find(v3), root);
    }
}

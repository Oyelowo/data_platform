use crate::unify::{UnificationTable, UnifyKey};

#[derive(Clone, Debug, PartialEq)]
enum TestValue {
    Known(i32),
    Unknown,
}

impl Default for TestValue {
    fn default() -> Self {
        TestValue::Unknown
    }
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
    table.union(v1, v2);
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
    table.union(v1, v2);
    let root = table.find(v1);
    assert_eq!(*table.probe_value(root), TestValue::Known(10));
}

#[test]
fn snapshot_and_rollback() {
    let mut table: UnificationTable<TestVid, TestValue> = UnificationTable::default();
    let v1 = table.new_var(TestValue::Unknown);
    let snap = table.snapshot();
    let v2 = table.new_var(TestValue::Unknown);
    table.union(v1, v2);
    table.set_value(v1, TestValue::Known(99));

    table.rollback_to(snap);

    assert_eq!(table.snapshot().num_vars, 1);
    assert_eq!(*table.probe_value(v1), TestValue::Unknown);
}

#[test]
fn rollback_restores_parent() {
    let mut table: UnificationTable<TestVid, TestValue> = UnificationTable::default();
    let v1 = table.new_var(TestValue::Unknown);
    let v2 = table.new_var(TestValue::Unknown);
    let snap = table.snapshot();
    table.union(v1, v2);
    assert_eq!(table.find(v1), table.find(v2));

    table.rollback_to(snap);
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
    table.union(v1, v2);
    table.union(v2, v3);

    let root = table.find(v1);
    assert_eq!(table.find(v2), root);
    assert_eq!(table.find(v3), root);
}

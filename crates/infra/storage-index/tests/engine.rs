//! Integration tests for `storage-index`.

use bytes::Bytes;
use storage_index::{IndexEngine, Record};
use storage_memory::MemoryEngine;
use storage_traits::{Engine, IndexedEngine, Transaction};

fn make_record(name: &str, age: &str) -> Bytes {
    Bytes::from(
        Record::new()
            .with_column("name", name)
            .with_column("age", age)
            .encode(),
    )
}

#[test]
fn put_get_and_scan() {
    let engine = IndexEngine::open(MemoryEngine::new()).unwrap();

    let mut txn = engine.begin(Default::default()).unwrap();
    txn.put(b"pk1", &make_record("alice", "30")).unwrap();
    txn.put(b"pk2", &make_record("bob", "25")).unwrap();
    txn.commit().unwrap();

    let got = engine.get(b"pk1").unwrap().unwrap();
    assert_eq!(Record::decode(&got), Record::decode(&make_record("alice", "30")));

    let mut cursor = engine.scan(None, None).unwrap();
    let entries: Vec<_> = std::iter::from_fn(|| cursor.next())
        .filter_map(Result::ok)
        .collect();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].0.as_ref(), b"pk1");
    assert_eq!(entries[1].0.as_ref(), b"pk2");
}

#[test]
fn secondary_index_scans_by_column() {
    let engine = IndexEngine::open(MemoryEngine::new()).unwrap();

    let mut txn = engine.begin(Default::default()).unwrap();
    txn.put(b"pk1", &make_record("alice", "30")).unwrap();
    txn.put(b"pk2", &make_record("bob", "25")).unwrap();
    txn.put(b"pk3", &make_record("alice", "40")).unwrap();
    txn.commit().unwrap();

    let name_idx = engine.create_index("name", &["name"]).unwrap();

    let mut cursor = engine.index_scan(name_idx, Some(b"alice"), Some(b"bob")).unwrap();
    let mut pks: Vec<_> = std::iter::from_fn(|| cursor.next())
        .filter_map(Result::ok)
        .map(|(_, pk)| pk.to_vec())
        .collect();
    pks.sort();
    assert_eq!(pks, vec![b"pk1".to_vec(), b"pk3".to_vec()]);
}

#[test]
fn index_updates_on_value_change() {
    let engine = IndexEngine::open(MemoryEngine::new()).unwrap();

    let name_idx = engine.create_index("name", &["name"]).unwrap();

    {
        let mut txn = engine.begin(Default::default()).unwrap();
        txn.put(b"pk1", &make_record("alice", "30")).unwrap();
        txn.commit().unwrap();
    }

    {
        let mut txn = engine.begin(Default::default()).unwrap();
        txn.put(b"pk1", &make_record("carol", "30")).unwrap();
        txn.commit().unwrap();
    }

    let mut cursor = engine.index_scan(name_idx, Some(b"alice"), Some(b"bob")).unwrap();
    let pks: Vec<_> = std::iter::from_fn(|| cursor.next())
        .filter_map(Result::ok)
        .map(|(_, pk)| pk.to_vec())
        .collect();
    assert!(pks.is_empty(), "old index entry should be removed");

    let mut cursor = engine.index_scan(name_idx, Some(b"carol"), Some(b"carol\x00")).unwrap();
    let pks: Vec<_> = std::iter::from_fn(|| cursor.next())
        .filter_map(Result::ok)
        .map(|(_, pk)| pk.to_vec())
        .collect();
    assert_eq!(pks, vec![b"pk1".to_vec()]);
}

#[test]
fn drop_index_removes_entries() {
    let engine = IndexEngine::open(MemoryEngine::new()).unwrap();

    let name_idx = engine.create_index("name", &["name"]).unwrap();

    {
        let mut txn = engine.begin(Default::default()).unwrap();
        txn.put(b"pk1", &make_record("alice", "30")).unwrap();
        txn.commit().unwrap();
    }

    engine.drop_index(name_idx).unwrap();

    // Primary data is still there.
    assert!(engine.get(b"pk1").unwrap().is_some());

    // Scanning the dropped index returns nothing.
    let mut cursor = engine.index_scan(name_idx, None, None).unwrap();
    assert!(std::iter::from_fn(|| cursor.next()).next().is_none());
}

#[test]
fn opaque_value_is_not_indexed() {
    let engine = IndexEngine::open(MemoryEngine::new()).unwrap();

    let name_idx = engine.create_index("name", &["name"]).unwrap();

    {
        let mut txn = engine.begin(Default::default()).unwrap();
        txn.put(b"pk1", b"raw opaque bytes").unwrap();
        txn.commit().unwrap();
    }

    let mut cursor = engine.index_scan(name_idx, None, None).unwrap();
    assert!(std::iter::from_fn(|| cursor.next()).next().is_none());
}

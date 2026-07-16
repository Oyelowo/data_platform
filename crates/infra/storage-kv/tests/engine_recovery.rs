//! Crash-recovery and property-based tests for storage-kv.

use std::collections::BTreeMap;

use proptest::collection::vec;
use proptest::prelude::*;
use storage_kv::{LsmEngine, LsmOptions};
use storage_traits::Engine;

#[derive(Clone, Debug)]
enum Op {
    Put { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        (any::<Vec<u8>>(), any::<Vec<u8>>()).prop_map(|(key, value)| Op::Put { key, value }),
        any::<Vec<u8>>().prop_map(|key| Op::Delete { key }),
    ]
}

fn fresh_opts() -> LsmOptions {
    LsmOptions {
        write_buffer_size: 256,
        ..Default::default()
    }
}

/// Crash-recovery property: after reopening, all synced writes are visible.
#[test]
fn reopen_keeps_synced_writes() {
    let dir = tempfile::tempdir().unwrap();
    let opts = fresh_opts();
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();

    for i in 0..100u16 {
        engine.put(&i.to_be_bytes(), &i.to_le_bytes()).unwrap();
    }
    engine.sync().unwrap();
    drop(engine);

    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
    for i in 0..100u16 {
        let expected = bytes::Bytes::copy_from_slice(&i.to_le_bytes());
        assert_eq!(engine.get(&i.to_be_bytes()).unwrap(), Some(expected));
    }
}

// Random operations are recovered after reopen when synced.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn random_ops_recover_after_reopen(ops in vec(op_strategy(), 0..20)) {
        let dir = tempfile::tempdir().unwrap();
        let opts = fresh_opts();
        let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
        let mut model: BTreeMap<Vec<u8>, Option<Vec<u8>>> = BTreeMap::new();

        for op in &ops {
            match op {
                Op::Put { key, value } => {
                    engine.put(key, value).unwrap();
                    model.insert(key.clone(), Some(value.clone()));
                }
                Op::Delete { key } => {
                    engine.delete(key).unwrap();
                    model.insert(key.clone(), None);
                }
            }
        }
        engine.sync().unwrap();
        drop(engine);

        let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
        for (key, expected) in &model {
            let got = engine.get(key).unwrap();
            let want = expected.as_ref().map(|v| bytes::Bytes::copy_from_slice(v));
            prop_assert_eq!(got, want, "key {:?}", key);
        }

        // Scan the full range and compare with the live model entries.
        let mut live: BTreeMap<Vec<u8>, bytes::Bytes> = BTreeMap::new();
        let cursor = engine.scan(None, None).unwrap();
        for item in cursor {
            let (k, v) = item.unwrap();
            live.insert(k.to_vec(), v);
        }

        let expected_live: BTreeMap<Vec<u8>, bytes::Bytes> = model
            .into_iter()
            .filter_map(|(k, v)| v.map(|v| (k, bytes::Bytes::from(v))))
            .collect();
        prop_assert_eq!(live, expected_live);
    }
}

/// Compaction must not lose data: overwrite keys many times, then reopen.
#[test]
fn compaction_preserves_latest_values() {
    let dir = tempfile::tempdir().unwrap();
    let opts = LsmOptions {
        write_buffer_size: 64,
        level0_file_num_compaction_trigger: 2,
        target_file_size_base: 256,
        ..Default::default()
    };
    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();

    for round in 0..10u8 {
        for i in 0..50u8 {
            let value = vec![round, i];
            engine.put(&[i], &value).unwrap();
        }
    }
    engine.sync().unwrap();
    drop(engine);

    let engine = LsmEngine::open(dir.path(), opts.clone()).unwrap();
    for i in 0..50u8 {
        let expected = bytes::Bytes::from(vec![9, i]);
        assert_eq!(engine.get(&[i]).unwrap(), Some(expected));
    }
}

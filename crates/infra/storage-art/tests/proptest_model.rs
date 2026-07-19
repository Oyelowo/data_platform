//! Property-based test comparing `ArtMap` behavior against `BTreeMap`.

use std::collections::BTreeMap;

use proptest::prelude::*;
use storage_art::{ArtMap, ArtMapOptions};

#[derive(Clone, Debug)]
enum Op {
    Insert(Vec<u8>, Vec<u8>),
    Remove(Vec<u8>),
    Get(Vec<u8>),
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        (any::<Vec<u8>>(), any::<Vec<u8>>()).prop_map(|(k, v)| Op::Insert(k, v)),
        any::<Vec<u8>>().prop_map(Op::Remove),
        any::<Vec<u8>>().prop_map(Op::Get),
    ]
}

proptest! {
    #[test]
    fn matches_btree_model(ops in prop::collection::vec(op_strategy(), 1..200)) {
        let map = ArtMap::new(ArtMapOptions::default());
        let mut model = BTreeMap::<Vec<u8>, Vec<u8>>::new();

        for op in ops {
            match op {
                Op::Insert(k, v) => {
                    let prev = map.insert(&k, &v).unwrap();
                    let model_prev = model.insert(k.clone(), v.clone());
                    assert_eq!(prev.as_ref().map(|b| b.to_vec()), model_prev);
                }
                Op::Remove(k) => {
                    let prev = map.remove(&k).unwrap();
                    let model_prev = model.remove(&k);
                    assert_eq!(prev.as_ref().map(|b| b.to_vec()), model_prev);
                }
                Op::Get(k) => {
                    let got = map.get(&k);
                    let model_got = model.get(&k);
                    assert_eq!(got.as_ref().map(|b| b.to_vec()), model_got.cloned());
                }
            }
        }

        // Final range scan must match model.
        let mut cursor = map.range(None, None);
        for (expected_key, expected_value) in &model {
            let (k, v) = cursor.next().unwrap().unwrap();
            assert_eq!(&k[..], expected_key.as_slice());
            assert_eq!(&v[..], expected_value.as_slice());
        }
        assert!(cursor.next().is_none());
    }
}

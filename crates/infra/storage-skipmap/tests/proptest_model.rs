//! Property-based tests comparing SkipMap against a BTreeMap oracle.

use std::collections::BTreeMap;

use proptest::collection::vec;
use proptest::prelude::*;
use storage_skipmap::SkipMap;

#[derive(Clone, Debug)]
enum Op {
    Insert { key: Vec<u8>, value: Vec<u8> },
    Remove { key: Vec<u8> },
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        (any::<Vec<u8>>(), any::<Vec<u8>>()).prop_map(|(key, value)| Op::Insert { key, value }),
        any::<Vec<u8>>().prop_map(|key| Op::Remove { key }),
    ]
}

proptest! {
    #[test]
    fn sequential_ops_match_btree(ops in vec(op_strategy(), 0..200)) {
        let map = SkipMap::new();
        let mut model: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();

        for op in &ops {
            match op {
                Op::Insert { key, value } => {
                    let old = map.insert(key.clone(), value.clone());
                    let expected = model.insert(key.clone(), value.clone());
                    prop_assert_eq!(old, expected);
                }
                Op::Remove { key } => {
                    let old = map.remove(key);
                    let expected = model.remove(key);
                    prop_assert_eq!(old, expected);
                }
            }
        }

        // Final state must match for every key that was ever touched.
        for op in &ops {
            let key = match op {
                Op::Insert { key, .. } | Op::Remove { key } => key,
            };
            prop_assert_eq!(map.get(key), model.get(key).cloned());
        }

        prop_assert_eq!(
            map.iter().collect::<Vec<_>>(),
            model.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<Vec<_>>()
        );
    }
}

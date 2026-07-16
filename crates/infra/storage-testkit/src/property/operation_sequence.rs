//! Property-based tests using random operation sequences.

use proptest::collection::vec;
use proptest::prelude::*;
use storage_traits::{Engine, Transaction, TxnOptions};

use crate::model::{Model, Op};

/// Strategy for generating operations.
fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        (any::<Vec<u8>>(), any::<Vec<u8>>()).prop_map(|(k, v)| Op::Put {
            key: k.into(),
            value: v.into(),
        }),
        any::<Vec<u8>>().prop_map(|k| Op::Delete { key: k.into() }),
    ]
}

/// Verify that a sequence of direct operations matches the model oracle.
pub fn direct_ops_match_model<E, F>(factory: F)
where
    E: Engine,
    F: Fn() -> E,
{
    proptest!(|(ops in vec(op_strategy(), 0..200))| {
        let engine = factory();
        let mut model = Model::new();

        for op in &ops {
            match op {
                Op::Put { key, value } => {
                    let mut tx = engine.begin(TxnOptions::default())?;
                    tx.put(key, value)?;
                    tx.commit()?;
                    model.put(key.clone(), value.clone());
                }
                Op::Delete { key } => {
                    let mut tx = engine.begin(TxnOptions::default())?;
                    tx.delete(key)?;
                    tx.commit()?;
                    model.delete(key);
                }
            }
        }

        // Verify every possible key.
        for op in &ops {
            let key = match op {
                Op::Put { key, .. } => key,
                Op::Delete { key } => key,
            };
            prop_assert_eq!(engine.get(key).unwrap(), model.get(key));
        }
    });
}

/// Verify that scans always return sorted results.
pub fn scans_are_sorted<E, F>(factory: F)
where
    E: Engine,
    F: Fn() -> E,
{
    proptest!(|(ops in vec(op_strategy(), 0..100))| {
        let engine = factory();
        for op in &ops {
            match op {
                Op::Put { key, value } => {
                    let mut tx = engine.begin(TxnOptions::default())?;
                    tx.put(key, value)?;
                    tx.commit()?;
                }
                Op::Delete { key } => {
                    let mut tx = engine.begin(TxnOptions::default())?;
                    tx.delete(key)?;
                    tx.commit()?;
                }
            }
        }

        let cursor = engine.scan(None, None)?;
        let mut last: Option<Vec<u8>> = None;
        for item in cursor {
            let (k, _) = item?;
            if let Some(ref last) = last {
                prop_assert!(k.as_ref() > last.as_slice(), "scan not sorted");
            }
            last = Some(k.to_vec());
        }
    });
}

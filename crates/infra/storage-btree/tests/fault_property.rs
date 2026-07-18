//! Property-based fault-injection tests.
//!
//! Each case generates a deterministic seed, builds a [`FaultSchedule`] from it,
//! runs a random workload, simulates a crash, reopens with [`RealBackend`], and
//! compares the visible state to a `BTreeMap` oracle.

use std::collections::BTreeMap;
use std::io::ErrorKind;

use bytes::Bytes;
use proptest::prelude::*;
use storage_btree::{BtreeEngine, BtreeOptions, FaultRule, FaultSchedule, OpFamily};
use storage_traits::{Engine, Transaction, TxnOptions};

#[derive(Clone, Debug)]
enum Op {
    Put(Vec<u8>, Vec<u8>),
    Delete(Vec<u8>),
}

fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (any::<[u8; 4]>(), any::<[u8; 8]>()).prop_map(|(k, v)| Op::Put(k.to_vec(), v.to_vec())),
        any::<[u8; 4]>().prop_map(|k| Op::Delete(k.to_vec())),
    ]
}

fn schedule_from_seed(seed: u64) -> FaultSchedule {
    let mut rules = Vec::new();
    if seed.is_multiple_of(5) {
        rules.push(FaultRule::FailNth {
            op: OpFamily::PageWrite,
            n: ((seed % 4) + 1) as usize,
            error: ErrorKind::PermissionDenied,
        });
    }
    if seed.is_multiple_of(7) {
        rules.push(FaultRule::FailNth {
            op: OpFamily::PageSync,
            n: ((seed % 3) + 1) as usize,
            error: ErrorKind::Other,
        });
    }
    if seed.is_multiple_of(6) {
        rules.push(FaultRule::FailNth {
            op: OpFamily::ValueLogAppend,
            n: ((seed % 4) + 1) as usize,
            error: ErrorKind::PermissionDenied,
        });
    }
    if seed.is_multiple_of(8) {
        rules.push(FaultRule::FailNth {
            op: OpFamily::ValueLogSync,
            n: ((seed % 3) + 1) as usize,
            error: ErrorKind::PermissionDenied,
        });
    }
    if seed.is_multiple_of(11) {
        rules.push(FaultRule::PartialWriteNth {
            op: OpFamily::PageWrite,
            n: ((seed % 3) + 1) as usize,
            truncate_to: 64,
        });
    }
    FaultSchedule { seed, rules }
}

fn run_workload(seed: u64, ops: &[Op]) -> BTreeMap<Vec<u8>, Vec<u8>> {
    let dir = tempfile::tempdir().unwrap();
    let schedule = schedule_from_seed(seed);
    let options = BtreeOptions {
        fault_schedule: Some(schedule),
        ..Default::default()
    };
    let engine = BtreeEngine::open(dir.path(), options).unwrap();
    let mut oracle = BTreeMap::new();
    let mut touched = Vec::new();

    for op in ops {
        let mut txn = engine.begin(TxnOptions::default()).unwrap();
        let committed = match op {
            Op::Put(key, value) => {
                touched.push(key.clone());
                if txn.put(key, value).is_ok() && txn.commit().is_ok() {
                    oracle.insert(key.clone(), value.clone());
                    true
                } else {
                    false
                }
            }
            Op::Delete(key) => {
                touched.push(key.clone());
                if txn.delete(key).is_ok() && txn.commit().is_ok() {
                    oracle.remove(key);
                    true
                } else {
                    false
                }
            }
        };
        // If a commit failed, any buffered value-log or page writes from this
        // transaction are not durable; the oracle correctly excludes them.
        let _ = committed;
    }

    engine.crash();
    std::mem::forget(engine);

    let engine2 = BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap();
    engine2.check_integrity().unwrap();

    for key in &touched {
        let expected = oracle.get(key).cloned();
        let actual = engine2.get(key).unwrap();
        assert_eq!(
            actual,
            expected.map(Bytes::from),
            "seed {seed}: key {:?} does not match oracle",
            key
        );
    }

    oracle
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn random_fault_schedules_match_oracle(seed in 0u64..32, ops in prop::collection::vec(arb_op(), 1..24)) {
        run_workload(seed, &ops);
    }
}

#[test]
fn explicit_seed_reproduces_failure() {
    // A deterministic smoke test that exercises the same harness as the
    // proptest with a known seed.
    let ops = vec![
        Op::Put(vec![0, 0, 0, 0], vec![1; 8]),
        Op::Put(vec![0, 0, 0, 1], vec![2; 8]),
        Op::Delete(vec![0, 0, 0, 0]),
        Op::Put(vec![0, 0, 0, 2], vec![3; 8]),
    ];
    run_workload(42, &ops);
}

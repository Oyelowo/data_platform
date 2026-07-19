//! Model-based state-machine test for the LSM engine.
//!
//! A reference model (a `BTreeMap`) and the real `LsmEngine` are driven through
//! the same sequence of operations.  After every transition the engine is
//! checked against the model for `get`, `scan`, and ordering invariants.

use std::collections::BTreeMap;

use proptest::prelude::*;
use proptest_state_machine::{ReferenceStateMachine, StateMachineTest, prop_state_machine};
use storage_kv::{LsmEngine, LsmOptions};
use storage_traits::Engine;
use tempfile::TempDir;

fn opts() -> LsmOptions {
    LsmOptions {
        write_buffer_size: 256,
        ..Default::default()
    }
}

/// The abstract reference state: the visible key/value map.
///
/// * `Some(value)`  -> key exists with value.
/// * `None`         -> key was explicitly deleted.
/// * absent         -> key has never been written.
type RefState = BTreeMap<Vec<u8>, Option<Vec<u8>>>;

#[derive(Debug, Clone)]
enum Transition {
    Put(Vec<u8>, Vec<u8>),
    Delete(Vec<u8>),
    Sync,
    Reopen,
    Get(Vec<u8>),
    Scan(Option<Vec<u8>>, Option<Vec<u8>>),
}

struct EngineModel;

impl ReferenceStateMachine for EngineModel {
    type State = RefState;
    type Transition = Transition;

    fn init_state() -> BoxedStrategy<Self::State> {
        Just(BTreeMap::new()).boxed()
    }

    fn transitions(_state: &Self::State) -> BoxedStrategy<Self::Transition> {
        let key = prop::collection::vec(any::<u8>(), 0..4);
        let value = prop::collection::vec(any::<u8>(), 0..8);

        prop_oneof![
            (key.clone(), value.clone()).prop_map(|(k, v)| Transition::Put(k, v)),
            key.clone().prop_map(Transition::Delete),
            Just(Transition::Sync),
            Just(Transition::Reopen),
            key.clone().prop_map(Transition::Get),
            (key.clone().prop_map(Some), key.prop_map(Some))
                .prop_map(|(start, end)| Transition::Scan(start, end)),
        ]
        .boxed()
    }

    fn apply(mut state: Self::State, transition: &Self::Transition) -> Self::State {
        match transition {
            Transition::Put(k, v) => {
                state.insert(k.clone(), Some(v.clone()));
            }
            Transition::Delete(k) => {
                state.insert(k.clone(), None);
            }
            Transition::Sync | Transition::Reopen | Transition::Get(_) | Transition::Scan(_, _) => {
                // No visible state change in the reference model.
            }
        }
        state
    }
}

struct EngineSut {
    dir: TempDir,
    engine: LsmEngine,
}

fn ref_value_to_bytes(v: &Option<Vec<u8>>) -> Option<bytes::Bytes> {
    v.as_ref().map(|b| bytes::Bytes::copy_from_slice(b))
}

impl StateMachineTest for EngineModel {
    type SystemUnderTest = EngineSut;
    type Reference = EngineModel;

    fn init_test(_ref_state: &RefState) -> Self::SystemUnderTest {
        let dir = TempDir::new().unwrap();
        let engine = LsmEngine::open(dir.path(), opts()).unwrap();
        EngineSut { dir, engine }
    }

    fn apply(
        mut state: Self::SystemUnderTest,
        ref_state: &RefState,
        transition: Transition,
    ) -> Self::SystemUnderTest {
        match transition {
            Transition::Put(k, v) => {
                state.engine.put(&k, &v).unwrap();
                assert_eq!(
                    state.engine.get(&k).unwrap(),
                    Some(bytes::Bytes::copy_from_slice(&v))
                );
            }
            Transition::Delete(k) => {
                state.engine.delete(&k).unwrap();
                assert_eq!(state.engine.get(&k).unwrap(), None);
            }
            Transition::Sync => {
                state.engine.sync().unwrap();
            }
            Transition::Reopen => {
                // The old engine must be dropped first so the WAL advisory lock
                // is released before the new open attempts to acquire it.
                drop(state.engine);
                state.engine = LsmEngine::open(state.dir.path(), opts()).unwrap();
            }
            Transition::Get(k) => {
                let expected = ref_state.get(&k).and_then(ref_value_to_bytes);
                assert_eq!(state.engine.get(&k).unwrap(), expected);
            }
            Transition::Scan(start, end) => {
                let mut cursor = state.engine.scan(start.as_deref(), end.as_deref()).unwrap();

                let expected: Vec<_> = ref_state
                    .iter()
                    .filter(|(k, v)| {
                        v.is_some()
                            && start
                                .as_ref()
                                .map(|s| k.as_slice() >= s.as_slice())
                                .unwrap_or(true)
                            && end
                                .as_ref()
                                .map(|e| k.as_slice() < e.as_slice())
                                .unwrap_or(true)
                    })
                    .map(|(k, v)| {
                        (
                            bytes::Bytes::copy_from_slice(k),
                            bytes::Bytes::copy_from_slice(v.as_ref().unwrap()),
                        )
                    })
                    .collect();

                let mut got = Vec::new();
                while let Some(Ok((k, v))) = cursor.next() {
                    got.push((k, v));
                }
                assert_eq!(got, expected, "scan mismatch");
            }
        }
        state
    }

    fn check_invariants(state: &Self::SystemUnderTest, ref_state: &RefState) {
        for (k, v) in ref_state {
            let expected = ref_value_to_bytes(v);
            assert_eq!(
                state.engine.get(k).unwrap(),
                expected,
                "invariant failed for key {:?}",
                k
            );
        }

        let mut cursor = state.engine.scan(None, None).unwrap();
        let mut last: Option<Vec<u8>> = None;
        while let Some(Ok((k, _v))) = cursor.next() {
            if let Some(ref l) = last {
                assert!(l.as_slice() < k.as_ref(), "scan must be strictly ascending");
            }
            last = Some(k.to_vec());
        }
    }
}

prop_state_machine! {
    #[test]
    fn engine_state_machine(sequential 1..30 => EngineModel);
}

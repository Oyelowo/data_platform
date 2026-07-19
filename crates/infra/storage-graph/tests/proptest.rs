//! Property-based integration tests against an in-memory oracle.

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;
use storage_graph::{Direction, GraphEngine, GraphOptions, PropertyMap};

fn opts() -> GraphOptions {
    GraphOptions {
        max_unsynced_records: 10_000,
        ..GraphOptions::default()
    }
}

#[derive(Debug, Clone)]
enum Op {
    CreateNode(Vec<u8>, Vec<String>),
    CreateEdge(Vec<u8>, Vec<u8>, Vec<u8>, String),
    DeleteNode(Vec<u8>),
    DeleteEdge(Vec<u8>),
}

fn vec_u8() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 1..16)
}

fn label() -> impl Strategy<Value = String> {
    prop::collection::vec(any::<char>(), 1..8).prop_map(|v| v.into_iter().collect())
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (vec_u8(), prop::collection::vec(label(), 0..3))
            .prop_map(|(id, labels)| Op::CreateNode(id, labels)),
        (vec_u8(), vec_u8(), vec_u8(), label())
            .prop_map(|(id, from, to, label)| Op::CreateEdge(id, from, to, label)),
        vec_u8().prop_map(Op::DeleteNode),
        vec_u8().prop_map(Op::DeleteEdge),
    ]
}

proptest! {
    #[test]
    fn random_ops_match_oracle(ops in prop::collection::vec(op(), 0..50)) {
        let dir = tempfile::tempdir().unwrap();
        let engine = GraphEngine::open(dir.path(), opts()).unwrap();

        // Oracle tracks live nodes and edges by external id.
        let mut oracle_nodes: BTreeSet<Vec<u8>> = BTreeSet::new();
        let mut oracle_edges: BTreeMap<Vec<u8>, (Vec<u8>, Vec<u8>)> = BTreeMap::new();

        for op in ops {
            match op {
                Op::CreateNode(id, labels) => {
                    let labels: Vec<String> = labels.into_iter().collect();
                    let _ = engine.create_node(id.clone(), labels, PropertyMap::new());
                    oracle_nodes.insert(id);
                }
                Op::CreateEdge(id, from, to, label) => {
                    if oracle_nodes.contains(&from) && oracle_nodes.contains(&to) {
                        let _ = engine.create_edge(id.clone(), from.clone(), to.clone(), label, PropertyMap::new());
                        oracle_edges.insert(id, (from, to));
                    } else {
                        // Engine should reject edges with missing endpoints.
                        assert!(engine.create_edge(id.clone(), from.clone(), to.clone(), label, PropertyMap::new()).is_err());
                    }
                }
                Op::DeleteNode(id) => {
                    let _ = engine.delete_node(&id);
                    oracle_nodes.remove(&id);
                    oracle_edges.retain(|_, (from, to)| from != &id && to != &id);
                }
                Op::DeleteEdge(id) => {
                    let _ = engine.delete_edge(&id);
                    oracle_edges.remove(&id);
                }
            }
        }

        for id in &oracle_nodes {
            prop_assert!(engine.get_node(id).unwrap().is_some(), "node {:?} missing", id);
        }
        for (id, (from, to)) in &oracle_edges {
            let edge = engine.get_edge(id).unwrap().expect("edge missing");
            prop_assert_eq!(&edge.from, from);
            prop_assert_eq!(&edge.to, to);
        }

        // Check that edges incident to deleted nodes are gone.
        for id in &oracle_nodes {
            let out = engine.edges(id, Direction::Out).unwrap();
            for edge in out {
                prop_assert!(oracle_edges.contains_key(&edge.id));
                prop_assert_eq!(&edge.from, id);
            }
            let inc = engine.edges(id, Direction::In).unwrap();
            for edge in inc {
                prop_assert!(oracle_edges.contains_key(&edge.id));
                prop_assert_eq!(&edge.to, id);
            }
        }
    }
}

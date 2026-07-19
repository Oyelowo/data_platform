//! Adjacency index integration tests.

use storage_graph::{Direction, GraphEngine, GraphOptions, PropertyMap};

fn opts() -> GraphOptions {
    GraphOptions {
        max_unsynced_records: 1,
        ..GraphOptions::default()
    }
}

fn make_triangle(engine: &GraphEngine) {
    engine
        .create_node(b"a", ["N"], PropertyMap::new())
        .unwrap();
    engine
        .create_node(b"b", ["N"], PropertyMap::new())
        .unwrap();
    engine
        .create_node(b"c", ["N"], PropertyMap::new())
        .unwrap();
    engine.create_edge(b"ab", b"a", b"b", "E", PropertyMap::new()).unwrap();
    engine.create_edge(b"bc", b"b", b"c", "E", PropertyMap::new()).unwrap();
    engine.create_edge(b"ca", b"c", b"a", "E", PropertyMap::new()).unwrap();
}

#[test]
fn outgoing_neighbors() {
    let dir = tempfile::tempdir().unwrap();
    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    make_triangle(&engine);
    let neighbors = engine.neighbors(b"a", Direction::Out, None).unwrap();
    let ids: Vec<_> = neighbors.iter().map(|n| n.id.clone()).collect();
    assert_eq!(ids, vec![b"b".to_vec()]);
}

#[test]
fn incoming_neighbors() {
    let dir = tempfile::tempdir().unwrap();
    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    make_triangle(&engine);
    let neighbors = engine.neighbors(b"a", Direction::In, None).unwrap();
    let ids: Vec<_> = neighbors.iter().map(|n| n.id.clone()).collect();
    assert_eq!(ids, vec![b"c".to_vec()]);
}

#[test]
fn both_directions() {
    let dir = tempfile::tempdir().unwrap();
    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    make_triangle(&engine);
    let neighbors = engine.neighbors(b"a", Direction::Both, None).unwrap();
    let mut ids: Vec<_> = neighbors.iter().map(|n| n.id.clone()).collect();
    ids.sort();
    assert_eq!(ids, vec![b"b".to_vec(), b"c".to_vec()]);
}

#[test]
fn edges_by_direction() {
    let dir = tempfile::tempdir().unwrap();
    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    make_triangle(&engine);
    let out_edges = engine.edges(b"a", Direction::Out).unwrap();
    assert_eq!(out_edges.len(), 1);
    assert_eq!(out_edges[0].id, b"ab");

    let in_edges = engine.edges(b"a", Direction::In).unwrap();
    assert_eq!(in_edges.len(), 1);
    assert_eq!(in_edges[0].id, b"ca");
}

#[test]
fn self_loop() {
    let dir = tempfile::tempdir().unwrap();
    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    engine
        .create_node(b"a", ["N"], PropertyMap::new())
        .unwrap();
    engine.create_edge(b"aa", b"a", b"a", "E", PropertyMap::new()).unwrap();
    let both = engine.neighbors(b"a", Direction::Both, None).unwrap();
    assert_eq!(both.len(), 2);
    let out = engine.edges(b"a", Direction::Out).unwrap();
    assert_eq!(out.len(), 1);
}

#[test]
fn parallel_edges() {
    let dir = tempfile::tempdir().unwrap();
    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    engine
        .create_node(b"a", ["N"], PropertyMap::new())
        .unwrap();
    engine
        .create_node(b"b", ["N"], PropertyMap::new())
        .unwrap();
    engine.create_edge(b"e1", b"a", b"b", "E", PropertyMap::new()).unwrap();
    engine.create_edge(b"e2", b"a", b"b", "E", PropertyMap::new()).unwrap();
    let edges = engine.edges(b"a", Direction::Out).unwrap();
    assert_eq!(edges.len(), 2);
}

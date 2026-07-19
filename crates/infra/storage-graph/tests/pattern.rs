//! Pattern matching integration tests.

use storage_graph::{
    Direction, GraphEngine, GraphOptions, GraphQuery, PatternStep, PropertyMap,
};

fn opts() -> GraphOptions {
    GraphOptions {
        max_unsynced_records: 1,
        ..GraphOptions::default()
    }
}

#[test]
fn simple_chain_pattern() {
    let dir = tempfile::tempdir().unwrap();
    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    engine
        .create_node(b"alice", ["User"], PropertyMap::new())
        .unwrap();
    engine
        .create_node(b"bob", ["User"], PropertyMap::new())
        .unwrap();
    engine
        .create_node(b"post", ["Post"], PropertyMap::new())
        .unwrap();
    engine
        .create_edge(b"ab", b"alice", b"bob", "FOLLOWS", PropertyMap::new())
        .unwrap();
    engine
        .create_edge(b"bp", b"bob", b"post", "WROTE", PropertyMap::new())
        .unwrap();

    let steps = vec![
        PatternStep::new(["User"], Direction::Out, Some("FOLLOWS"), ["User"]),
        PatternStep::new(["User"], Direction::Out, Some("WROTE"), ["Post"]),
    ];
    let result = engine.query(GraphQuery::Pattern(steps)).unwrap();
    assert_eq!(result.paths.len(), 1);
    assert_eq!(
        result.paths[0],
        vec![b"alice".to_vec(), b"bob".to_vec(), b"post".to_vec()]
    );
}

#[test]
fn pattern_with_no_matches() {
    let dir = tempfile::tempdir().unwrap();
    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    engine
        .create_node(b"a", ["A"], PropertyMap::new())
        .unwrap();
    engine
        .create_node(b"b", ["B"], PropertyMap::new())
        .unwrap();
    engine.create_edge(b"ab", b"a", b"b", "E", PropertyMap::new()).unwrap();

    let steps = vec![PatternStep::new(["B"], Direction::Out, Some("E"), ["A"])];
    let result = engine.query(GraphQuery::Pattern(steps)).unwrap();
    assert!(result.paths.is_empty());
}

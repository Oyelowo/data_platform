//! Traversal integration tests.

use storage_graph::{Direction, GraphEngine, GraphOptions, GraphQuery, PropertyMap};

fn opts() -> GraphOptions {
    GraphOptions {
        max_unsynced_records: 1,
        ..GraphOptions::default()
    }
}

fn make_line(engine: &GraphEngine) {
    for id in [b"a", b"b", b"c", b"d"] {
        engine
            .create_node(id, ["N"], PropertyMap::new())
            .unwrap();
    }
    engine.create_edge(b"ab", b"a", b"b", "E", PropertyMap::new()).unwrap();
    engine.create_edge(b"bc", b"b", b"c", "E", PropertyMap::new()).unwrap();
    engine.create_edge(b"cd", b"c", b"d", "E", PropertyMap::new()).unwrap();
}

#[test]
fn bfs_reaches_end() {
    let dir = tempfile::tempdir().unwrap();
    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    make_line(&engine);
    let result = engine
        .query(GraphQuery::Path {
            from: b"a".to_vec(),
            to: b"d".to_vec(),
            max_depth: 10,
        })
        .unwrap();
    assert_eq!(result.paths.len(), 1);
    assert_eq!(result.paths[0].len(), 4);
    assert_eq!(result.paths[0][0], b"a");
    assert_eq!(result.paths[0][3], b"d");
}

#[test]
fn path_respects_max_depth() {
    let dir = tempfile::tempdir().unwrap();
    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    make_line(&engine);
    let result = engine
        .query(GraphQuery::Path {
            from: b"a".to_vec(),
            to: b"d".to_vec(),
            max_depth: 2,
        })
        .unwrap();
    assert!(result.paths.is_empty());
}

#[test]
fn neighbors_with_edge_label_filter() {
    let dir = tempfile::tempdir().unwrap();
    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    engine
        .create_node(b"a", ["N"], PropertyMap::new())
        .unwrap();
    engine
        .create_node(b"b", ["N"], PropertyMap::new())
        .unwrap();
    engine
        .create_node(b"c", ["N"], PropertyMap::new())
        .unwrap();
    engine.create_edge(b"ab", b"a", b"b", "FOLLOWS", PropertyMap::new()).unwrap();
    engine.create_edge(b"ac", b"a", b"c", "BLOCKS", PropertyMap::new()).unwrap();

    let follows = engine
        .neighbors(b"a", Direction::Out, Some("FOLLOWS"))
        .unwrap();
    assert_eq!(follows.len(), 1);
    assert_eq!(follows[0].id, b"b");
}

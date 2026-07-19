//! Label index integration tests.

use storage_graph::{GraphEngine, GraphOptions, GraphQuery, PropertyMap};

fn opts() -> GraphOptions {
    GraphOptions {
        max_unsynced_records: 1,
        ..GraphOptions::default()
    }
}

#[test]
fn nodes_by_label() {
    let dir = tempfile::tempdir().unwrap();
    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    engine
        .create_node(b"a", ["User"], PropertyMap::new())
        .unwrap();
    engine
        .create_node(b"b", ["User"], PropertyMap::new())
        .unwrap();
    engine
        .create_node(b"c", ["Post"], PropertyMap::new())
        .unwrap();

    let result = engine.query(GraphQuery::NodesByLabel("User".into())).unwrap();
    let ids: Vec<_> = result.nodes.iter().map(|n| n.id.clone()).collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&b"a".to_vec()));
    assert!(ids.contains(&b"b".to_vec()));
}

#[test]
fn edges_by_label() {
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
    engine.create_edge(b"bc", b"b", b"c", "WROTE", PropertyMap::new()).unwrap();

    let result = engine
        .query(GraphQuery::EdgesByLabel("FOLLOWS".into()))
        .unwrap();
    assert_eq!(result.edges.len(), 1);
    assert_eq!(result.edges[0].id, b"ab");
}

#[test]
fn add_and_remove_node_label() {
    let dir = tempfile::tempdir().unwrap();
    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    engine
        .create_node(b"a", ["User"], PropertyMap::new())
        .unwrap();
    engine.add_node_label(b"a", "Admin").unwrap();
    let node = engine.get_node(b"a").unwrap().unwrap();
    assert!(node.labels.contains("Admin"));

    let result = engine.query(GraphQuery::NodesByLabel("Admin".into())).unwrap();
    assert_eq!(result.nodes.len(), 1);

    engine.remove_node_label(b"a", "Admin").unwrap();
    let result = engine.query(GraphQuery::NodesByLabel("Admin".into())).unwrap();
    assert!(result.nodes.is_empty());
}

#[test]
fn label_index_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    {
        let engine = GraphEngine::open(dir.path(), opts()).unwrap();
        engine
            .create_node(b"a", ["User"], PropertyMap::new())
            .unwrap();
        engine.sync().unwrap();
        engine.close().unwrap();
    }
    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    let result = engine.query(GraphQuery::NodesByLabel("User".into())).unwrap();
    assert_eq!(result.nodes.len(), 1);
    assert_eq!(result.nodes[0].id, b"a");
}

//! Basic graph engine integration tests.

use storage_graph::{GraphEngine, GraphOptions, PropertyMap};

fn opts() -> GraphOptions {
    GraphOptions {
        max_unsynced_records: 1,
        ..GraphOptions::default()
    }
}

#[test]
fn create_and_get_node() {
    let dir = tempfile::tempdir().unwrap();
    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    engine
        .create_node(b"n1", ["User"], PropertyMap::new())
        .unwrap();
    let node = engine.get_node(b"n1").unwrap().unwrap();
    assert_eq!(node.id, b"n1");
    assert!(node.labels.contains("User"));
}

#[test]
fn create_and_get_edge() {
    let dir = tempfile::tempdir().unwrap();
    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    engine
        .create_node(b"n1", ["User"], PropertyMap::new())
        .unwrap();
    engine
        .create_node(b"n2", ["User"], PropertyMap::new())
        .unwrap();
    engine
        .create_edge(b"e1", b"n1", b"n2", "FOLLOWS", PropertyMap::new())
        .unwrap();
    let edge = engine.get_edge(b"e1").unwrap().unwrap();
    assert_eq!(edge.id, b"e1");
    assert_eq!(edge.from, b"n1");
    assert_eq!(edge.to, b"n2");
    assert_eq!(edge.label, "FOLLOWS");
}

#[test]
fn edge_requires_existing_endpoints() {
    let dir = tempfile::tempdir().unwrap();
    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    let result = engine.create_edge(b"e1", b"n1", b"n2", "FOLLOWS", PropertyMap::new());
    assert!(result.is_err());
}

#[test]
fn delete_node_cascades_to_edges() {
    let dir = tempfile::tempdir().unwrap();
    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    engine
        .create_node(b"n1", ["User"], PropertyMap::new())
        .unwrap();
    engine
        .create_node(b"n2", ["User"], PropertyMap::new())
        .unwrap();
    engine
        .create_edge(b"e1", b"n1", b"n2", "FOLLOWS", PropertyMap::new())
        .unwrap();
    assert!(engine.delete_node(b"n1").unwrap());
    assert!(engine.get_node(b"n1").unwrap().is_none());
    assert!(engine.get_edge(b"e1").unwrap().is_none());
}

#[test]
fn upsert_node_replaces_labels() {
    let dir = tempfile::tempdir().unwrap();
    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    engine
        .create_node(b"n1", ["User"], PropertyMap::new())
        .unwrap();
    engine
        .create_node(b"n1", ["Admin"], PropertyMap::new())
        .unwrap();
    let node = engine.get_node(b"n1").unwrap().unwrap();
    assert!(node.labels.contains("Admin"));
    assert!(!node.labels.contains("User"));
}

#[test]
fn properties_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    let mut props = PropertyMap::new();
    props.insert("name".into(), b"Ada".to_vec());
    engine.create_node(b"n1", ["User"], props).unwrap();
    let node = engine.get_node(b"n1").unwrap().unwrap();
    assert_eq!(node.properties.get("name"), Some(&b"Ada".to_vec()));

    engine
        .set_node_property(b"n1", "age", b"42".to_vec())
        .unwrap();
    let node = engine.get_node(b"n1").unwrap().unwrap();
    assert_eq!(node.properties.get("age"), Some(&b"42".to_vec()));

    engine.delete_node_property(b"n1", "name").unwrap();
    let node = engine.get_node(b"n1").unwrap().unwrap();
    assert!(!node.properties.contains_key("name"));
}

#[test]
fn reopen_engine() {
    let dir = tempfile::tempdir().unwrap();
    {
        let engine = GraphEngine::open(dir.path(), opts()).unwrap();
        engine
            .create_node(b"n1", ["User"], PropertyMap::new())
            .unwrap();
        engine
            .create_edge(b"e1", b"n1", b"n1", "SELF", PropertyMap::new())
            .unwrap();
        engine.sync().unwrap();
        engine.close().unwrap();
    }
    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    let node = engine.get_node(b"n1").unwrap().unwrap();
    assert_eq!(node.id, b"n1");
    let edge = engine.get_edge(b"e1").unwrap().unwrap();
    assert_eq!(edge.label, "SELF");
}

#[test]
fn engine_trait_get_and_scan() {
    use storage_traits::Engine;
    let dir = tempfile::tempdir().unwrap();
    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    let mut props = PropertyMap::new();
    props.insert("name".into(), b"Ada".to_vec());
    engine.create_node(b"n1", ["User"], props).unwrap();

    let value = engine.get(b"node:n1").unwrap().unwrap();
    assert!(!value.is_empty());

    let prop = engine.get(b"prop:node:n1:name").unwrap().unwrap();
    assert_eq!(&prop[..], b"Ada");

    let mut cursor = engine.scan(None, None).unwrap();
    let mut count = 0;
    while cursor.next().is_some() {
        count += 1;
    }
    assert_eq!(count, 2);
}

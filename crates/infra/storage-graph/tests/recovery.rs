//! WAL recovery integration tests.

use storage_graph::{GraphEngine, GraphOptions, PropertyMap};

fn opts() -> GraphOptions {
    GraphOptions {
        max_unsynced_records: 100,
        wal_sync_policy: storage_graph::WalSyncPolicy::SyncOnEngineSync,
        ..GraphOptions::default()
    }
}

#[test]
fn recover_unsynced_writes() {
    let dir = tempfile::tempdir().unwrap();
    {
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
        // Do not sync; simulate crash by dropping engine without close.
    }

    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    let n1 = engine.get_node(b"n1").unwrap().unwrap();
    assert_eq!(n1.id, b"n1");
    let n2 = engine.get_node(b"n2").unwrap().unwrap();
    assert_eq!(n2.id, b"n2");
    let e1 = engine.get_edge(b"e1").unwrap().unwrap();
    assert_eq!(e1.label, "FOLLOWS");
}

#[test]
fn recover_node_deletion() {
    let dir = tempfile::tempdir().unwrap();
    {
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
        engine.delete_node(b"n1").unwrap();
    }

    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    assert!(engine.get_node(b"n1").unwrap().is_none());
    assert!(engine.get_edge(b"e1").unwrap().is_none());
    assert!(engine.get_node(b"n2").unwrap().is_some());
}

#[test]
fn recover_property_and_label_mutations() {
    let dir = tempfile::tempdir().unwrap();
    {
        let engine = GraphEngine::open(dir.path(), opts()).unwrap();
        engine
            .create_node(b"n1", ["User"], PropertyMap::new())
            .unwrap();
        engine
            .set_node_property(b"n1", "name", b"Ada".to_vec())
            .unwrap();
        engine.add_node_label(b"n1", "Admin").unwrap();
    }

    let engine = GraphEngine::open(dir.path(), opts()).unwrap();
    let node = engine.get_node(b"n1").unwrap().unwrap();
    assert_eq!(node.properties.get("name"), Some(&b"Ada".to_vec()));
    assert!(node.labels.contains("Admin"));
    assert!(node.labels.contains("User"));
}

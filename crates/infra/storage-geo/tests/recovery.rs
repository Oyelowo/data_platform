//! Crash-recovery integration tests.

use geo::Point;
use storage_geo::{GeoEngine, GeoOptions, Geometry, PropertyMap, SpatialQuery};

#[test]
fn recover_unsynced_inserts() {
    let dir = tempfile::tempdir().unwrap();
    let engine = GeoEngine::open(dir.path(), GeoOptions::default()).unwrap();
    engine
        .insert_feature(b"a", Geometry::Point(Point::new(0.0, 0.0)), PropertyMap::new())
        .unwrap();
    engine
        .insert_feature(b"b", Geometry::Point(Point::new(1.0, 1.0)), PropertyMap::new())
        .unwrap();
    // Do not sync; simulate crash by dropping the engine handle.
    drop(engine);

    let engine = GeoEngine::open(dir.path(), GeoOptions::default()).unwrap();
    assert!(engine.get_feature(b"a").unwrap().is_some());
    assert!(engine.get_feature(b"b").unwrap().is_some());

    let results = engine
        .query(&SpatialQuery::BBox {
            min_lon: -1.0,
            min_lat: -1.0,
            max_lon: 2.0,
            max_lat: 2.0,
        })
        .unwrap();
    assert_eq!(results.len(), 2);
}

#[test]
fn recover_unsynced_delete_and_update() {
    let dir = tempfile::tempdir().unwrap();
    let engine = GeoEngine::open(dir.path(), GeoOptions::default()).unwrap();
    engine
        .insert_feature(b"a", Geometry::Point(Point::new(0.0, 0.0)), PropertyMap::new())
        .unwrap();
    engine
        .insert_feature(b"b", Geometry::Point(Point::new(1.0, 1.0)), PropertyMap::new())
        .unwrap();
    engine.delete_feature(b"a").unwrap();
    engine
        .update_property(b"b", "name", b"updated".to_vec())
        .unwrap();
    drop(engine);

    let engine = GeoEngine::open(dir.path(), GeoOptions::default()).unwrap();
    assert!(engine.get_feature(b"a").unwrap().is_none());
    let b = engine.get_feature(b"b").unwrap().unwrap();
    assert_eq!(b.properties["name"], b"updated"[..]);
}

#[test]
fn sync_then_reopen_no_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let engine = GeoEngine::open(dir.path(), GeoOptions::default()).unwrap();
    engine
        .insert_feature(b"a", Geometry::Point(Point::new(0.0, 0.0)), PropertyMap::new())
        .unwrap();
    engine.sync().unwrap();
    drop(engine);

    let engine = GeoEngine::open(dir.path(), GeoOptions::default()).unwrap();
    let stats = engine.stats().unwrap();
    assert_eq!(stats.num_features, 1);
}

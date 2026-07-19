//! Basic CRUD, sync, and reopen tests for `storage-geo`.

use geo::Point;
use storage_geo::{GeoEngine, GeoOptions, Geometry, PropertyMap, SpatialQuery};

fn temp_engine() -> (tempfile::TempDir, GeoEngine) {
    let dir = tempfile::tempdir().unwrap();
    let engine = GeoEngine::open(dir.path(), GeoOptions::default()).unwrap();
    (dir, engine)
}

fn props(name: &str) -> PropertyMap {
    let mut m = PropertyMap::new();
    m.insert("name".to_string(), name.as_bytes().to_vec());
    m
}

#[test]
fn insert_and_get_feature() {
    let (_dir, engine) = temp_engine();
    engine
        .insert_feature(b"f1", Geometry::Point(Point::new(2.35, 48.85)), props("paris"))
        .unwrap();

    let feature = engine.get_feature(b"f1").unwrap().expect("feature exists");
    assert_eq!(feature.id, b"f1");
    assert_eq!(feature.properties["name"], b"paris"[..]);
}

#[test]
fn upsert_overwrites_feature() {
    let (_dir, engine) = temp_engine();
    engine
        .insert_feature(b"f1", Geometry::Point(Point::new(0.0, 0.0)), props("a"))
        .unwrap();
    engine
        .insert_feature(b"f1", Geometry::Point(Point::new(1.0, 1.0)), props("b"))
        .unwrap();

    let feature = engine.get_feature(b"f1").unwrap().expect("feature exists");
    assert_eq!(feature.properties["name"], b"b"[..]);
}

#[test]
fn update_property() {
    let (_dir, engine) = temp_engine();
    engine
        .insert_feature(b"f1", Geometry::Point(Point::new(0.0, 0.0)), props("a"))
        .unwrap();
    engine
        .update_property(b"f1", "name", b"c".to_vec())
        .unwrap();

    assert_eq!(
        engine.get_property(b"f1", "name").unwrap().unwrap(),
        b"c"[..]
    );
}

#[test]
fn delete_feature() {
    let (_dir, engine) = temp_engine();
    engine
        .insert_feature(b"f1", Geometry::Point(Point::new(0.0, 0.0)), props("a"))
        .unwrap();
    assert!(engine.delete_feature(b"f1").unwrap());
    assert!(engine.get_feature(b"f1").unwrap().is_none());
}

#[test]
fn sync_and_reopen() {
    let (dir, engine) = temp_engine();
    engine
        .insert_feature(b"f1", Geometry::Point(Point::new(2.35, 48.85)), props("paris"))
        .unwrap();
    engine
        .insert_feature(b"f2", Geometry::Point(Point::new(13.4, 52.5)), props("berlin"))
        .unwrap();
    engine.sync().unwrap();
    drop(engine);

    let engine = GeoEngine::open(dir.path(), GeoOptions::default()).unwrap();
    assert_eq!(engine.get_feature(b"f1").unwrap().unwrap().properties["name"], b"paris"[..]);
    assert_eq!(engine.get_feature(b"f2").unwrap().unwrap().properties["name"], b"berlin"[..]);

    let results = engine
        .query(&SpatialQuery::BBox {
            min_lon: 0.0,
            min_lat: 40.0,
            max_lon: 10.0,
            max_lat: 50.0,
        })
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, b"f1");
}

#[test]
fn engine_trait_scan() {
    use storage_traits::Engine;
    let (_dir, engine) = temp_engine();
    engine
        .insert_feature(b"b", Geometry::Point(Point::new(0.0, 0.0)), PropertyMap::new())
        .unwrap();
    engine
        .insert_feature(b"a", Geometry::Point(Point::new(1.0, 1.0)), PropertyMap::new())
        .unwrap();

    let mut cursor = engine.scan(Some(b"a"[..].as_ref()), Some(b"b"[..].as_ref())).unwrap();
    let (k, _v) = cursor.next().unwrap().unwrap();
    assert_eq!(k.as_ref(), b"a");
    assert!(cursor.next().is_none());
}

#[test]
fn stats_report_features() {
    let (_dir, engine) = temp_engine();
    engine
        .insert_feature(b"f1", Geometry::Point(Point::new(0.0, 0.0)), PropertyMap::new())
        .unwrap();
    let stats = engine.stats().unwrap();
    assert_eq!(stats.num_features, 1);
    assert_eq!(stats.name, "storage-geo");
}

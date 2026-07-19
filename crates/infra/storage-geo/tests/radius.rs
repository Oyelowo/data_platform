//! Within-distance (DWithin) integration tests.

use geo::Point;
use storage_geo::{GeoEngine, GeoOptions, Geometry, PropertyMap, SpatialQuery};

fn temp_engine() -> (tempfile::TempDir, GeoEngine) {
    let dir = tempfile::tempdir().unwrap();
    let engine = GeoEngine::open(dir.path(), GeoOptions::default()).unwrap();
    (dir, engine)
}

#[test]
fn dwithin_finds_nearby_points() {
    let (_dir, engine) = temp_engine();
    // Paris
    engine
        .insert_feature(
            b"paris",
            Geometry::Point(Point::new(2.35, 48.85)),
            PropertyMap::new(),
        )
        .unwrap();
    // Berlin
    engine
        .insert_feature(
            b"berlin",
            Geometry::Point(Point::new(13.4, 52.5)),
            PropertyMap::new(),
        )
        .unwrap();

    // Distance Paris -> Berlin is ~878 km.
    let results = engine
        .query(&SpatialQuery::DWithin {
            point: Point::new(2.35, 48.85),
            distance_m: 100_000.0,
        })
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, b"paris");
}

#[test]
fn dwithin_crosses_meridian() {
    let (_dir, engine) = temp_engine();
    engine
        .insert_feature(b"a", Geometry::Point(Point::new(179.0, 0.0)), PropertyMap::new())
        .unwrap();
    engine
        .insert_feature(b"b", Geometry::Point(Point::new(-179.0, 0.0)), PropertyMap::new())
        .unwrap();

    let results = engine
        .query(&SpatialQuery::DWithin {
            point: Point::new(179.0, 0.0),
            distance_m: 300_000.0,
        })
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, b"a");
}

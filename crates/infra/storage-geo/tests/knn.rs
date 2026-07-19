//! K-nearest-neighbor integration tests.

use geo::Point;
use storage_geo::{GeoEngine, GeoOptions, Geometry, PropertyMap, SpatialQuery};

fn temp_engine() -> (tempfile::TempDir, GeoEngine) {
    let dir = tempfile::tempdir().unwrap();
    let engine = GeoEngine::open(dir.path(), GeoOptions::default()).unwrap();
    (dir, engine)
}

#[test]
fn nearest_points_ordered() {
    let (_dir, engine) = temp_engine();
    engine
        .insert_feature(b"near", Geometry::Point(Point::new(0.1, 0.1)), PropertyMap::new())
        .unwrap();
    engine
        .insert_feature(b"mid", Geometry::Point(Point::new(1.0, 1.0)), PropertyMap::new())
        .unwrap();
    engine
        .insert_feature(b"far", Geometry::Point(Point::new(2.0, 2.0)), PropertyMap::new())
        .unwrap();

    let results = engine
        .query(&SpatialQuery::Nearest {
            point: Point::new(0.0, 0.0),
            k: 2,
        })
        .unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].id, b"near");
    assert_eq!(results[1].id, b"mid");
}

#[test]
fn nearest_with_k_zero_returns_empty() {
    let (_dir, engine) = temp_engine();
    engine
        .insert_feature(b"a", Geometry::Point(Point::new(0.0, 0.0)), PropertyMap::new())
        .unwrap();

    let results = engine
        .query(&SpatialQuery::Nearest {
            point: Point::new(0.0, 0.0),
            k: 0,
        })
        .unwrap();
    assert!(results.is_empty());
}

//! Spatial predicate integration tests.

use geo::{LineString, Point, Polygon};
use storage_geo::{GeoEngine, GeoOptions, Geometry, PropertyMap, SpatialQuery};

fn temp_engine() -> (tempfile::TempDir, GeoEngine) {
    let dir = tempfile::tempdir().unwrap();
    let engine = GeoEngine::open(dir.path(), GeoOptions::default()).unwrap();
    (dir, engine)
}

#[test]
fn bbox_query() {
    let (_dir, engine) = temp_engine();
    engine
        .insert_feature(b"a", Geometry::Point(Point::new(0.5, 0.5)), PropertyMap::new())
        .unwrap();
    engine
        .insert_feature(b"b", Geometry::Point(Point::new(2.0, 2.0)), PropertyMap::new())
        .unwrap();

    let results = engine
        .query(&SpatialQuery::BBox {
            min_lon: 0.0,
            min_lat: 0.0,
            max_lon: 1.0,
            max_lat: 1.0,
        })
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, b"a");
}

#[test]
fn intersects_polygon() {
    let (_dir, engine) = temp_engine();
    engine
        .insert_feature(b"inside", Geometry::Point(Point::new(0.5, 0.5)), PropertyMap::new())
        .unwrap();
    engine
        .insert_feature(b"outside", Geometry::Point(Point::new(2.0, 2.0)), PropertyMap::new())
        .unwrap();

    let poly = Geometry::Polygon(Polygon::new(
        LineString::from(vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)]),
        vec![],
    ));
    let results = engine.query(&SpatialQuery::Intersects(poly)).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, b"inside");
}

#[test]
fn contains_point() {
    let (_dir, engine) = temp_engine();
    let poly = Geometry::Polygon(Polygon::new(
        LineString::from(vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)]),
        vec![],
    ));
    engine
        .insert_feature(b"poly", poly.clone(), PropertyMap::new())
        .unwrap();

    let point = Geometry::Point(Point::new(0.5, 0.5));
    let results = engine.query(&SpatialQuery::Contains(point)).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, b"poly");
}

#[test]
fn within_polygon() {
    let (_dir, engine) = temp_engine();
    let poly = Geometry::Polygon(Polygon::new(
        LineString::from(vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)]),
        vec![],
    ));
    engine
        .insert_feature(b"point", Geometry::Point(Point::new(0.5, 0.5)), PropertyMap::new())
        .unwrap();

    let results = engine.query(&SpatialQuery::Within(poly)).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, b"point");
}

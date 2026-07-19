//! WKB round-trip integration tests.

use geo::{LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};
use storage_geo::{GeoEngine, GeoOptions, Geometry, PropertyMap};

fn temp_engine() -> (tempfile::TempDir, GeoEngine) {
    let dir = tempfile::tempdir().unwrap();
    let engine = GeoEngine::open(dir.path(), GeoOptions::default()).unwrap();
    (dir, engine)
}

#[test]
fn roundtrip_all_geometry_types() {
    let (_dir, engine) = temp_engine();

    let geometries = vec![
        Geometry::Point(Point::new(12.3, 45.6)),
        Geometry::LineString(LineString::from(vec![(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)])),
        Geometry::Polygon(Polygon::new(
            LineString::from(vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)]),
            vec![LineString::from(vec![
                (0.25, 0.25),
                (0.75, 0.25),
                (0.75, 0.75),
                (0.25, 0.75),
                (0.25, 0.25),
            ])],
        )),
        Geometry::MultiPoint(MultiPoint::from(vec![(0.0, 0.0), (1.0, 2.0)])),
        Geometry::MultiLineString(MultiLineString::new(vec![
            LineString::from(vec![(0.0, 0.0), (1.0, 1.0)]),
            LineString::from(vec![(2.0, 2.0), (3.0, 3.0)]),
        ])),
        Geometry::MultiPolygon(MultiPolygon::new(vec![Polygon::new(
            LineString::from(vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)]),
            vec![],
        )])),
        Geometry::GeometryCollection(vec![
            Geometry::Point(Point::new(1.0, 2.0)),
            Geometry::LineString(LineString::from(vec![(0.0, 0.0), (1.0, 1.0)])),
        ]),
    ];

    for (i, geom) in geometries.into_iter().enumerate() {
        let id = format!("g{i}").into_bytes();
        engine
            .insert_feature(id.clone(), geom.clone(), PropertyMap::new())
            .unwrap();
        let fetched = engine.get_feature(&id).unwrap().expect("feature exists");
        assert_eq!(fetched.geometry, geom);
    }
}

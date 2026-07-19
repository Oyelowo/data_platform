//! Property-based tests validating spatial queries against a brute-force oracle.

use geo::Point;
use proptest::prelude::*;
use storage_geo::{GeoEngine, GeoOptions, Geometry, PropertyMap, SpatialQuery};

fn temp_engine() -> (tempfile::TempDir, GeoEngine) {
    let dir = tempfile::tempdir().unwrap();
    let engine = GeoEngine::open(dir.path(), GeoOptions::default()).unwrap();
    (dir, engine)
}

prop_compose! {
    fn point_strategy()(lon in -180.0f64..=180.0, lat in -90.0f64..=90.0) -> Point<f64> {
        Point::new(lon, lat)
    }
}

proptest! {
    #[test]
    fn bbox_query_matches_brute_force(
        points in prop::collection::vec(point_strategy(), 0..50),
        min_lon in -180.0f64..=180.0,
        max_lon in -180.0f64..=180.0,
        min_lat in -90.0f64..=90.0,
        max_lat in -90.0f64..=90.0,
    ) {
        let (_dir, engine) = temp_engine();
        for (i, p) in points.iter().enumerate() {
            let id = format!("p{i}").into_bytes();
            engine.insert_feature(id, Geometry::Point(*p), PropertyMap::new()).unwrap();
        }

        let (min_lon, max_lon) = if min_lon <= max_lon { (min_lon, max_lon) } else { (max_lon, min_lon) };
        let (min_lat, max_lat) = if min_lat <= max_lat { (min_lat, max_lat) } else { (max_lat, min_lat) };

        let results = engine.query(&SpatialQuery::BBox { min_lon, min_lat, max_lon, max_lat }).unwrap();
        let result_ids: std::collections::HashSet<_> = results.into_iter().map(|f| f.id).collect();

        let mut expected = std::collections::HashSet::new();
        for (i, p) in points.iter().enumerate() {
            let lon = p.0.x;
            let lat = p.0.y;
            if lon >= min_lon && lon <= max_lon && lat >= min_lat && lat <= max_lat {
                expected.insert(format!("p{i}").into_bytes());
            }
        }

        prop_assert_eq!(result_ids, expected);
    }
}

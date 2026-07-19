//! Spatial query API and execution.

use std::collections::HashSet;

use geo::Point;

use crate::feature::{Feature, Geometry};
use crate::index::SpatialIndex;
use crate::query::distance::{distance_bbox, haversine_meters};
use crate::query::predicate::{evaluate, Predicate};
use crate::store::FeatureStore;

pub mod distance;
pub mod predicate;

/// A spatial query against the engine.
#[derive(Debug, Clone, PartialEq)]
pub enum SpatialQuery {
    /// Bounding-box query.
    BBox {
        /// Minimum longitude.
        min_lon: f64,
        /// Minimum latitude.
        min_lat: f64,
        /// Maximum longitude.
        max_lon: f64,
        /// Maximum latitude.
        max_lat: f64,
    },
    /// Features whose geometry intersects the supplied geometry.
    Intersects(Geometry),
    /// Features whose geometry contains the supplied geometry.
    Contains(Geometry),
    /// Features whose geometry is within the supplied geometry.
    Within(Geometry),
    /// Features within `distance_m` of `point`.
    DWithin {
        /// Query point.
        point: Point<f64>,
        /// Distance in meters.
        distance_m: f64,
    },
    /// `k` nearest features to `point`.
    Nearest {
        /// Query point.
        point: Point<f64>,
        /// Number of results.
        k: usize,
    },
}

/// Execute a spatial query and return matching features.
pub fn execute(
    index: &SpatialIndex,
    store: &FeatureStore,
    query: &SpatialQuery,
) -> crate::Result<Vec<Feature>> {
    match query {
        SpatialQuery::BBox {
            min_lon,
            min_lat,
            max_lon,
            max_lat,
        } => fetch_candidates(index, store, *min_lon, *min_lat, *max_lon, *max_lat, |_, _| true),
        SpatialQuery::Intersects(geometry) => {
            let (min_lon, min_lat, max_lon, max_lat) = geometry
                .envelope()
                .ok_or_else(|| crate::Error::invalid_geometry("query geometry has no envelope"))?;
            fetch_candidates(
                index,
                store,
                min_lon,
                min_lat,
                max_lon,
                max_lat,
                |candidate, _| evaluate(Predicate::Intersects, candidate, geometry),
            )
        }
        SpatialQuery::Contains(geometry) => {
            let (min_lon, min_lat, max_lon, max_lat) = geometry
                .envelope()
                .ok_or_else(|| crate::Error::invalid_geometry("query geometry has no envelope"))?;
            fetch_candidates(
                index,
                store,
                min_lon,
                min_lat,
                max_lon,
                max_lat,
                |candidate, _| evaluate(Predicate::Contains, candidate, geometry),
            )
        }
        SpatialQuery::Within(geometry) => {
            let (min_lon, min_lat, max_lon, max_lat) = geometry
                .envelope()
                .ok_or_else(|| crate::Error::invalid_geometry("query geometry has no envelope"))?;
            fetch_candidates(
                index,
                store,
                min_lon,
                min_lat,
                max_lon,
                max_lat,
                |candidate, _| evaluate(Predicate::Within, candidate, geometry),
            )
        }
        SpatialQuery::DWithin {
            point,
            distance_m,
        } => {
            let (min_lon, min_lat, max_lon, max_lat) = distance_bbox(point, *distance_m);
            fetch_candidates(
                index,
                store,
                min_lon,
                min_lat,
                max_lon,
                max_lat,
                |candidate, _| match candidate {
                    Geometry::Point(p) => haversine_meters(point, p) <= *distance_m,
                    _ => geometry_intersects_envelope_point(candidate, point, *distance_m),
                },
            )
        }
        SpatialQuery::Nearest { point, k } => nearest_neighbors(index, store, point, *k),
    }
}

fn fetch_candidates(
    index: &SpatialIndex,
    store: &FeatureStore,
    min_lon: f64,
    min_lat: f64,
    max_lon: f64,
    max_lat: f64,
    mut filter: impl FnMut(&Geometry, &Feature) -> bool,
) -> crate::Result<Vec<Feature>> {
    let candidates = index.intersecting_bbox(min_lon, min_lat, max_lon, max_lat);
    let mut results = Vec::with_capacity(candidates.len());
    let mut seen = HashSet::with_capacity(candidates.len());
    for entry in candidates {
        if !seen.insert(entry.id.clone()) {
            continue;
        }
        if let Some(feature) = store.get(entry.address)?
            && filter(&feature.geometry, &feature)
        {
            results.push(feature);
        }
    }
    Ok(results)
}

fn nearest_neighbors(
    index: &SpatialIndex,
    store: &FeatureStore,
    point: &Point<f64>,
    k: usize,
) -> crate::Result<Vec<Feature>> {
    if k == 0 {
        return Ok(Vec::new());
    }

    // Ask for a generous number of envelope candidates so that Haversine
    // refinement can produce exactly `k` results even if some candidates are
    // stale.
    let candidates = index.nearest(point, k.saturating_mul(4).max(64));
    let mut scored: Vec<(f64, Feature)> = Vec::with_capacity(candidates.len());
    let mut seen = HashSet::with_capacity(candidates.len());
    for entry in candidates {
        if !seen.insert(entry.id.clone()) {
            continue;
        }
        if let Some(feature) = store.get(entry.address)? {
            let dist = haversine_geometry_point(&feature.geometry, point);
            scored.push((dist, feature));
        }
    }

    scored.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(scored.into_iter().take(k).map(|(_, f)| f).collect())
}

fn haversine_geometry_point(geometry: &Geometry, point: &Point<f64>) -> f64 {
    match geometry {
        Geometry::Point(p) => haversine_meters(point, p),
        _ => {
            // For non-point geometries use a cheap approximation: distance from
            // the query point to the geometry's envelope centroid.
            let (min_lon, min_lat, max_lon, max_lat) = geometry
                .envelope()
                .unwrap_or((point.0.x, point.0.y, point.0.x, point.0.y));
            let centroid = Point::new((min_lon + max_lon) / 2.0, (min_lat + max_lat) / 2.0);
            haversine_meters(point, &centroid)
        }
    }
}

fn geometry_intersects_envelope_point(
    geometry: &Geometry,
    point: &Point<f64>,
    distance_m: f64,
) -> bool {
    // Build a small circle polygon around the point and test intersection.
    let bbox = distance_bbox(point, distance_m);
    let poly = Geometry::Polygon(geo::Polygon::new(
        geo::LineString::from(vec![
            (bbox.0, bbox.1),
            (bbox.2, bbox.1),
            (bbox.2, bbox.3),
            (bbox.0, bbox.3),
            (bbox.0, bbox.1),
        ]),
        vec![],
    ));
    evaluate(Predicate::Intersects, geometry, &poly)
}

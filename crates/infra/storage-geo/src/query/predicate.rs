//! Spatial predicate evaluation using `geo` algorithms.

use crate::feature::geometry_contains;
use crate::feature::geometry_intersects;
use crate::feature::geometry_within;
use crate::feature::Geometry;

/// Spatial predicate kind.
#[derive(Debug, Clone, PartialEq)]
pub enum Predicate {
    /// Strict envelope intersection.
    Intersects,
    /// `candidate` contains `query_geometry`.
    Contains,
    /// `candidate` is within `query_geometry`.
    Within,
}

/// Evaluate a predicate between a candidate geometry and a query geometry.
pub fn evaluate(predicate: Predicate, candidate: &Geometry, query: &Geometry) -> bool {
    match predicate {
        Predicate::Intersects => geometry_intersects(candidate, query),
        Predicate::Contains => geometry_contains(candidate, query),
        Predicate::Within => geometry_within(candidate, query),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{LineString, Point, Polygon};

    #[test]
    fn point_in_polygon_within() {
        let poly = Geometry::Polygon(Polygon::new(
            LineString::from(vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)]),
            vec![],
        ));
        let point = Geometry::Point(Point::new(0.5, 0.5));
        assert!(evaluate(Predicate::Within, &point, &poly));
        assert!(evaluate(Predicate::Contains, &poly, &point));
    }

    #[test]
    fn point_outside_polygon_not_within() {
        let poly = Geometry::Polygon(Polygon::new(
            LineString::from(vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)]),
            vec![],
        ));
        let point = Geometry::Point(Point::new(2.0, 2.0));
        assert!(!evaluate(Predicate::Within, &point, &poly));
    }

    #[test]
    fn line_intersects_polygon() {
        let poly = Geometry::Polygon(Polygon::new(
            LineString::from(vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)]),
            vec![],
        ));
        let line = Geometry::LineString(LineString::from(vec![(0.5, -0.5), (0.5, 1.5)]));
        assert!(evaluate(Predicate::Intersects, &line, &poly));
    }
}

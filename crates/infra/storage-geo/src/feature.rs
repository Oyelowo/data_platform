//! Feature model and geometry helpers.

use std::collections::BTreeMap;

use geo::{
    BoundingRect, Contains, Coord, GeometryCollection, Intersects, LineString, MultiLineString,
    MultiPoint, MultiPolygon, Point, Polygon, Rect,
};

/// Opaque property map stored with each feature.
pub type PropertyMap = BTreeMap<String, Vec<u8>>;

/// A geographic feature: an id, a geometry, and arbitrary properties.
#[derive(Debug, Clone, PartialEq)]
pub struct Feature {
    /// User-provided primary key.
    pub id: Vec<u8>,
    /// 2D WGS84 geometry.
    pub geometry: Geometry,
    /// Opaque key-value properties.
    pub properties: PropertyMap,
}

impl Feature {
    /// Create a new feature.
    pub fn new(id: impl Into<Vec<u8>>, geometry: Geometry, properties: PropertyMap) -> Self {
        Self {
            id: id.into(),
            geometry,
            properties,
        }
    }
}

/// 2D geometry enum wrapping `geo` types.
#[derive(Debug, Clone, PartialEq)]
pub enum Geometry {
    /// A single point.
    Point(Point<f64>),
    /// A line string.
    LineString(LineString<f64>),
    /// A polygon with optional interior rings.
    Polygon(Polygon<f64>),
    /// A collection of points.
    MultiPoint(MultiPoint<f64>),
    /// A collection of line strings.
    MultiLineString(MultiLineString<f64>),
    /// A collection of polygons.
    MultiPolygon(MultiPolygon<f64>),
    /// A heterogeneous collection of geometries.
    GeometryCollection(Vec<Geometry>),
}

impl Geometry {
    /// Compute the axis-aligned bounding rectangle in degrees.
    pub fn bounding_rect(&self) -> Option<Rect<f64>> {
        match self {
            Geometry::Point(p) => Some(Rect::new((p.0.x, p.0.y), (p.0.x, p.0.y))),
            Geometry::LineString(ls) => ls.bounding_rect(),
            Geometry::Polygon(poly) => poly.bounding_rect(),
            Geometry::MultiPoint(mp) => mp.bounding_rect(),
            Geometry::MultiLineString(mls) => mls.bounding_rect(),
            Geometry::MultiPolygon(mp) => mp.bounding_rect(),
            Geometry::GeometryCollection(children) => {
                let mut rects = children.iter().filter_map(|g| g.bounding_rect());
                let first = rects.next()?;
                Some(rects.fold(first, |a, b| {
                    Rect::new(
                        (a.min().x.min(b.min().x), a.min().y.min(b.min().y)),
                        (a.max().x.max(b.max().x), a.max().y.max(b.max().y)),
                    )
                }))
            }
        }
    }

    /// Return the envelope as `(min_lon, min_lat, max_lon, max_lat)`.
    pub fn envelope(&self) -> Option<(f64, f64, f64, f64)> {
        self.bounding_rect()
            .map(|r| (r.min().x, r.min().y, r.max().x, r.max().y))
    }

    /// Validate the geometry.
    ///
    /// Polygons must have at least four coordinates in each ring, must be closed,
    /// and must not self-intersect. Geometry collections are validated recursively.
    pub fn validate(&self) -> crate::Result<()> {
        match self {
            Geometry::Point(_) => Ok(()),
            Geometry::LineString(ls) => {
                if ls.0.len() < 2 {
                    return Err(crate::Error::invalid_geometry(
                        "line string must contain at least two points",
                    ));
                }
                Ok(())
            }
            Geometry::Polygon(poly) => validate_polygon(poly),
            Geometry::MultiPoint(mp) => {
                if mp.0.is_empty() {
                    return Err(crate::Error::invalid_geometry(
                        "multi-point must contain at least one point",
                    ));
                }
                Ok(())
            }
            Geometry::MultiLineString(mls) => {
                for ls in &mls.0 {
                    if ls.0.len() < 2 {
                        return Err(crate::Error::invalid_geometry(
                            "multi-line-string contains an empty line string",
                        ));
                    }
                }
                Ok(())
            }
            Geometry::MultiPolygon(mp) => {
                for poly in &mp.0 {
                    validate_polygon(poly)?;
                }
                Ok(())
            }
            Geometry::GeometryCollection(children) => {
                if children.is_empty() {
                    return Err(crate::Error::invalid_geometry(
                        "geometry collection must contain at least one geometry",
                    ));
                }
                for child in children {
                    child.validate()?;
                }
                Ok(())
            }
        }
    }

    /// Convert to a `geo::Geometry` for algorithmic operations.
    pub fn to_geo_geometry(&self) -> geo::Geometry<f64> {
        match self.clone() {
            Geometry::Point(g) => geo::Geometry::Point(g),
            Geometry::LineString(g) => geo::Geometry::LineString(g),
            Geometry::Polygon(g) => geo::Geometry::Polygon(g),
            Geometry::MultiPoint(g) => geo::Geometry::MultiPoint(g),
            Geometry::MultiLineString(g) => geo::Geometry::MultiLineString(g),
            Geometry::MultiPolygon(g) => geo::Geometry::MultiPolygon(g),
            Geometry::GeometryCollection(children) => geo::Geometry::GeometryCollection(
                GeometryCollection::new_from(children.into_iter().map(|c| c.to_geo_geometry()).collect()),
            ),
        }
    }
}

fn validate_polygon(poly: &Polygon<f64>) -> crate::Result<()> {
    let exterior = poly.exterior();
    if exterior.0.len() < 4 {
        return Err(crate::Error::invalid_geometry(
            "polygon exterior ring must contain at least four coordinates",
        ));
    }
    if exterior.0.first() != exterior.0.last() {
        return Err(crate::Error::invalid_geometry(
            "polygon exterior ring is not closed",
        ));
    }
    if ring_self_intersects(exterior) {
        return Err(crate::Error::invalid_geometry(
            "polygon exterior ring self-intersects",
        ));
    }
    for interior in poly.interiors() {
        if interior.0.len() < 4 {
            return Err(crate::Error::invalid_geometry(
                "polygon interior ring must contain at least four coordinates",
            ));
        }
        if interior.0.first() != interior.0.last() {
            return Err(crate::Error::invalid_geometry(
                "polygon interior ring is not closed",
            ));
        }
        if ring_self_intersects(interior) {
            return Err(crate::Error::invalid_geometry(
                "polygon interior ring self-intersects",
            ));
        }
    }
    Ok(())
}

fn ring_self_intersects(ring: &LineString<f64>) -> bool {
    let coords: Vec<Coord<f64>> = ring.0.clone();
    let n = coords.len();
    if n < 4 {
        return false;
    }
    // The last closing coordinate is a duplicate of the first; ignore it for
    // segment iteration.
    for i in 0..n - 2 {
        let a1 = coords[i];
        let a2 = coords[i + 1];
        for j in i + 2..n - 1 {
            // Adjacent segments share an endpoint; that is not a self-intersection.
            if j == i + 1 {
                continue;
            }
            let b1 = coords[j];
            let b2 = coords[j + 1];
            if segments_intersect(a1, a2, b1, b2) {
                return true;
            }
        }
    }
    false
}

fn segments_intersect(a1: Coord<f64>, a2: Coord<f64>, b1: Coord<f64>, b2: Coord<f64>) -> bool {
    fn orientation(p: Coord<f64>, q: Coord<f64>, r: Coord<f64>) -> f64 {
        (q.y - p.y) * (r.x - q.x) - (q.x - p.x) * (r.y - q.y)
    }

    let eps = 1e-12;

    // Shared endpoints are allowed in closed polygon rings; they do not
    // constitute a self-intersection.
    let same = |p: Coord<f64>, q: Coord<f64>| {
        (p.x - q.x).abs() <= eps && (p.y - q.y).abs() <= eps
    };
    if same(a1, b1) || same(a1, b2) || same(a2, b1) || same(a2, b2) {
        return false;
    }

    let o1 = orientation(a1, a2, b1);
    let o2 = orientation(a1, a2, b2);
    let o3 = orientation(b1, b2, a1);
    let o4 = orientation(b1, b2, a2);

    let general_case = (o1 > eps && o2 < -eps || o1 < -eps && o2 > eps)
        && (o3 > eps && o4 < -eps || o3 < -eps && o4 > eps);
    if general_case {
        return true;
    }

    // Collinear cases: check whether an endpoint lies on the other segment.
    let on_segment = |p: Coord<f64>, q: Coord<f64>, r: Coord<f64>| {
        q.x >= p.x.min(r.x) - eps
            && q.x <= p.x.max(r.x) + eps
            && q.y >= p.y.min(r.y) - eps
            && q.y <= p.y.max(r.y) + eps
    };

    if o1.abs() <= eps && on_segment(a1, b1, a2) {
        return true;
    }
    if o2.abs() <= eps && on_segment(a1, b2, a2) {
        return true;
    }
    if o3.abs() <= eps && on_segment(b1, a1, b2) {
        return true;
    }
    if o4.abs() <= eps && on_segment(b1, a2, b2) {
        return true;
    }
    false
}

/// Test whether `a` intersects `b` using precise `geo` algorithms.
pub fn geometry_intersects(a: &Geometry, b: &Geometry) -> bool {
    let ga = a.to_geo_geometry();
    let gb = b.to_geo_geometry();
    ga.intersects(&gb)
}

/// Test whether `a` contains `b` using precise `geo` algorithms.
pub fn geometry_contains(a: &Geometry, b: &Geometry) -> bool {
    let ga = a.to_geo_geometry();
    let gb = b.to_geo_geometry();
    ga.contains(&gb)
}

/// Test whether `a` is within `b` using precise `geo` algorithms.
pub fn geometry_within(a: &Geometry, b: &Geometry) -> bool {
    geometry_contains(b, a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Point<f64> {
        Point::new(x, y)
    }

    #[test]
    fn point_envelope() {
        let g = Geometry::Point(p(10.0, 20.0));
        assert_eq!(g.envelope(), Some((10.0, 20.0, 10.0, 20.0)));
    }

    #[test]
    fn polygon_validation_rejects_self_intersection() {
        // A bow-tie polygon.
        let poly = Polygon::new(
            LineString::from(vec![(0.0, 0.0), (1.0, 1.0), (0.0, 1.0), (1.0, 0.0), (0.0, 0.0)]),
            vec![],
        );
        let g = Geometry::Polygon(poly);
        assert!(g.validate().is_err());
    }

    #[test]
    fn polygon_validation_accepts_valid_ring() {
        let poly = Polygon::new(
            LineString::from(vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)]),
            vec![],
        );
        let g = Geometry::Polygon(poly);
        assert!(g.validate().is_ok());
    }

    #[test]
    fn geometry_collection_recursive_validation() {
        let valid = Geometry::Polygon(Polygon::new(
            LineString::from(vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)]),
            vec![],
        ));
        let invalid = Geometry::Polygon(Polygon::new(
            LineString::from(vec![(0.0, 0.0), (1.0, 1.0), (0.0, 1.0), (1.0, 0.0), (0.0, 0.0)]),
            vec![],
        ));
        assert!(Geometry::GeometryCollection(vec![valid.clone()]).validate().is_ok());
        assert!(Geometry::GeometryCollection(vec![valid, invalid])
            .validate()
            .is_err());
    }
}

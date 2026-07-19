//! Minimal Well-Known Binary encode/decode for the geometry types supported by
//! `storage-geo`.
//!
//! All WKB values use little-endian byte order and 2D coordinates. Supported
//! geometry types are Point, LineString, Polygon, MultiPoint, MultiLineString,
//! MultiPolygon, and GeometryCollection.

use bytes::{Buf, BufMut};

use crate::feature::Geometry;

const WKB_LITTLE_ENDIAN: u8 = 1;

const WKB_POINT: u32 = 1;
const WKB_LINESTRING: u32 = 2;
const WKB_POLYGON: u32 = 3;
const WKB_MULTIPOINT: u32 = 4;
const WKB_MULTILINESTRING: u32 = 5;
const WKB_MULTIPOLYGON: u32 = 6;
const WKB_GEOMETRYCOLLECTION: u32 = 7;

/// Encode a geometry to WKB bytes.
pub fn encode(geometry: &Geometry) -> crate::Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(estimate_size(geometry));
    encode_geometry(geometry, &mut buf);
    Ok(buf)
}

fn estimate_size(geometry: &Geometry) -> usize {
    match geometry {
        Geometry::Point(_) => 1 + 4 + 16,
        Geometry::LineString(ls) => 1 + 4 + 4 + ls.0.len() * 16,
        Geometry::Polygon(poly) => {
            let mut n = 1 + 4 + 4;
            n += ring_estimate(poly.exterior());
            for interior in poly.interiors() {
                n += ring_estimate(interior);
            }
            n
        }
        Geometry::MultiPoint(mp) => 1 + 4 + 4 + mp.0.len() * (1 + 4 + 16),
        Geometry::MultiLineString(mls) => {
            1 + 4 + 4 + mls.0.iter().map(|ls| estimate_size(&Geometry::LineString(ls.clone()))).sum::<usize>()
        }
        Geometry::MultiPolygon(mp) => {
            1 + 4 + 4 + mp.0.iter().map(|poly| estimate_size(&Geometry::Polygon(poly.clone()))).sum::<usize>()
        }
        Geometry::GeometryCollection(children) => {
            1 + 4 + 4 + children.iter().map(estimate_size).sum::<usize>()
        }
    }
}

fn ring_estimate(ring: &geo::LineString<f64>) -> usize {
    4 + ring.0.len() * 16
}

fn encode_geometry(geometry: &Geometry, buf: &mut Vec<u8>) {
    buf.put_u8(WKB_LITTLE_ENDIAN);
    match geometry {
        Geometry::Point(p) => {
            buf.put_u32_le(WKB_POINT);
            put_coord(buf, p.0);
        }
        Geometry::LineString(ls) => {
            buf.put_u32_le(WKB_LINESTRING);
            encode_line_string_body(ls, buf);
        }
        Geometry::Polygon(poly) => {
            buf.put_u32_le(WKB_POLYGON);
            encode_polygon_body(poly, buf);
        }
        Geometry::MultiPoint(mp) => {
            buf.put_u32_le(WKB_MULTIPOINT);
            buf.put_u32_le(mp.0.len() as u32);
            for p in &mp.0 {
                encode_geometry(&Geometry::Point(*p), buf);
            }
        }
        Geometry::MultiLineString(mls) => {
            buf.put_u32_le(WKB_MULTILINESTRING);
            buf.put_u32_le(mls.0.len() as u32);
            for ls in &mls.0 {
                encode_geometry(&Geometry::LineString(ls.clone()), buf);
            }
        }
        Geometry::MultiPolygon(mp) => {
            buf.put_u32_le(WKB_MULTIPOLYGON);
            buf.put_u32_le(mp.0.len() as u32);
            for poly in &mp.0 {
                encode_geometry(&Geometry::Polygon(poly.clone()), buf);
            }
        }
        Geometry::GeometryCollection(children) => {
            buf.put_u32_le(WKB_GEOMETRYCOLLECTION);
            buf.put_u32_le(children.len() as u32);
            for child in children {
                encode_geometry(child, buf);
            }
        }
    }
}

fn encode_line_string_body(ls: &geo::LineString<f64>, buf: &mut Vec<u8>) {
    buf.put_u32_le(ls.0.len() as u32);
    for coord in &ls.0 {
        put_coord(buf, *coord);
    }
}

fn encode_polygon_body(poly: &geo::Polygon<f64>, buf: &mut Vec<u8>) {
    let rings = std::iter::once(poly.exterior()).chain(poly.interiors());
    buf.put_u32_le(rings.clone().count() as u32);
    for ring in rings {
        encode_line_string_body(ring, buf);
    }
}

fn put_coord(buf: &mut Vec<u8>, coord: geo::Coord<f64>) {
    buf.put_f64_le(coord.x);
    buf.put_f64_le(coord.y);
}

/// Decode a geometry from WKB bytes.
pub fn decode(bytes: &[u8]) -> crate::Result<Geometry> {
    let mut cursor = bytes;
    decode_geometry(&mut cursor)
}

fn decode_geometry(cursor: &mut &[u8]) -> crate::Result<Geometry> {
    if cursor.len() < 5 {
        return Err(crate::Error::wkb("truncated geometry header"));
    }
    let byte_order = cursor.get_u8();
    if byte_order != WKB_LITTLE_ENDIAN {
        return Err(crate::Error::wkb("only little-endian WKB is supported"));
    }
    let ty = cursor.get_u32_le();
    match ty {
        WKB_POINT => {
            let coord = get_coord(cursor)?;
            Ok(Geometry::Point(geo::Point(coord)))
        }
        WKB_LINESTRING => Ok(Geometry::LineString(decode_line_string(cursor)?)),
        WKB_POLYGON => Ok(Geometry::Polygon(decode_polygon(cursor)?)),
        WKB_MULTIPOINT => {
            let count = get_count(cursor)?;
            let mut points = Vec::with_capacity(count);
            for _ in 0..count {
                match decode_geometry(cursor)? {
                    Geometry::Point(p) => points.push(p),
                    other => {
                        return Err(crate::Error::wkb(format!(
                            "expected Point in MultiPoint, got {}",
                            geometry_kind(&other)
                        )))
                    }
                }
            }
            Ok(Geometry::MultiPoint(geo::MultiPoint(points)))
        }
        WKB_MULTILINESTRING => {
            let count = get_count(cursor)?;
            let mut lines = Vec::with_capacity(count);
            for _ in 0..count {
                match decode_geometry(cursor)? {
                    Geometry::LineString(ls) => lines.push(ls),
                    other => {
                        return Err(crate::Error::wkb(format!(
                            "expected LineString in MultiLineString, got {}",
                            geometry_kind(&other)
                        )))
                    }
                }
            }
            Ok(Geometry::MultiLineString(geo::MultiLineString(lines)))
        }
        WKB_MULTIPOLYGON => {
            let count = get_count(cursor)?;
            let mut polys = Vec::with_capacity(count);
            for _ in 0..count {
                match decode_geometry(cursor)? {
                    Geometry::Polygon(poly) => polys.push(poly),
                    other => {
                        return Err(crate::Error::wkb(format!(
                            "expected Polygon in MultiPolygon, got {}",
                            geometry_kind(&other)
                        )))
                    }
                }
            }
            Ok(Geometry::MultiPolygon(geo::MultiPolygon(polys)))
        }
        WKB_GEOMETRYCOLLECTION => {
            let count = get_count(cursor)?;
            let mut children = Vec::with_capacity(count);
            for _ in 0..count {
                children.push(decode_geometry(cursor)?);
            }
            Ok(Geometry::GeometryCollection(children))
        }
        other => Err(crate::Error::wkb(format!("unsupported WKB type {other}"))),
    }
}

fn decode_line_string(cursor: &mut &[u8]) -> crate::Result<geo::LineString<f64>> {
    let count = get_count(cursor)?;
    let mut coords = Vec::with_capacity(count);
    for _ in 0..count {
        coords.push(get_coord(cursor)?);
    }
    Ok(geo::LineString::new(coords))
}

fn decode_polygon(cursor: &mut &[u8]) -> crate::Result<geo::Polygon<f64>> {
    let ring_count = get_count(cursor)?;
    if ring_count == 0 {
        return Ok(geo::Polygon::new(geo::LineString::new(vec![]), vec![]));
    }
    let exterior = decode_line_string(cursor)?;
    let mut interiors = Vec::with_capacity(ring_count.saturating_sub(1));
    for _ in 1..ring_count {
        interiors.push(decode_line_string(cursor)?);
    }
    Ok(geo::Polygon::new(exterior, interiors))
}

fn get_count(cursor: &mut &[u8]) -> crate::Result<usize> {
    if cursor.len() < 4 {
        return Err(crate::Error::wkb("truncated count field"));
    }
    Ok(cursor.get_u32_le() as usize)
}

fn get_coord(cursor: &mut &[u8]) -> crate::Result<geo::Coord<f64>> {
    if cursor.len() < 16 {
        return Err(crate::Error::wkb("truncated coordinate"));
    }
    Ok(geo::Coord {
        x: cursor.get_f64_le(),
        y: cursor.get_f64_le(),
    })
}

fn geometry_kind(g: &Geometry) -> &'static str {
    match g {
        Geometry::Point(_) => "Point",
        Geometry::LineString(_) => "LineString",
        Geometry::Polygon(_) => "Polygon",
        Geometry::MultiPoint(_) => "MultiPoint",
        Geometry::MultiLineString(_) => "MultiLineString",
        Geometry::MultiPolygon(_) => "MultiPolygon",
        Geometry::GeometryCollection(_) => "GeometryCollection",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::{LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};

    fn roundtrip(g: Geometry) {
        let encoded = encode(&g).unwrap();
        let decoded = decode(&encoded).unwrap();
        assert_eq!(g, decoded);
    }

    #[test]
    fn point_roundtrip() {
        roundtrip(Geometry::Point(Point::new(12.3, 45.6)));
    }

    #[test]
    fn line_string_roundtrip() {
        roundtrip(Geometry::LineString(LineString::from(vec![
            (0.0, 0.0),
            (1.0, 1.0),
            (2.0, 0.0),
        ])));
    }

    #[test]
    fn polygon_roundtrip() {
        roundtrip(Geometry::Polygon(Polygon::new(
            LineString::from(vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)]),
            vec![LineString::from(vec![
                (0.25, 0.25),
                (0.75, 0.25),
                (0.75, 0.75),
                (0.25, 0.75),
                (0.25, 0.25),
            ])],
        )));
    }

    #[test]
    fn multi_point_roundtrip() {
        roundtrip(Geometry::MultiPoint(MultiPoint::from(vec![
            (0.0, 0.0),
            (1.0, 2.0),
        ])));
    }

    #[test]
    fn multi_line_string_roundtrip() {
        roundtrip(Geometry::MultiLineString(MultiLineString::new(vec![
            LineString::from(vec![(0.0, 0.0), (1.0, 1.0)]),
            LineString::from(vec![(2.0, 2.0), (3.0, 3.0)]),
        ])));
    }

    #[test]
    fn multi_polygon_roundtrip() {
        roundtrip(Geometry::MultiPolygon(MultiPolygon::new(vec![Polygon::new(
            LineString::from(vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0), (0.0, 0.0)]),
            vec![],
        )])));
    }

    #[test]
    fn geometry_collection_roundtrip() {
        roundtrip(Geometry::GeometryCollection(vec![
            Geometry::Point(Point::new(1.0, 2.0)),
            Geometry::LineString(LineString::from(vec![(0.0, 0.0), (1.0, 1.0)])),
        ]));
    }

    #[test]
    fn empty_polygon_roundtrip() {
        roundtrip(Geometry::Polygon(Polygon::new(LineString::new(vec![]), vec![])));
    }
}

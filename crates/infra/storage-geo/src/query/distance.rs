//! Distance calculations for WGS84 coordinates.

use geo::Point;

const EARTH_RADIUS_M: f64 = 6_371_000.0;
const DEG_TO_RAD: f64 = std::f64::consts::PI / 180.0;

/// Haversine distance between two WGS84 points in meters.
pub fn haversine_meters(a: &Point<f64>, b: &Point<f64>) -> f64 {
    let lat1 = a.0.y * DEG_TO_RAD;
    let lat2 = b.0.y * DEG_TO_RAD;
    let dlat = (b.0.y - a.0.y) * DEG_TO_RAD;
    let dlon = (b.0.x - a.0.x) * DEG_TO_RAD;

    let sin_dlat = (dlat / 2.0).sin();
    let sin_dlon = (dlon / 2.0).sin();
    let aa = sin_dlat * sin_dlat + lat1.cos() * lat2.cos() * sin_dlon * sin_dlon;
    let c = 2.0 * aa.sqrt().atan2((1.0 - aa).sqrt());
    EARTH_RADIUS_M * c
}

/// Approximate axis-aligned bounding box that contains all points within
/// `distance_m` of `center`.
///
/// This is a conservative planar approximation used for the R-tree filter
/// stage; precise distance is checked afterwards using [`haversine_meters`].
pub fn distance_bbox(center: &Point<f64>, distance_m: f64) -> (f64, f64, f64, f64) {
    let lat = center.0.y.clamp(-90.0, 90.0);
    // Meters per degree of latitude is approximately constant.
    let lat_delta = distance_m / 111_320.0;
    // Meters per degree of longitude depends on latitude.
    let lon_scale = 111_320.0 * (lat * DEG_TO_RAD).cos().max(1e-12);
    let lon_delta = distance_m / lon_scale;

    let min_lat = (lat - lat_delta).clamp(-90.0, 90.0);
    let max_lat = (lat + lat_delta).clamp(-90.0, 90.0);
    let min_lon = center.0.x - lon_delta;
    let max_lon = center.0.x + lon_delta;
    (min_lon, min_lat, max_lon, max_lat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn haversine_equator() {
        let a = Point::new(0.0, 0.0);
        let b = Point::new(1.0, 0.0);
        let d = haversine_meters(&a, &b);
        // 1 degree of longitude at the equator is ~111.2 km.
        assert!((d - 111_195.0).abs() < 100.0);
    }

    #[test]
    fn haversine_poles() {
        let a = Point::new(0.0, 90.0);
        let b = Point::new(0.0, -90.0);
        let d = haversine_meters(&a, &b);
        assert!((d - 20_015_000.0).abs() < 1_000.0);
    }

    #[test]
    fn distance_bbox_wraps_latitudes() {
        let p = Point::new(0.0, 89.0);
        let (_, min_lat, _, max_lat) = distance_bbox(&p, 20_000_000.0);
        assert_eq!(min_lat, -90.0);
        assert_eq!(max_lat, 90.0);
    }
}

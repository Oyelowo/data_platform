//! Distance metrics for vector search.

use serde::{Deserialize, Serialize};

/// Distance metric used for nearest-neighbor search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DistanceMetric {
    /// Euclidean (L2) distance.
    Euclidean,
    /// Cosine distance: `1 - cosine_similarity`.
    Cosine,
    /// Negative dot product (so smaller is closer).
    DotProduct,
}

impl DistanceMetric {
    /// Compute the distance between two vectors according to this metric.
    ///
    /// # Errors
    ///
    /// Returns `None` if the vectors have different lengths.
    pub fn distance(&self, a: &[f32], b: &[f32]) -> Option<f32> {
        if a.len() != b.len() {
            return None;
        }
        match self {
            DistanceMetric::Euclidean => Some(euclidean(a, b)),
            DistanceMetric::Cosine => Some(cosine_distance(a, b)),
            DistanceMetric::DotProduct => Some(neg_dot_product(a, b)),
        }
    }

    /// Compute the distance between a query and a quantized vector.
    ///
    /// The quantized vector is represented by its per-dimension `min`, `max`,
    /// and `u8` codes. Dequantization happens on the fly to avoid extra copies.
    pub fn quantized_distance(
        &self,
        query: &[f32],
        min: &[f32],
        max: &[f32],
        codes: &[u8],
    ) -> Option<f32> {
        if query.len() != codes.len() || query.len() != min.len() || query.len() != max.len() {
            return None;
        }
        let dequantized: Vec<f32> = codes
            .iter()
            .zip(min.iter().zip(max.iter()))
            .map(|(&c, (&mn, &mx))| dequantize_scalar(c, mn, mx))
            .collect();
        self.distance(query, &dequantized)
    }
}

/// Euclidean distance between two equal-length vectors.
pub fn euclidean(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| {
            let d = x - y;
            d * d
        })
        .sum::<f32>()
        .sqrt()
}

/// Cosine distance: `1 - (a·b) / (||a|| ||b||)`.
pub fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        return 1.0;
    }
    1.0 - (dot / denom)
}

/// Negative dot product (so sorting ascending yields nearest neighbors).
pub fn neg_dot_product(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    -a.iter().zip(b.iter()).map(|(x, y)| x * y).sum::<f32>()
}

/// Compute the L2 norm of a vector.
pub fn l2_norm(v: &[f32]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

/// Normalize a vector to unit length for cosine/dot-product indexes.
pub fn normalize(v: &mut [f32]) {
    let norm = l2_norm(v);
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

/// Dequantize a single scalar quantized dimension.
pub fn dequantize_scalar(code: u8, min: f32, max: f32) -> f32 {
    if min == max {
        return min;
    }
    let t = f32::from(code) / 255.0;
    min + t * (max - min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn euclidean_identity() {
        let v = vec![1.0f32, 2.0, 3.0];
        assert!((euclidean(&v, &v) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn euclidean_simple() {
        let a = vec![0.0f32, 0.0];
        let b = vec![3.0f32, 4.0];
        assert!((euclidean(&a, &b) - 5.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_identical() {
        let v = vec![1.0f32, 2.0, 3.0];
        assert!((cosine_distance(&v, &v)).abs() < 1e-6);
    }

    #[test]
    fn cosine_orthogonal() {
        let a = vec![1.0f32, 0.0];
        let b = vec![0.0f32, 1.0];
        assert!((cosine_distance(&a, &b) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn dot_product_ordering() {
        let query = vec![1.0f32, 0.0];
        let close = vec![1.0f32, 0.0];
        let far = vec![0.0f32, 1.0];
        assert!(neg_dot_product(&query, &close) < neg_dot_product(&query, &far));
    }

    #[test]
    fn normalize_unit_length() {
        let mut v = vec![3.0f32, 4.0];
        normalize(&mut v);
        assert!((l2_norm(&v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn quantized_distance_roundtrip() {
        let a = vec![0.0f32, 0.5, 1.0];
        let b = vec![1.0f32, 0.5, 0.0];
        let min = vec![0.0f32; 3];
        let max = vec![1.0f32; 3];
        let codes: Vec<u8> = b.iter().map(|&x| (x * 255.0).round() as u8).collect();
        let raw = euclidean(&a, &b);
        let approx = DistanceMetric::Euclidean
            .quantized_distance(&a, &min, &max, &codes)
            .unwrap();
        assert!((raw - approx).abs() < 0.02);
    }
}

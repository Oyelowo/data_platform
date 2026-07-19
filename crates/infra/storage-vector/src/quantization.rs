//! Vector quantization helpers.

use serde::{Deserialize, Serialize};

/// State required to scalar-quantize vectors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScalarQuantizer {
    /// Per-dimension minimum value.
    pub min: Vec<f32>,
    /// Per-dimension maximum value.
    pub max: Vec<f32>,
}

impl ScalarQuantizer {
    /// Build a quantizer from a collection of vectors.
    ///
    /// Returns `None` if `vectors` is empty or vectors have inconsistent lengths.
    pub fn fit(vectors: &[Vec<f32>]) -> Option<Self> {
        if vectors.is_empty() {
            return None;
        }
        let dim = vectors[0].len();
        let mut min = vec![f32::INFINITY; dim];
        let mut max = vec![f32::NEG_INFINITY; dim];
        for v in vectors {
            if v.len() != dim {
                return None;
            }
            for (i, &x) in v.iter().enumerate() {
                if x < min[i] {
                    min[i] = x;
                }
                if x > max[i] {
                    max[i] = x;
                }
            }
        }
        Some(Self { min, max })
    }

    /// Quantize a single vector into `u8` codes.
    ///
    /// # Panics
    ///
    /// Panics if the vector length differs from the fitted dimension.
    pub fn quantize(&self, v: &[f32]) -> Vec<u8> {
        assert_eq!(v.len(), self.min.len());
        v.iter()
            .enumerate()
            .map(|(i, &x)| quantize_scalar(x, self.min[i], self.max[i]))
            .collect()
    }

    /// Dequantize a vector back to `f32`.
    pub fn dequantize(&self, codes: &[u8]) -> Vec<f32> {
        assert_eq!(codes.len(), self.min.len());
        codes
            .iter()
            .enumerate()
            .map(|(i, &c)| dequantize_scalar(c, self.min[i], self.max[i]))
            .collect()
    }

    /// Return the dimension of the quantizer.
    pub fn dimension(&self) -> usize {
        self.min.len()
    }
}

/// Quantize a single `f32` dimension to `u8`.
pub fn quantize_scalar(value: f32, min: f32, max: f32) -> u8 {
    if min == max {
        return 0;
    }
    let t = ((value - min) / (max - min)).clamp(0.0, 1.0);
    (t * 255.0).round() as u8
}

/// Dequantize a single `u8` code back to `f32`.
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
    fn roundtrip() {
        let vectors = vec![
            vec![0.0f32, 0.0, 0.0],
            vec![1.0f32, 0.5, 0.25],
        ];
        let q = ScalarQuantizer::fit(&vectors).unwrap();
        let original = vec![0.75f32, 0.25, 0.1];
        let codes = q.quantize(&original);
        let back = q.dequantize(&codes);
        for (a, b) in original.iter().zip(back.iter()) {
            assert!((a - b).abs() < 0.01);
        }
    }

    #[test]
    fn empty_fit_returns_none() {
        assert!(ScalarQuantizer::fit(&[]).is_none());
    }

    #[test]
    fn inconsistent_dimension_returns_none() {
        let vectors = vec![vec![1.0f32, 2.0], vec![3.0f32]];
        assert!(ScalarQuantizer::fit(&vectors).is_none());
    }

    #[test]
    fn quantize_bounds() {
        let q = ScalarQuantizer {
            min: vec![0.0f32, -1.0],
            max: vec![1.0f32, 1.0],
        };
        assert_eq!(q.quantize(&[0.0f32, -1.0]), vec![0, 0]);
        assert_eq!(q.quantize(&[1.0f32, 1.0]), vec![255, 255]);
    }
}

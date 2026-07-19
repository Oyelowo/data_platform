//! In-engine aggregation iterators.

use crate::format::{Sample, Timestamp, Value};

/// Supported aggregations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aggregation {
    /// Sum of all values.
    Sum,
    /// Number of values.
    Count,
    /// Arithmetic mean.
    Avg,
    /// Minimum value.
    Min,
    /// Maximum value.
    Max,
    /// Difference between last and first value.
    Rate,
}

/// Result of an aggregation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AggregateResult {
    /// No values were present.
    Empty,
    /// A scalar result.
    Scalar(f64),
}

/// Aggregate a sequence of samples.
pub fn aggregate_samples(
    samples: &[Sample],
    agg: Aggregation,
) -> crate::Result<AggregateResult> {
    let values: Vec<f64> = samples
        .iter()
        .filter_map(|s| match s.value {
            Value::F64(v) => Some(v),
            _ => None,
        })
        .collect();
    if values.is_empty() {
        return Ok(AggregateResult::Empty);
    }
    match agg {
        Aggregation::Count => Ok(AggregateResult::Scalar(values.len() as f64)),
        Aggregation::Sum => Ok(AggregateResult::Scalar(values.iter().sum())),
        Aggregation::Avg => Ok(AggregateResult::Scalar(
            values.iter().sum::<f64>() / values.len() as f64,
        )),
        Aggregation::Min => Ok(AggregateResult::Scalar(
            values.iter().copied().fold(f64::INFINITY, f64::min),
        )),
        Aggregation::Max => Ok(AggregateResult::Scalar(
            values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        )),
        Aggregation::Rate => {
            if values.len() < 2 {
                Ok(AggregateResult::Empty)
            } else {
                let first = values.first().copied().unwrap_or(0.0);
                let last = values.last().copied().unwrap_or(0.0);
                Ok(AggregateResult::Scalar(last - first))
            }
        }
    }
}

/// Aggregate samples grouped into fixed-width time buckets.
pub fn aggregate_samples_binned(
    samples: &[Sample],
    agg: Aggregation,
    start: Timestamp,
    bucket_width: Timestamp,
    bucket_count: usize,
) -> crate::Result<Vec<AggregateResult>> {
    let mut buckets: Vec<Vec<f64>> = vec![Vec::new(); bucket_count];
    for sample in samples {
        if sample.timestamp < start || bucket_width == 0 {
            continue;
        }
        let idx = ((sample.timestamp - start) / bucket_width) as usize;
        if idx < bucket_count && let Value::F64(v) = sample.value {
            buckets[idx].push(v);
        }
    }
    buckets
        .into_iter()
        .map(|values| {
            if values.is_empty() {
                return Ok(AggregateResult::Empty);
            }
            match agg {
                Aggregation::Count => Ok(AggregateResult::Scalar(values.len() as f64)),
                Aggregation::Sum => Ok(AggregateResult::Scalar(values.iter().sum())),
                Aggregation::Avg => Ok(AggregateResult::Scalar(
                    values.iter().sum::<f64>() / values.len() as f64,
                )),
                Aggregation::Min => Ok(AggregateResult::Scalar(
                    values.iter().copied().fold(f64::INFINITY, f64::min),
                )),
                Aggregation::Max => Ok(AggregateResult::Scalar(
                    values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                )),
                Aggregation::Rate => {
                    if values.len() < 2 {
                        Ok(AggregateResult::Empty)
                    } else {
                        let first = values.first().copied().unwrap_or(0.0);
                        let last = values.last().copied().unwrap_or(0.0);
                        Ok(AggregateResult::Scalar(last - first))
                    }
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples() -> Vec<Sample> {
        (0..10u64)
            .map(|i| Sample {
                timestamp: i,
                value: Value::F64(i as f64),
            })
            .collect()
    }

    #[test]
    fn sum_count_avg_min_max() {
        let s = samples();
        assert_eq!(
            aggregate_samples(&s, Aggregation::Sum).unwrap(),
            AggregateResult::Scalar(45.0)
        );
        assert_eq!(
            aggregate_samples(&s, Aggregation::Count).unwrap(),
            AggregateResult::Scalar(10.0)
        );
        assert_eq!(
            aggregate_samples(&s, Aggregation::Avg).unwrap(),
            AggregateResult::Scalar(4.5)
        );
        assert_eq!(
            aggregate_samples(&s, Aggregation::Min).unwrap(),
            AggregateResult::Scalar(0.0)
        );
        assert_eq!(
            aggregate_samples(&s, Aggregation::Max).unwrap(),
            AggregateResult::Scalar(9.0)
        );
    }

    #[test]
    fn binned_aggregation() {
        let s = samples();
        let buckets = aggregate_samples_binned(&s, Aggregation::Sum, 0, 3, 4).unwrap();
        assert_eq!(buckets[0], AggregateResult::Scalar(3.0)); // 0+1+2
        assert_eq!(buckets[1], AggregateResult::Scalar(12.0)); // 3+4+5
        assert_eq!(buckets[2], AggregateResult::Scalar(21.0)); // 6+7+8
        assert_eq!(buckets[3], AggregateResult::Scalar(9.0)); // 9
    }
}

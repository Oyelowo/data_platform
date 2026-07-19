//! Predicate evaluation against column statistics and row values.

use std::collections::HashMap;

use bytes::Bytes;
use storage_traits::Predicate;

use crate::Result;
use crate::manifest::{ColumnStats, StatsValue};
use crate::schema::TableSchema;
use crate::types::ColumnType;

/// Decide whether a file can possibly contain rows matching the predicate using
/// only file-level column statistics.
///
/// Returns `true` if the file should be read (statistics are inconclusive or
/// the predicate may match).
pub fn prune_file_by_stats(
    predicate: &Predicate,
    stats: &HashMap<String, ColumnStats>,
    schema: &TableSchema,
) -> bool {
    match predicate {
        Predicate::True => true,
        Predicate::Eq { column, value } => {
            let Some(col_stats) = stats.get(column) else {
                return true;
            };
            if col_stats.min.is_unknown() || col_stats.max.is_unknown() {
                return true;
            }
            let Some(def) = schema.column(column) else {
                return true;
            };
            let Ok(query) = StatsValue::from_bytes(value, def.ty) else {
                return true;
            };
            match compare_stats_value(&col_stats.min, &query) {
                Some(std::cmp::Ordering::Greater) => false,
                _ => !matches!(
                    compare_stats_value(&query, &col_stats.max),
                    Some(std::cmp::Ordering::Greater)
                ),
            }
        }
        Predicate::Range {
            column,
            lower,
            upper,
            ..
        } => {
            let Some(col_stats) = stats.get(column) else {
                return true;
            };
            if col_stats.min.is_unknown() || col_stats.max.is_unknown() {
                return true;
            }
            let Some(def) = schema.column(column) else {
                return true;
            };

            let lower_overlaps = match lower {
                Some(lo) => {
                    let Ok(lo) = StatsValue::from_bytes(lo, def.ty) else {
                        return true;
                    };
                    !matches!(
                        compare_stats_value(&lo, &col_stats.max),
                        Some(std::cmp::Ordering::Greater)
                    )
                }
                None => true,
            };

            let upper_overlaps = match upper {
                Some(hi) => {
                    let Ok(hi) = StatsValue::from_bytes(hi, def.ty) else {
                        return true;
                    };
                    !matches!(
                        compare_stats_value(&col_stats.min, &hi),
                        Some(std::cmp::Ordering::Greater)
                    )
                }
                None => true,
            };

            lower_overlaps && upper_overlaps
        }
        Predicate::And(children) => children
            .iter()
            .all(|c| prune_file_by_stats(c, stats, schema)),
        Predicate::Or(children) => children
            .iter()
            .any(|c| prune_file_by_stats(c, stats, schema)),
    }
}

/// Compare two `StatsValue`s, returning `None` if their types are incompatible.
fn compare_stats_value(left: &StatsValue, right: &StatsValue) -> Option<std::cmp::Ordering> {
    left.partial_cmp(right)
}

/// Evaluate a predicate against a single row given as a map from column name to
/// optional byte value.
///
/// Missing columns (e.g. due to schema evolution) are treated as null and do
/// not match any comparison predicate.
pub fn eval_row(
    predicate: &Predicate,
    schema: &TableSchema,
    row: &HashMap<String, Option<Bytes>>,
) -> Result<bool> {
    match predicate {
        Predicate::True => Ok(true),
        Predicate::Eq { column, value } => {
            let Some(def) = schema.column(column) else {
                return Ok(false);
            };
            let Some(Some(actual)) = row.get(column) else {
                return Ok(false);
            };
            Ok(values_equal(actual, value, def.ty))
        }
        Predicate::Range {
            column,
            lower,
            lower_inclusive,
            upper,
            upper_inclusive,
        } => {
            let Some(def) = schema.column(column) else {
                return Ok(false);
            };
            let Some(Some(actual)) = row.get(column) else {
                return Ok(false);
            };

            if let Some(lo) = lower {
                let ord = compare_values(actual, lo, def.ty)?;
                let ok = if *lower_inclusive {
                    ord != std::cmp::Ordering::Less
                } else {
                    ord == std::cmp::Ordering::Greater
                };
                if !ok {
                    return Ok(false);
                }
            }
            if let Some(hi) = upper {
                let ord = compare_values(actual, hi, def.ty)?;
                let ok = if *upper_inclusive {
                    ord != std::cmp::Ordering::Greater
                } else {
                    ord == std::cmp::Ordering::Less
                };
                if !ok {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        Predicate::And(children) => {
            for c in children {
                if !eval_row(c, schema, row)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        Predicate::Or(children) => {
            for c in children {
                if eval_row(c, schema, row)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
    }
}

/// Compare two values of the same logical type.
fn compare_values(left: &Bytes, right: &Bytes, ty: ColumnType) -> Result<std::cmp::Ordering> {
    match ty {
        ColumnType::Int64 | ColumnType::TimestampMicros => {
            let l = parse_i64(left)?;
            let r = parse_i64(right)?;
            Ok(l.cmp(&r))
        }
        ColumnType::Float64 => {
            let l = parse_f64(left)?;
            let r = parse_f64(right)?;
            l.partial_cmp(&r)
                .ok_or_else(|| crate::Error::Predicate("cannot compare NaN values".into()))
        }
        ColumnType::Bool | ColumnType::Utf8 | ColumnType::Binary => {
            Ok(left.as_ref().cmp(right.as_ref()))
        }
    }
}

fn values_equal(left: &Bytes, right: &Bytes, ty: ColumnType) -> bool {
    match ty {
        ColumnType::Int64 | ColumnType::TimestampMicros => {
            parse_i64(left).ok() == parse_i64(right).ok()
        }
        ColumnType::Float64 => parse_f64(left).ok() == parse_f64(right).ok(),
        ColumnType::Bool | ColumnType::Utf8 | ColumnType::Binary => left.as_ref() == right.as_ref(),
    }
}

fn parse_i64(bytes: &Bytes) -> Result<i64> {
    let s = std::str::from_utf8(bytes)
        .map_err(|e| crate::Error::Predicate(format!("invalid utf8 for i64: {e}")))?;
    s.parse::<i64>()
        .map_err(|e| crate::Error::Predicate(format!("invalid i64 '{s}': {e}")))
}

fn parse_f64(bytes: &Bytes) -> Result<f64> {
    let s = std::str::from_utf8(bytes)
        .map_err(|e| crate::Error::Predicate(format!("invalid utf8 for f64: {e}")))?;
    s.parse::<f64>()
        .map_err(|e| crate::Error::Predicate(format!("invalid f64 '{s}': {e}")))
}

//! Partition key derivation and partition-pruning helpers.

use std::collections::HashSet;

use bytes::Bytes;
use storage_traits::Predicate;

/// Sanitize a raw partition value into a safe directory name.
///
/// * Empty values become `__empty`.
/// * Null values become `__null`.
/// * `/`, `\`, and NUL are replaced with `_` to avoid directory traversal.
pub fn partition_key(value: Option<&Bytes>) -> String {
    let raw = match value {
        None => return "__null".into(),
        Some(v) if v.is_empty() => return "__empty".into(),
        Some(v) => std::str::from_utf8(v).unwrap_or("__binary"),
    };

    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '/' | '\\' | '\0' => out.push('_'),
            _ => out.push(ch),
        }
    }
    out
}

/// Decide whether a partition directory can possibly contain rows matching the
/// predicate, given that the table is partitioned on `partition_column`.
///
/// Returns `true` if the partition should be read (inconclusive or possibly
/// matches). Returns `false` only when the predicate definitively excludes the
/// partition value.
pub fn partition_prune(
    partition_column: &str,
    partition_value: &str,
    predicate: &Predicate,
) -> bool {
    match predicate {
        Predicate::True => true,
        Predicate::Eq { column, value } => {
            if column != partition_column {
                return true;
            }
            let value_str = String::from_utf8_lossy(value);
            partition_value == value_str.as_ref()
        }
        Predicate::Range {
            column,
            lower,
            upper,
            ..
        } => {
            if column != partition_column {
                return true;
            }
            if let Some(lo) = lower {
                let lo_str = String::from_utf8_lossy(lo);
                if partition_value < lo_str.as_ref() {
                    return false;
                }
            }
            if let Some(hi) = upper {
                let hi_str = String::from_utf8_lossy(hi);
                if partition_value > hi_str.as_ref() {
                    return false;
                }
            }
            true
        }
        Predicate::And(children) => children
            .iter()
            .all(|c| partition_prune(partition_column, partition_value, c)),
        Predicate::Or(children) => children
            .iter()
            .any(|c| partition_prune(partition_column, partition_value, c)),
    }
}

/// Collect the set of partition directory names mentioned in a predicate for
/// the configured partition column, if any.
pub fn collect_partition_literals(
    predicate: &Predicate,
    partition_column: &str,
) -> HashSet<String> {
    let mut out = HashSet::new();
    collect(predicate, partition_column, &mut out);
    out
}

fn collect(predicate: &Predicate, partition_column: &str, out: &mut HashSet<String>) {
    match predicate {
        Predicate::Eq { column, value } if column == partition_column => {
            out.insert(String::from_utf8_lossy(value).into_owned());
        }
        Predicate::Range { .. } | Predicate::Eq { .. } => {}
        Predicate::And(children) | Predicate::Or(children) => {
            for c in children {
                collect(c, partition_column, out);
            }
        }
        Predicate::True => {}
    }
}

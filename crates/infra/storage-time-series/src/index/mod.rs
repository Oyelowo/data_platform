//! Label index for tag-based series discovery.

pub mod posting;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::format::parse_series_key;
use crate::query::TagFilter;

/// Inverted label index.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LabelIndex {
    /// tag_key → tag_value → set<series_key>
    inverted: BTreeMap<String, BTreeMap<String, BTreeSet<Vec<u8>>>>,
    /// series_key → metric + sorted tags
    forward: HashMap<Vec<u8>, SeriesTags>,
}

/// Tags associated with a series.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SeriesTags {
    /// Metric name.
    pub metric: Vec<u8>,
    /// Sorted tag pairs.
    pub tags: Vec<(String, String)>,
}

impl LabelIndex {
    /// Create an empty label index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Index a series key.
    pub fn insert(&mut self, series_key: Vec<u8>) -> crate::Result<()> {
        if self.forward.contains_key(&series_key) {
            return Ok(());
        }
        let (metric, tags) = parse_series_key(&series_key)?;
        for (key, value) in &tags {
            self.inverted
                .entry(key.clone())
                .or_default()
                .entry(value.clone())
                .or_default()
                .insert(series_key.clone());
        }
        self.forward.insert(
            series_key,
            SeriesTags {
                metric,
                tags,
            },
        );
        Ok(())
    }

    /// Remove a series from the index.
    pub fn remove(&mut self, series_key: &[u8]) {
        if let Some(info) = self.forward.remove(series_key) {
            for (key, value) in &info.tags {
                if let Some(values) = self.inverted.get_mut(key) {
                    if let Some(set) = values.get_mut(value) {
                        set.remove(series_key);
                        if set.is_empty() {
                            values.remove(value);
                        }
                    }
                    if values.is_empty() {
                        self.inverted.remove(key);
                    }
                }
            }
        }
    }

    /// Return all known series keys.
    pub fn series(&self) -> impl Iterator<Item = &Vec<u8>> {
        self.forward.keys()
    }

    /// Return series keys matching a metric and filters.
    pub fn match_series<'a>(
        &'a self,
        metric: &[u8],
        filters: &'a [TagFilter],
    ) -> crate::Result<Vec<Vec<u8>>> {
        let mut candidates: Option<BTreeSet<Vec<u8>>> = None;

        for filter in filters {
            match filter {
                TagFilter::Eq { key, value } => {
                    let set = self
                        .inverted
                        .get(key)
                        .and_then(|m| m.get(value))
                        .cloned()
                        .unwrap_or_default();
                    candidates = Some(intersect_or_new(candidates, set));
                }
                TagFilter::Neq { key, value } => {
                    let set: BTreeSet<Vec<u8>> = self
                        .inverted
                        .get(key)
                        .map(|m| {
                            m.iter()
                                .filter(|(k, _)| k.as_str() != value.as_str())
                                .flat_map(|(_, set)| set.iter().cloned())
                                .collect()
                        })
                        .unwrap_or_default();
                    candidates = Some(intersect_or_new(candidates, set));
                }
                TagFilter::HasKey { key } => {
                    let set: BTreeSet<Vec<u8>> = self
                        .inverted
                        .get(key)
                        .map(|m| m.values().flat_map(|set| set.iter().cloned()).collect())
                        .unwrap_or_default();
                    candidates = Some(intersect_or_new(candidates, set));
                }
            }
        }

        let metric_set: BTreeSet<Vec<u8>> = self
            .forward
            .iter()
            .filter(|(_, info)| info.metric == metric)
            .map(|(k, _)| k.clone())
            .collect();

        let mut result = intersect_or_new(candidates, metric_set);
        result.retain(|k| self.forward.contains_key(k));
        Ok(result.into_iter().collect())
    }

    /// Return tags for a series key.
    pub fn tags(&self, series_key: &[u8]) -> Option<&SeriesTags> {
        self.forward.get(series_key)
    }
}

fn intersect_or_new(
    acc: Option<BTreeSet<Vec<u8>>>,
    other: BTreeSet<Vec<u8>>,
) -> BTreeSet<Vec<u8>> {
    match acc {
        None => other,
        Some(mut acc) => {
            if acc.is_empty() {
                other
            } else {
                acc.retain(|k| other.contains(k));
                acc
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::build_series_key;

    #[test]
    fn insert_and_match() {
        let mut index = LabelIndex::new();
        let key = build_series_key(b"cpu", &[("host".to_string(), "db1".to_string())]);
        index.insert(key.clone()).unwrap();
        let matched = index.match_series(b"cpu", &[TagFilter::Eq {
            key: "host".into(),
            value: "db1".into(),
        }]).unwrap();
        assert_eq!(matched, vec![key]);
    }

    #[test]
    fn multiple_filters_intersect() {
        let mut index = LabelIndex::new();
        let key1 = build_series_key(
            b"cpu",
            &[
                ("host".to_string(), "db1".to_string()),
                ("region".to_string(), "us-east".to_string()),
            ],
        );
        let key2 = build_series_key(
            b"cpu",
            &[
                ("host".to_string(), "db2".to_string()),
                ("region".to_string(), "us-east".to_string()),
            ],
        );
        index.insert(key1.clone()).unwrap();
        index.insert(key2.clone()).unwrap();
        let matched = index
            .match_series(
                b"cpu",
                &[
                    TagFilter::Eq {
                        key: "region".into(),
                        value: "us-east".into(),
                    },
                    TagFilter::Eq {
                        key: "host".into(),
                        value: "db1".into(),
                    },
                ],
            )
            .unwrap();
        assert_eq!(matched, vec![key1]);
    }

    #[test]
    fn neq_filter() {
        let mut index = LabelIndex::new();
        let key1 = build_series_key(b"cpu", &[("host".to_string(), "db1".to_string())]);
        let key2 = build_series_key(b"cpu", &[("host".to_string(), "db2".to_string())]);
        index.insert(key1.clone()).unwrap();
        index.insert(key2.clone()).unwrap();
        let matched = index.match_series(b"cpu", &[TagFilter::Neq {
            key: "host".into(),
            value: "db1".into(),
        }]).unwrap();
        assert_eq!(matched, vec![key2]);
    }

    #[test]
    fn remove_series() {
        let mut index = LabelIndex::new();
        let key = build_series_key(b"cpu", &[("host".to_string(), "db1".to_string())]);
        index.insert(key.clone()).unwrap();
        index.remove(&key);
        assert!(index.match_series(b"cpu", &[]).unwrap().is_empty());
    }
}

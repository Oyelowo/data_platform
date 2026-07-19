//! Bulk-load the R-tree from a set of live features.

use rstar::AABB;

use crate::feature::Feature;
use crate::index::rtree::{IndexedFeature, SpatialIndex};
use crate::store::FeatureAddress;

/// Helper for constructing a spatial index from a snapshot of features.
pub struct IndexBuilder;

impl IndexBuilder {
    /// Build a spatial index from an iterator of `(feature, address)` pairs.
    pub fn build<'a>(
        features: impl IntoIterator<Item = (&'a Feature, FeatureAddress)>,
    ) -> SpatialIndex {
        let mut entries = Vec::new();
        for (feature, address) in features {
            if let Some((min_lon, min_lat, max_lon, max_lat)) = feature.geometry.envelope() {
                entries.push(IndexedFeature {
                    envelope: AABB::from_corners([min_lon, min_lat], [max_lon, max_lat]),
                    id: feature.id.clone(),
                    address,
                });
            }
        }
        SpatialIndex::bulk_load(entries)
    }
}

/// Build an [`IndexedFeature`] for a single feature.
pub fn make_entry(feature: &Feature, address: FeatureAddress) -> Option<IndexedFeature> {
    let (min_lon, min_lat, max_lon, max_lat) = feature.geometry.envelope()?;
    Some(IndexedFeature {
        envelope: AABB::from_corners([min_lon, min_lat], [max_lon, max_lat]),
        id: feature.id.clone(),
        address,
    })
}

//! R-tree spatial index.

pub mod builder;
pub mod rtree;

pub use builder::IndexBuilder;
pub use rtree::{IndexedFeature, SpatialIndex};

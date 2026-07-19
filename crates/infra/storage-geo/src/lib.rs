//! Durable geospatial storage engine.
//!
//! `storage-geo` provides an embeddable, crash-safe geospatial database with:
//!
//! * WGS84 geometry storage (Point, LineString, Polygon, MultiPoint,
//!   MultiLineString, MultiPolygon, GeometryCollection).
//! * R-tree spatial indexing using `rstar`.
//! * Spatial queries: BBox, Intersects, Contains, Within, DWithin, Nearest.
//! * Haversine distance for WGS84 radius and nearest-neighbor refinement.
//! * WAL-backed durability and recovery.
//! * A `storage_traits::Engine` implementation for byte-key / encoded-feature
//!   access.
//!
//! # Example
//!
//! ```rust,no_run
//! use storage_geo::{GeoEngine, GeoOptions, Geometry, PropertyMap};
//! use geo::Point;
//!
//! let dir = tempfile::tempdir().unwrap();
//! let engine = GeoEngine::open(dir.path(), GeoOptions::default()).unwrap();
//! engine.insert_feature(b"paris", Geometry::Point(Point::new(2.35, 48.85)), PropertyMap::new()).unwrap();
//! let results = engine.query(&storage_geo::SpatialQuery::Nearest {
//!     point: Point::new(2.35, 48.85),
//!     k: 10,
//! }).unwrap();
//! ```

#![warn(missing_docs)]
#![deny(clippy::unwrap_used)]
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod compaction;
pub mod cursor;
pub mod engine;
pub mod error;
pub mod feature;
pub mod format;
pub mod index;
pub mod options;
pub mod query;
pub mod recovery;
pub mod stats;
pub mod store;
pub mod transaction;
pub mod wal;
pub mod wkb;

pub use engine::GeoEngine;
pub use error::{Error, Result};
pub use feature::{Feature, Geometry, PropertyMap};
pub use transaction::GeoTransaction;
pub use options::{GeoOptions, GeometryKind, WalSyncPolicy};
pub use query::{SpatialQuery};
pub use stats::GeoStats;
pub use store::FeatureAddress;

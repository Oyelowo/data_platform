//! Configuration options for the vector engine.

use serde::{Deserialize, Serialize};

use crate::distance::DistanceMetric;

/// Top-level options for [`VectorEngine`](crate::VectorEngine).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VectorOptions {
    /// Vector dimension. All vectors stored in the engine must match this.
    pub dimension: usize,

    /// Distance metric used for search.
    pub metric: DistanceMetric,

 /// Index type used for approximate nearest-neighbor search.
    pub index_type: IndexType,

    /// HNSW-specific options, used when `index_type` is [`IndexType::Hnsw`].
    pub hnsw: HnswOptions,

    /// IVF-specific options, used when `index_type` is [`IndexType::Ivf`].
    pub ivf: IvfOptions,

    /// Quantization strategy for in-memory vector storage.
    pub quantization: Quantization,

    /// For indexes other than BruteForce, fall back to an exact scan when the
    /// dataset has at most this many vectors. Zero disables the fallback.
    pub brute_force_threshold: usize,

    /// Page file size limit for vector storage in bytes.
    pub vector_page_size: usize,

    /// Maximum key length in bytes.
    pub max_key_len: usize,

    /// Maximum vector page file count before forcing a compaction.
    pub max_vector_pages: usize,
}

impl VectorOptions {
    /// Validate options and return an error if they are unusable.
    pub fn validate(&self) -> crate::Result<()> {
        if self.dimension == 0 {
            return Err(crate::Error::InvalidArgument(
                "dimension must be greater than zero".into(),
            ));
        }
        if self.max_key_len == 0 {
            return Err(crate::Error::InvalidArgument(
                "max_key_len must be greater than zero".into(),
            ));
        }
        if self.vector_page_size < 4096 {
            return Err(crate::Error::InvalidArgument(
                "vector_page_size must be at least 4096 bytes".into(),
            ));
        }
        self.hnsw.validate()?;
        self.ivf.validate()?;
        Ok(())
    }

    /// Return options for a brute-force index.
    pub fn brute_force(dimension: usize, metric: DistanceMetric) -> Self {
        Self {
            dimension,
            metric,
            index_type: IndexType::BruteForce,
            hnsw: HnswOptions::default(),
            ivf: IvfOptions::default(),
            quantization: Quantization::None,
            brute_force_threshold: 10_000,
            vector_page_size: 16 * 1024 * 1024,
            max_key_len: 4096,
            max_vector_pages: 64,
        }
    }
}

impl Default for VectorOptions {
    fn default() -> Self {
        Self {
            dimension: 128,
            metric: DistanceMetric::Euclidean,
            index_type: IndexType::Hnsw,
            hnsw: HnswOptions::default(),
            ivf: IvfOptions::default(),
            quantization: Quantization::None,
            brute_force_threshold: 10_000,
            vector_page_size: 16 * 1024 * 1024,
            max_key_len: 4096,
            max_vector_pages: 64,
        }
    }
}

/// ANN index type.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexType {
    /// Exact brute-force scan.
    BruteForce,
    /// Hierarchical Navigable Small World graph.
    #[default]
    Hnsw,
    /// Inverted file index with k-means centroids.
    Ivf,
}

/// HNSW construction and search parameters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HnswOptions {
    /// Maximum number of neighbors per node (default 16).
    pub m: usize,
    /// Size of the dynamic candidate list during construction (default 200).
    pub ef_construction: usize,
    /// Size of the dynamic candidate list during search (default 64).
    pub ef_search: usize,
    /// Probability decay factor for layer selection; `1 / ln(M)` is typical.
    pub level_multiplier: f64,
    /// Maximum number of layers. Zero means unlimited.
    pub max_level: usize,
    /// RNG seed for deterministic construction. Zero means non-deterministic
    /// (`thread_rng`).
    pub seed: u64,
}

impl HnswOptions {
    /// Validate HNSW options.
    pub fn validate(&self) -> crate::Result<()> {
        if self.m < 2 {
            return Err(crate::Error::InvalidArgument("HNSW m must be >= 2".into()));
        }
        if self.ef_construction < self.m {
            return Err(crate::Error::InvalidArgument(
                "HNSW ef_construction must be >= m".into(),
            ));
        }
        if self.ef_search < 1 {
            return Err(crate::Error::InvalidArgument(
                "HNSW ef_search must be >= 1".into(),
            ));
        }
        if self.level_multiplier <= 0.0 {
            return Err(crate::Error::InvalidArgument(
                "HNSW level_multiplier must be positive".into(),
            ));
        }
        Ok(())
    }
}

impl Default for HnswOptions {
    fn default() -> Self {
        Self {
            m: 16,
            ef_construction: 200,
            ef_search: 64,
            level_multiplier: 1.0 / std::f64::consts::LN_2,
            max_level: 16,
            seed: 0,
        }
    }
}

/// IVF construction and search parameters.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct IvfOptions {
    /// Number of clusters / centroids.
    pub n_clusters: usize,
    /// Number of clusters to probe during search.
    pub n_probe: usize,
    /// Maximum k-means iterations during training.
    pub max_iters: usize,
}

impl IvfOptions {
    /// Validate IVF options.
    pub fn validate(&self) -> crate::Result<()> {
        if self.n_clusters == 0 {
            return Err(crate::Error::InvalidArgument(
                "IVF n_clusters must be > 0".into(),
            ));
        }
        if self.n_probe == 0 || self.n_probe > self.n_clusters {
            return Err(crate::Error::InvalidArgument(
                "IVF n_probe must be in [1, n_clusters]".into(),
            ));
        }
        if self.max_iters == 0 {
            return Err(crate::Error::InvalidArgument(
                "IVF max_iters must be > 0".into(),
            ));
        }
        Ok(())
    }
}

impl Default for IvfOptions {
    fn default() -> Self {
        Self {
            n_clusters: 256,
            n_probe: 16,
            max_iters: 25,
        }
    }
}

/// Quantization strategy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quantization {
    /// No quantization: store raw `f32` vectors in memory.
    #[default]
    None,
    /// 8-bit scalar quantization per dimension.
    Scalar,
}

//! Centralized Queryable method dispatch.
//!
//! Maps method names to a type-safe enum, eliminating scattered string
//! matching throughout the extraction code. The extraction first verifies
//! `trait_def_id == LangItem::Queryable`, then uses this enum to dispatch.

/// Known `Queryable` trait methods and their plan-level semantics.
///
/// Each variant maps to a specific plan operator construction in the
/// extraction phase. Unknown methods (not in this enum) become
/// `Plan::Extension` barriers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryableMethod {
    // ── Filtering / mapping ────────────────────────────────────────────
    Filter,
    Map,
    FlatMap,

    // ── Joins ──────────────────────────────────────────────────────────
    Join,
    InnerJoin,
    LeftJoin,
    SemiJoin,
    AntiJoin,

    // ── Grouping / aggregation ─────────────────────────────────────────
    GroupBy,
    Aggregate,

    // ── Ordering ───────────────────────────────────────────────────────
    OrderBy,
    SortBy,
    OrderByDesc,
    SortByDesc,

    // ── Deduplication ──────────────────────────────────────────────────
    Distinct,
    DistinctBy,
    Unique,
    UniqueBy,

    // ── Slicing ────────────────────────────────────────────────────────
    Take,
    Skip,

    // ── Scalar aggregates ──────────────────────────────────────────────
    Sum,
    Count,
    Avg,
    Min,
    Max,

    // ── Set operations ─────────────────────────────────────────────────
    Union,
    UnionAll,
}

impl QueryableMethod {
    /// Resolve a method name to a [`QueryableMethod`].
    ///
    /// Returns `None` for unrecognized methods (which become Extension barriers).
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "filter" => Self::Filter,
            "map" => Self::Map,
            "flat_map" => Self::FlatMap,

            "join" => Self::Join,
            "inner_join" => Self::InnerJoin,
            "left_join" => Self::LeftJoin,
            "semi_join" => Self::SemiJoin,
            "anti_join" => Self::AntiJoin,

            "group_by" => Self::GroupBy,
            "aggregate" => Self::Aggregate,

            "order_by" => Self::OrderBy,
            "sort_by" => Self::SortBy,
            "order_by_desc" => Self::OrderByDesc,
            "sort_by_desc" => Self::SortByDesc,

            "distinct" => Self::Distinct,
            "distinct_by" => Self::DistinctBy,
            "unique" => Self::Unique,
            "unique_by" => Self::UniqueBy,

            "take" => Self::Take,
            "skip" => Self::Skip,

            "sum" => Self::Sum,
            "count" => Self::Count,
            "avg" => Self::Avg,
            "min" => Self::Min,
            "max" => Self::Max,

            "union" => Self::Union,
            "union_all" => Self::UnionAll,

            _ => return None,
        })
    }

    /// Whether this method is a scalar aggregate (produces a single value).
    pub fn is_scalar_aggregate(self) -> bool {
        matches!(
            self,
            Self::Sum | Self::Count | Self::Avg | Self::Min | Self::Max
        )
    }

    /// Whether this method is a join variant.
    pub fn is_join(self) -> bool {
        matches!(
            self,
            Self::Join | Self::InnerJoin | Self::LeftJoin | Self::SemiJoin | Self::AntiJoin
        )
    }
}

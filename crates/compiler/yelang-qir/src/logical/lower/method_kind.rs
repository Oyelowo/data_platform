//! Centralized Queryable method dispatch.
//!
//! Dispatch priority:
//! 1. **DefId-based** (preferred): resolve method DefId → `LangItem` → `QueryableMethod`
//!    via `from_lang_item()`. Requires `@lang("queryable_*")` on stdlib methods.
//! 2. **Name-based** (fallback): `from_name()` for methods without lang items yet.
//!
//! The lowering first verifies `trait_def_id == LangItem::Queryable`, then
//! dispatches via lang item when available, falling back to name matching.

use yelang_resolve::lang_items::LangItem;

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

    // ── Eager evaluation ───────────────────────────────────────────────
    Fold,
    Reduce,
    Execute,
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

    /// Resolve from a `LangItem` (DefId-based dispatch).
    ///
    /// Preferred over `from_name()` — uses the `@lang("queryable_*")`
    /// annotation on the method's DefId instead of matching name strings.
    pub fn from_lang_item(item: LangItem) -> Option<Self> {
        Some(match item {
            LangItem::QueryableMap => Self::Map,
            LangItem::QueryableFilter => Self::Filter,
            LangItem::QueryableFlatMap => Self::FlatMap,
            LangItem::QueryableOrderBy => Self::OrderBy,
            LangItem::QueryableGroupBy => Self::GroupBy,
            LangItem::QueryableDistinct => Self::Distinct,
            LangItem::QueryableTake => Self::Take,
            LangItem::QueryableSkip => Self::Skip,
            LangItem::QueryableAggregate => Self::Aggregate,
            LangItem::QueryableSum => Self::Sum,
            LangItem::QueryableCount => Self::Count,
            LangItem::QueryableAvg => Self::Avg,
            LangItem::QueryableMin => Self::Min,
            LangItem::QueryableMax => Self::Max,
            LangItem::QueryableFold => Self::Fold,
            LangItem::QueryableReduce => Self::Reduce,
            LangItem::QueryableExecute => Self::Execute,
            LangItem::QueryableJoin => Self::Join,
            LangItem::QueryableLeftJoin => Self::LeftJoin,
            LangItem::QueryableSemiJoin => Self::SemiJoin,
            LangItem::QueryableAntiJoin => Self::AntiJoin,
            LangItem::QueryableUnion => Self::Union,
            _ => return None,
        })
    }
}

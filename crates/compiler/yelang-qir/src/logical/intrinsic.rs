//! Compiler-recognized `@intrinsic` names used by `Queryable` method bodies.

/// Intrinsic names that the extractor maps directly to LIR operators.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum QueryableIntrinsic {
    Map,
    Filter,
    FlatMap,
    OrderBy,
    GroupBy,
    Distinct,
    Take,
    Skip,
    Aggregate,
    Fold,
    Reduce,
    Execute,
}

impl QueryableIntrinsic {
    /// Parse an intrinsic name of the form `query_*`.
    pub fn from_symbol(sym: yelang_interner::Symbol, interner: &yelang_interner::Interner) -> Option<Self> {
        let s = interner.resolve(&sym);
        Some(match s {
            "query_map" => Self::Map,
            "query_filter" => Self::Filter,
            "query_flat_map" => Self::FlatMap,
            "query_order_by" => Self::OrderBy,
            "query_group_by" => Self::GroupBy,
            "query_distinct" => Self::Distinct,
            "query_take" => Self::Take,
            "query_skip" => Self::Skip,
            "query_aggregate" => Self::Aggregate,
            "query_fold" => Self::Fold,
            "query_reduce" => Self::Reduce,
            "query_execute" => Self::Execute,
            _ => return None,
        })
    }
}

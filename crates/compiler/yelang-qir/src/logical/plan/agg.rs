//! [`AggCall`], [`AggKind`], [`AggProperties`], [`AggClass`].

use yelang_arena::DefId;
use yelang_interner::Symbol;

use super::ExprRef;

/// A single aggregate computation inside an `Aggregate` or `GroupJoin`.
#[derive(Debug, Clone)]
pub struct AggCall {
    /// The output column name for this aggregate's result.
    pub output: Symbol,
    /// What kind of aggregate this is.
    pub kind: AggKind,
}

/// The three tiers of aggregate recognition.
///
/// Every variant carries [`AggProperties`] so the physical planner can
/// make optimization decisions based on declared algebraic properties
/// rather than guessing from method names.
#[derive(Debug, Clone)]
pub enum AggKind {
    // ── Tier 1: compiler-known (full optimization) ─────────────────────
    Count,
    Sum { expr: ExprRef },
    Avg { expr: ExprRef },
    Min { expr: ExprRef },
    Max { expr: ExprRef },

    // ── Tier 2: user-defined via `Aggregate` trait ─────────────────────
    /// The compiler reads the `AggregateProperties` declared by the trait
    /// impl and uses them for optimization decisions:
    /// - `associative` → parallel partial aggregation across shards
    /// - `commutative` → reorder inputs freely
    /// - `class == Algebraic` → decompose into Distributive components
    /// - `class == Holistic` → must gather all data to one node
    /// - `invertible` → sliding window incremental updates
    UserAggregate {
        /// The `DefId` of the `Aggregate` trait impl.
        impl_def: DefId,
        /// Constructor arguments (e.g. `Percentile { p: 0.99 }`).
        args: Vec<ExprRef>,
        /// The expression being aggregated (input to `step`).
        input_expr: Option<ExprRef>,
        /// Algebraic properties declared by the impl's `properties()` method.
        properties: AggProperties,
    },

    // ── Tier 3: fully opaque (no optimization through it) ──────────────
    /// A method the compiler doesn't recognize as an aggregate.
    /// Treated as a black box — no parallelization, no pushdown.
    Opaque { call: ExprRef },
}

impl AggKind {
    /// Get the algebraic properties for this aggregate.
    ///
    /// Compiler-known aggregates return hardcoded properties.
    /// User-defined aggregates return the properties declared by the impl.
    /// Opaque aggregates return the most conservative properties.
    pub fn properties(&self) -> AggProperties {
        match self {
            AggKind::Count => AggProperties {
                class: AggClass::Distributive,
                associative: true,
                commutative: true,
                invertible: true,
            },
            AggKind::Sum { .. } => AggProperties {
                class: AggClass::Distributive,
                associative: true,
                commutative: true,
                invertible: true,
            },
            AggKind::Avg { .. } => AggProperties {
                class: AggClass::Algebraic,
                associative: true,
                commutative: true,
                invertible: false,
            },
            AggKind::Min { .. } | AggKind::Max { .. } => AggProperties {
                class: AggClass::Distributive,
                associative: true,
                commutative: true,
                invertible: false,
            },
            AggKind::UserAggregate { properties, .. } => *properties,
            AggKind::Opaque { .. } => AggProperties {
                class: AggClass::Holistic,
                associative: false,
                commutative: false,
                invertible: false,
            },
        }
    }

    /// Whether this aggregate can be parallelized across shards.
    ///
    /// Requires associativity and non-holistic classification.
    pub fn is_parallelizable(&self) -> bool {
        let props = self.properties();
        props.associative && props.class != AggClass::Holistic
    }

    /// Whether this aggregate can be decomposed into simpler aggregates.
    ///
    /// Algebraic aggregates (e.g., Avg = Sum/Count) can be decomposed
    /// into Distributive components that are computed separately and
    /// combined at finish time.
    pub fn is_decomposable(&self) -> bool {
        self.properties().class == AggClass::Algebraic
    }

    /// Whether this aggregate supports sliding window incremental updates.
    pub fn supports_sliding_window(&self) -> bool {
        self.properties().invertible
    }
}

/// Algebraic properties of an aggregate, declared by the `Aggregate` trait
/// impl's `properties()` method.
///
/// The compiler uses these to make reliable optimization decisions:
///
/// | Property       | Optimization enabled                              |
/// |----------------|---------------------------------------------------|
/// | `associative`  | Parallel partial agg: shard1 + shard2 → merge     |
/// | `commutative`  | Reorder inputs freely (join reorder safe)         |
/// | `Distributive` | Push below joins, compute partial before join     |
/// | `Algebraic`    | Decompose: AVG → SUM/COUNT, combine at finish     |
/// | `Holistic`     | Must gather all data to one node (Exchange::Gather)|
/// | `invertible`   | Sliding windows: subtract old, add new            |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AggProperties {
    /// Classification: how the aggregate relates to its inputs.
    pub class: AggClass,
    /// `merge(merge(a,b),c) == merge(a,merge(b,c))`
    /// Required for parallel partial aggregation across shards.
    pub associative: bool,
    /// `merge(a,b) == merge(b,a)`
    /// Allows reordering inputs without changing the result.
    pub commutative: bool,
    /// Can undo a step: enables sliding windows via incremental update.
    pub invertible: bool,
}

/// Aggregate classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggClass {
    /// Computable from partial results. Can be split across shards.
    /// Examples: Sum, Count, Min, Max, Product.
    Distributive,
    /// Computable from fixed-size intermediate state.
    /// Can be decomposed into Distributive components.
    /// Example: Avg = Sum / Count.
    Algebraic,
    /// Requires all data. Cannot be parallelized.
    /// Must gather to a single node before computing.
    /// Examples: Median, Percentile, Mode.
    Holistic,
}

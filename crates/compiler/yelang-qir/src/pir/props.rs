//! Physical properties required/satisfied by PIR operators.

/// Ordering requirement/satisfaction.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct PhysicalOrdering {
    pub keys: Vec<crate::expr::OrderKey>,
}

/// Partitioning requirement/satisfaction.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Partitioning {
    /// Any partitioning is acceptable.
    #[default]
    Any,
    /// Data is not partitioned (single node).
    Singleton,
    /// Hash-partitioned by expressions.
    Hash(Vec<crate::ids::QExprId>),
    /// Range-partitioned by sorted ranges.
    Range(Vec<crate::expr::OrderKey>),
    /// Replicated on every node.
    Replicated,
}

/// Where an operator runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Location {
    /// Any location.
    #[default]
    Any,
    /// Local to the coordinator / client.
    Local,
    /// Remote backend with id.
    Remote(Symbol),
}

use yelang_interner::Symbol;

/// Boundedness of a physical stream.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Boundedness {
    #[default]
    Bounded,
    Unbounded,
}

/// Physical properties of an operator.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct PhysicalProps {
    pub ordering: PhysicalOrdering,
    pub partitioning: Partitioning,
    pub location: Location,
    pub boundedness: Boundedness,
}

impl PhysicalProps {
    pub fn any() -> Self {
        Self {
            ordering: PhysicalOrdering { keys: vec![] },
            partitioning: Partitioning::Any,
            location: Location::Any,
            boundedness: Boundedness::Bounded,
        }
    }

    pub fn satisfies(&self, required: &PhysicalProps) -> bool {
        ordering_satisfies(&self.ordering, &required.ordering)
            && partitioning_satisfies(&self.partitioning, &required.partitioning)
            && (required.location == Location::Any || self.location == required.location)
            && (required.boundedness == Boundedness::Bounded || self.boundedness == Boundedness::Unbounded)
    }
}

fn ordering_satisfies(actual: &PhysicalOrdering, required: &PhysicalOrdering) -> bool {
    if required.keys.is_empty() {
        return true;
    }
    // Actual ordering must be a prefix of (or equal to) required ordering.
    actual.keys.starts_with(&required.keys)
}

pub(crate) fn partitioning_satisfies(actual: &Partitioning, required: &Partitioning) -> bool {
    match (actual, required) {
        (_, Partitioning::Any) => true,
        (Partitioning::Singleton, Partitioning::Singleton) => true,
        (Partitioning::Hash(a), Partitioning::Hash(b)) => a == b,
        (Partitioning::Range(a), Partitioning::Range(b)) => a == b,
        (Partitioning::Replicated, Partitioning::Replicated) => true,
        _ => false,
    }
}

/// Cost estimate for a physical plan fragment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cost {
    pub startup: f64,
    pub per_row: f64,
}

impl Cost {
    pub fn zero() -> Self {
        Self { startup: 0.0, per_row: 0.0 }
    }

    pub fn total(&self, rows: f64) -> f64 {
        self.startup + self.per_row * rows
    }
}

impl std::ops::Add for Cost {
    type Output = Cost;
    fn add(self, rhs: Cost) -> Cost {
        Cost {
            startup: self.startup + rhs.startup,
            per_row: self.per_row + rhs.per_row,
        }
    }
}

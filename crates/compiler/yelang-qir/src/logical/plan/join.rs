//! [`JoinKind`], [`DepJoinKind`].

/// The type of a set-algebraic join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinKind {
    Inner,
    Left,
    Right,
    Full,
    /// EXISTS / IN — keeps left rows that have at least one match.
    Semi,
    /// NOT EXISTS / NOT IN — keeps left rows that have no match.
    Anti,
    /// Cartesian product (no predicate).
    Cross,
}

/// The type of a dependent (correlated) join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepJoinKind {
    /// Regular dependent join (scalar subquery).
    Join,
    /// Dependent semi join (EXISTS).
    Semi,
    /// Dependent anti join (NOT EXISTS).
    Anti,
    /// Dependent left outer join.
    LeftOuter,
    /// Dependent single join — guarantees at most one match per outer row.
    /// Used for scalar subqueries. See [NLK17].
    Single,
}

//! Nested result shapes and correlation modes for logical QIR.

use yelang_interner::Symbol;
use yelang_ty::ty::TyId;

/// Describes how a logical operator's output is nested relative to its parent.
#[derive(Clone, Debug, PartialEq)]
pub enum NestedShape {
    /// A single scalar value.
    Scalar(TyId),
    /// A flat collection of elements.
    Collection(TyId),
    /// A collection whose elements each carry nested sub-collections.
    Nested {
        elem: TyId,
        fields: Vec<(Symbol, NestedShape)>,
    },
    /// A grouping object: key record plus a nested member collection.
    Group {
        key: TyId,
        members_label: Symbol,
        members: Box<NestedShape>,
    },
}

/// Whether an operator's result is correlated with an outer scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CorrelationMode {
    /// No correlation: the operator can be evaluated independently.
    #[default]
    Independent,
    /// The operator references outer variables and must be decorrelated
    /// or executed as a nested loop.
    Correlated,
    /// The operator is the result of a `DependentJoin` rewrite.
    Decorrelated,
}

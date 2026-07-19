//! Logical properties carried by every LIR operator.

use yelang_ty::ty::TyId;

use crate::demand::DemandSet;
use crate::ids::BinderId;
use crate::volatility::Volatility;

/// Cardinality class of an operator's output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CardinalityClass {
    Zero,
    One,
    ZeroOrOne,
    Many,
}

/// Boundedness of a data stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Boundedness {
    Bounded,
    Unbounded,
}

/// Logical properties for a LIR operator.
#[derive(Clone, Debug, PartialEq)]
pub struct LogicalProps {
    /// Output row type.
    pub output_ty: TyId,
    /// True if output is ordered (Seq semantics).
    pub ordered: bool,
    /// Bounded or unbounded.
    pub bounded: Boundedness,
    /// Cardinality class.
    pub cardinality: CardinalityClass,
    /// Known unique keys.
    pub unique_keys: Vec<Vec<BinderId>>,
    /// Columns known non-null.
    pub non_null: Vec<BinderId>,
    /// Fields consumed downstream.
    pub demand: DemandSet,
    /// Volatility of this operator.
    pub volatility: Volatility,
}

impl LogicalProps {
    /// Create a default property set.
    pub fn new(output_ty: TyId) -> Self {
        Self {
            output_ty,
            ordered: false,
            bounded: Boundedness::Bounded,
            cardinality: CardinalityClass::Many,
            unique_keys: vec![],
            non_null: vec![],
            demand: DemandSet::all(),
            volatility: Volatility::Stable,
        }
    }
}

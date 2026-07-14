/*! Type variable tables for general, int, and float variables. */

use yelang_ty::primitive::{FloatTy, IntTy};
use yelang_ty::ty::{FloatVid, IntVid, Ty, TyVid};

use crate::unify::UnificationTable;

/// Value stored for a general type variable.
#[derive(Clone, Debug, PartialEq)]
pub enum TypeVarValue<'tcx> {
    Known(Ty<'tcx>),
    Unknown,
}

impl<'tcx> Default for TypeVarValue<'tcx> {
    fn default() -> Self {
        TypeVarValue::Unknown
    }
}

/// Value stored for an integral type variable.
#[derive(Clone, Debug, PartialEq)]
pub enum IntVarValue {
    Known(IntTy),
    Unknown,
}

impl Default for IntVarValue {
    fn default() -> Self {
        IntVarValue::Unknown
    }
}

/// Value stored for a floating-point type variable.
#[derive(Clone, Debug, PartialEq)]
pub enum FloatVarValue {
    Known(FloatTy),
    Unknown,
}

impl Default for FloatVarValue {
    fn default() -> Self {
        FloatVarValue::Unknown
    }
}

/// Table of general type variables.
pub type TypeVariableTable<'tcx> = UnificationTable<TyVid, TypeVarValue<'tcx>>;

/// Table of integral type variables.
pub type IntVariableTable = UnificationTable<IntVid, IntVarValue>;

/// Table of floating-point type variables.
pub type FloatVariableTable = UnificationTable<FloatVid, FloatVarValue>;

/// Combined variable tables.
pub struct VariableTables<'tcx> {
    pub ty_vars: TypeVariableTable<'tcx>,
    pub int_vars: IntVariableTable,
    pub float_vars: FloatVariableTable,
}

impl<'tcx> VariableTables<'tcx> {
    pub fn new() -> Self {
        Self {
            ty_vars: TypeVariableTable::new(TypeVarValue::Unknown),
            int_vars: IntVariableTable::new(IntVarValue::Unknown),
            float_vars: FloatVariableTable::new(FloatVarValue::Unknown),
        }
    }
}

impl<'tcx> Default for VariableTables<'tcx> {
    fn default() -> Self {
        Self::new()
    }
}

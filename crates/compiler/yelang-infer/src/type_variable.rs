/*! Type variable tables for general, int, and float variables. */

use yelang_ty::primitive::{FloatTy, IntegerTy};
use yelang_ty::ty::{FloatVid, IntVid, TyId, TyVid};

use crate::const_variable::{ConstVarValue, ConstVariableTable};
use crate::unify::UnificationTable;

/// Value stored for a general type variable.
#[derive(Clone, Debug, PartialEq)]
pub enum TypeVarValue {
    Known(TyId),
    Unknown,
}

impl Default for TypeVarValue {
    fn default() -> Self {
        TypeVarValue::Unknown
    }
}

/// Value stored for an integral type variable.
#[derive(Clone, Debug, PartialEq)]
pub enum IntVarValue {
    Known(IntegerTy),
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
pub type TypeVariableTable = UnificationTable<TyVid, TypeVarValue>;

/// Table of integral type variables.
pub type IntVariableTable = UnificationTable<IntVid, IntVarValue>;

/// Table of floating-point type variables.
pub type FloatVariableTable = UnificationTable<FloatVid, FloatVarValue>;

/// Combined variable tables.
pub struct VariableTables {
    pub ty_vars: TypeVariableTable,
    pub int_vars: IntVariableTable,
    pub float_vars: FloatVariableTable,
    pub const_vars: ConstVariableTable,
}

impl VariableTables {
    pub fn new() -> Self {
        Self {
            ty_vars: TypeVariableTable::new(TypeVarValue::Unknown),
            int_vars: IntVariableTable::new(IntVarValue::Unknown),
            float_vars: FloatVariableTable::new(FloatVarValue::Unknown),
            const_vars: ConstVariableTable::new(ConstVarValue::Unknown),
        }
    }
}

impl Default for VariableTables {
    fn default() -> Self {
        Self::new()
    }
}

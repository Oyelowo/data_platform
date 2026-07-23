//! MIR operands and constants.

use yelang_ty::ty::TyId;

use crate::place::Place;

/// An operand: a value used in rvalues and call arguments.
#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    /// Copy the value from a place (for Copy types).
    Copy(Place),
    /// Move the value out of a place (for non-Copy types).
    Move(Place),
    /// A compile-time constant.
    Constant(Constant),
}

impl Operand {
    /// Create a Copy operand from a local.
    pub fn copy(local: crate::body::Local) -> Self {
        Operand::Copy(Place::local(local))
    }

    /// Create a Move operand from a local.
    pub fn move_(local: crate::body::Local) -> Self {
        Operand::Move(Place::local(local))
    }
}

/// A compile-time constant value.
#[derive(Debug, Clone, PartialEq)]
pub struct Constant {
    /// The type of the constant.
    pub ty: TyId,
    /// The constant value.
    pub value: ConstValue,
}

/// The value of a constant.
#[derive(Debug, Clone, PartialEq)]
pub enum ConstValue {
    /// An integer literal.
    Int(i128),
    /// An unsigned integer literal.
    Uint(u128),
    /// A float literal.
    Float(f64),
    /// A boolean literal.
    Bool(bool),
    /// A character literal.
    Char(char),
    /// A string literal (interned).
    Str(yelang_interner::Symbol),
    /// Unit value `()`.
    Unit,
    /// A function pointer.
    FnPtr(yelang_arena::DefId),
}

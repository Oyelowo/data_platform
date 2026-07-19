//! Newtyped indices for QIR operators, physical operators, and expressions.

use std::fmt;

use yelang_arena::index_vec::{Idx, IndexVec};

/// Identifier for a logical QIR operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QirId(pub u32);

impl QirId {
    /// Return the raw integer value.
    pub fn raw(self) -> u32 {
        self.0
    }
}

impl fmt::Display for QirId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Idx for QirId {
    fn from_usize(idx: usize) -> Self {
        QirId(idx as u32)
    }

    fn index(self) -> usize {
        self.0 as usize
    }
}

/// Identifier for a physical QIR operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PhysId(pub u32);

impl PhysId {
    /// Return the raw integer value.
    pub fn raw(self) -> u32 {
        self.0
    }
}

impl fmt::Display for PhysId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Idx for PhysId {
    fn from_usize(idx: usize) -> Self {
        PhysId(idx as u32)
    }

    fn index(self) -> usize {
        self.0 as usize
    }
}

/// Identifier for a QIR scalar expression.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QExprId(pub u32);

impl QExprId {
    /// Return the raw integer value.
    pub fn raw(self) -> u32 {
        self.0
    }
}

impl fmt::Display for QExprId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Idx for QExprId {
    fn from_usize(idx: usize) -> Self {
        QExprId(idx as u32)
    }

    fn index(self) -> usize {
        self.0 as usize
    }
}

/// Convenience aliases for arena-backed collections.
pub type QirArena<T> = IndexVec<QirId, T>;
pub type PhysArena<T> = IndexVec<PhysId, T>;
pub type QExprArena<T> = IndexVec<QExprId, T>;

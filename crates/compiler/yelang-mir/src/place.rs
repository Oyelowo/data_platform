//! MIR places and projections.

use yelang_interner::Symbol;
use yelang_ty::ty::TyId;

use crate::body::Local;

/// A memory location: a local variable plus a projection path.
///
/// `Place { local: _1, projection: [Field("x"), Index(_2)] }`
/// represents `_1.x[_2]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// The base local variable.
    pub local: Local,
    /// The projection path (field access, indexing, deref).
    pub projection: Vec<Projection>,
}

impl Place {
    /// A simple local with no projection.
    pub fn local(local: Local) -> Self {
        Self {
            local,
            projection: Vec::new(),
        }
    }

    /// Project through a field.
    pub fn field(mut self, name: Symbol, ty: TyId) -> Self {
        self.projection.push(Projection::Field(name, ty));
        self
    }

    /// Project through an index.
    pub fn index(mut self, index: Local) -> Self {
        self.projection.push(Projection::Index(index));
        self
    }

    /// Dereference.
    pub fn deref(mut self) -> Self {
        self.projection.push(Projection::Deref);
        self
    }

    /// Whether this is a simple local (no projection).
    pub fn is_simple(&self) -> bool {
        self.projection.is_empty()
    }
}

/// A single step in a place projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Projection {
    /// Access a field by name: `.field_name`.
    Field(Symbol, TyId),
    /// Index by a local: `[index_local]`.
    Index(Local),
    /// Dereference: `*place`.
    Deref,
}

//! Demand-set tracking: which fields/columns are actually consumed downstream.
//!
//! Used for projection pushdown and dead-source elision.

use std::collections::HashSet;

use yelang_interner::Symbol;

/// A set of fields demanded from a record/tuple value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DemandSet {
    fields: HashSet<Symbol>,
    all: bool,
}

impl DemandSet {
    /// Demand all fields.
    pub fn all() -> Self {
        Self { fields: HashSet::new(), all: true }
    }

    /// Demand no fields.
    pub fn none() -> Self {
        Self { fields: HashSet::new(), all: false }
    }

    /// Demand a specific field.
    pub fn field(field: Symbol) -> Self {
        let mut s = Self::none();
        s.insert(field);
        s
    }

    /// Demand multiple fields.
    pub fn fields<I: IntoIterator<Item = Symbol>>(iter: I) -> Self {
        Self { fields: iter.into_iter().collect(), all: false }
    }

    /// Insert a field into the demand set.
    pub fn insert(&mut self, field: Symbol) {
        self.fields.insert(field);
    }

    /// Union two demand sets.
    pub fn union(&mut self, other: &DemandSet) {
        self.all |= other.all;
        self.fields.extend(other.fields.iter().copied());
    }

    /// Returns true if all fields are demanded.
    pub fn is_all(&self) -> bool {
        self.all
    }

    /// Returns true if no specific fields are demanded and `all` is false.
    pub fn is_empty(&self) -> bool {
        !self.all && self.fields.is_empty()
    }

    /// Returns true if the given field is demanded.
    pub fn contains(&self, field: Symbol) -> bool {
        self.all || self.fields.contains(&field)
    }

    /// Iterate demanded fields. Empty if `all` is true.
    pub fn iter(&self) -> impl Iterator<Item = Symbol> + '_ {
        self.fields.iter().copied()
    }
}

use std::fmt;
use std::num::NonZeroU32;

/// A newtype wrapper around a raw integer ID.
///
/// Provides type safety so that `DefId` and `HirId` cannot be confused.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id<T> {
    raw: NonZeroU32,
    _marker: std::marker::PhantomData<T>,
}

impl<T> Id<T> {
    /// Creates a new `Id` from a 1-based raw value.
    ///
    /// Panics if `raw` is 0.
    pub fn new(raw: u32) -> Self {
        Self {
            raw: NonZeroU32::new(raw).expect("Id cannot be 0"),
            _marker: std::marker::PhantomData,
        }
    }

    /// Creates an `Id` from a raw value, returning `None` if the value is 0.
    pub fn try_new(raw: u32) -> Option<Self> {
        NonZeroU32::new(raw).map(|raw| Self {
            raw,
            _marker: std::marker::PhantomData,
        })
    }

    /// Returns the raw integer value.
    pub fn raw(self) -> u32 {
        self.raw.get()
    }

    /// Returns the raw value as a `usize` for indexing.
    pub fn as_usize(self) -> usize {
        self.raw.get() as usize
    }

    /// Creates an `Id` from a `usize`, panicking on overflow or 0.
    pub fn from_usize(raw: usize) -> Self {
        Self::new(u32::try_from(raw).expect("Id overflow"))
    }
}

impl<T> Default for Id<T> {
    fn default() -> Self {
        Self::new(1)
    }
}

impl<T> fmt::Debug for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Id")
            .field("raw", &self.raw.get())
            .finish()
    }
}

impl<T> fmt::Display for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.raw.get())
    }
}

/// Tag types for `Id`.
pub mod tags {
    /// Tag for `Id<TagDef>`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct TagDef;

    /// Tag for `Id<TagHir>`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct TagHir;

    /// Tag for `Id<TagBody>`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct TagBody;

    /// Tag for `Id<TagLocal>`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct TagLocal;
}

/// Type-safe definition ID.
pub type DefId = Id<tags::TagDef>;

/// Type-safe HIR node ID.
pub type HirId = Id<tags::TagHir>;

/// Type-safe body ID.
pub type BodyId = Id<tags::TagBody>;

/// Type-safe local variable ID.
pub type LocalId = Id<tags::TagLocal>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_basic() {
        let id = DefId::new(1);
        assert_eq!(id.raw(), 1);
        assert_eq!(id.as_usize(), 1);
    }

    #[test]
    #[should_panic(expected = "Id cannot be 0")]
    fn id_zero_panics() {
        let _ = DefId::new(0);
    }

    #[test]
    fn id_try_new() {
        assert!(DefId::try_new(0).is_none());
        assert!(DefId::try_new(1).is_some());
    }

    #[test]
    fn id_type_safety() {
        let def = DefId::new(1);
        let hir = HirId::new(1);
        // def and hir are different types; this would be a compile error:
        // assert_eq!(def, hir);
        assert_eq!(def.raw(), hir.raw());
    }
}

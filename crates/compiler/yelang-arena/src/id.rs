use std::fmt;
use std::num::NonZeroU32;

use crate::index_vec::Idx;

/// A newtype wrapper around a raw integer ID.
///
/// Provides type safety so that different ID spaces (e.g. `DefId`) cannot be confused.
///
/// The implementations of `Copy`, `Eq`, `Hash`, etc. do not require `T` to
/// implement those traits because `Id<T>` owns only a `NonZeroU32` and a
/// `PhantomData<T>` marker.
pub struct Id<T> {
    raw: NonZeroU32,
    _marker: std::marker::PhantomData<T>,
}

impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Id<T> {}

impl<T> PartialEq for Id<T> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl<T> Eq for Id<T> {}

impl<T> PartialOrd for Id<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Id<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.raw.cmp(&other.raw)
    }
}

impl<T> std::hash::Hash for Id<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
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
        f.debug_struct("Id").field("raw", &self.raw.get()).finish()
    }
}

impl<T> fmt::Display for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.raw.get())
    }
}

impl<T> Idx for Id<T> {
    fn from_usize(idx: usize) -> Self {
        Self::new(u32::try_from(idx).expect("Id index overflow") + 1)
    }

    fn index(self) -> usize {
        (self.raw() - 1) as usize
    }
}

/// Tag types for `Id`.
pub mod tags {
    /// Tag for `Id<TagDef>`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct TagDef;

    /// Tag for `Id<TagSyntaxContext>`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct TagSyntaxContext;
}

/// Type-safe definition ID.
pub type DefId = Id<tags::TagDef>;

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

}

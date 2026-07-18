/*! Associated type projection types.
 *
 * A projection type `<T as Trait>::Assoc` is represented by `ProjectionTy`,
 * which stores the trait reference and the `DefId` of the associated type item.
 */

pub use crate::ty::ProjectionTy;

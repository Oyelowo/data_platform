/*!
 * Yelang Arena Crate
 *
 * Wrapper types around external dependencies. All collections used by the
 * compiler pipeline should be imported from this crate so that the underlying
 * implementation can be swapped without changing call sites.
 */

mod arena;
mod id;
pub mod index_vec;
mod map;
mod set;

pub use arena::*;
pub use id::*;
pub use index_vec::*;
pub use map::*;
pub use set::*;

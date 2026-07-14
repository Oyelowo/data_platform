/*!
 * Yelang Utility Crate
 *
 * Wrapper types around external dependencies. All collections used by the
 * compiler pipeline should be imported from this crate so that the underlying
 * implementation can be swapped without changing call sites.
 */

mod arena;
mod id;
mod map;
mod set;

pub use arena::*;
pub use id::*;
pub use map::*;
pub use set::*;

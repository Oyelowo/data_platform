/*!
 * Macro invocation, panic handling, and execution context.
 */

pub mod context;
pub mod convert;
pub mod invoke;
pub mod panic;

pub use context::MacroContext;
pub use invoke::{invoke_attr, invoke_derive, invoke_fn_like};

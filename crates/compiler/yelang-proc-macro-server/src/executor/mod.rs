/*!
 * Macro invocation, panic handling, and execution context.
 */

pub mod context;
pub mod invoke;
pub mod limits;
pub mod panic;

pub use context::MacroContext;
pub use invoke::{InvokeError, invoke_attr, invoke_derive, invoke_fn_like};

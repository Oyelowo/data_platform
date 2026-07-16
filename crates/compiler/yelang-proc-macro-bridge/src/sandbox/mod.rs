/*!
 * Sandboxing and resource limits for proc-macro execution.
 */

pub mod error;
pub mod limits;

pub use error::SandboxError;
pub use limits::Limits;

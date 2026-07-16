/*!
 * Yelang Proc-Macro Bridge
 *
 * Stable ABI and wire protocol between the compiler and the proc-macro server.
 * This crate must remain backwards compatible so that proc-macro crates compiled
 * with older compilers continue to work with newer servers.
 */

pub mod abi;
pub mod protocol;
pub mod sandbox;

pub use abi::{
    ALLOC_SYMBOL, CURRENT_ABI_VERSION, ENTRY_SYMBOL, FREE_SYMBOL, ProcMacroKind, YelangAllocFn,
    YelangAttrMacro, YelangDeriveMacro, YelangFnLikeMacro, YelangFreeFn, YelangMacroDescriptor,
    YelangMacroInvoke, YelangProcMacroEntry, YelangProcMacroExports,
};
pub use protocol::{ErrorCode, Request, Response, WireTokenStream};
pub use sandbox::{Limits, SandboxError};

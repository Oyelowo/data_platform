/*!
 * Stable C ABI exposed by every proc-macro dynamic library.
 */

pub mod registrar;
pub mod signature;
pub mod symbols;

pub use registrar::{
    CURRENT_ABI_VERSION, YelangAllocFn, YelangAttrMacro, YelangDeriveMacro, YelangFnLikeMacro,
    YelangFreeFn, YelangMacroDescriptor, YelangMacroInvoke, YelangProcMacroEntry,
    YelangProcMacroExports, YelangProcMacroKind,
};
pub use signature::ProcMacroKind;
pub use symbols::{ALLOC_SYMBOL, ENTRY_SYMBOL, FREE_SYMBOL};

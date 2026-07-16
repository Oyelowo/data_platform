//! Registration table exposed by a proc-macro dynamic library.

/// Opaque token stream handle used across the C ABI.
#[repr(C)]
pub struct YelangTokenStream {
    _private: [u8; 0],
}

/// Function-like macro signature.
pub type YelangFnLikeMacro =
    unsafe extern "C" fn(*const YelangTokenStream) -> *mut YelangTokenStream;

/// Attribute macro signature.
pub type YelangAttrMacro = unsafe extern "C" fn(
    *const YelangTokenStream,
    *const YelangTokenStream,
) -> *mut YelangTokenStream;

/// Derive macro signature.
pub type YelangDeriveMacro =
    unsafe extern "C" fn(*const YelangTokenStream) -> *mut YelangTokenStream;

/// Entry-point function signature.
pub type YelangProcMacroEntry =
    unsafe extern "C" fn(abi_version: u32) -> *const YelangProcMacroExports;

/// Macro descriptor inside the exports table.
#[repr(C)]
pub struct YelangMacroDescriptor {
    pub name: *const u8,
    pub name_len: usize,
    pub kind: YelangProcMacroKind,
    pub invoke: YelangMacroInvoke,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union YelangMacroInvoke {
    pub fn_like: YelangFnLikeMacro,
    pub attr: YelangAttrMacro,
    pub derive: YelangDeriveMacro,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub enum YelangProcMacroKind {
    FunctionLike,
    Attribute,
    Derive,
}

/// Exports table returned by `yelang_proc_macro_entry`.
#[repr(C)]
pub struct YelangProcMacroExports {
    pub abi_version: u32,
    pub macro_count: usize,
    pub macros: *const YelangMacroDescriptor,
}

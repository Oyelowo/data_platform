//! Registration table exposed by a proc-macro dynamic library.
//!
//! The ABI uses serialized postcard token streams rather than a handle-table/
//! vtable. The dylib receives input as a `(ptr, len)` byte buffer and returns
//! output by allocating a buffer with its exported `yelang_alloc` function and
//! writing the `(ptr, len)` into the provided out-parameters. The server frees
//! the returned buffer with the dylib's exported `yelang_free` function.

/// Current dylib ABI version.
///
/// This is checked at `yelang_proc_macro_entry` time. It is independent of the
/// compiler/server wire protocol version (`CURRENT_PROTOCOL_VERSION`).
pub const CURRENT_ABI_VERSION: u32 = 1;

/// Function-like macro signature.
///
/// `input`/`input_len` is a postcard-serialized `WireTokenStream`.
/// `output`/`output_len` receive an allocated postcard-serialized
/// `WireExpansionResult` that the caller must free with `YelangFreeFn`.
///
/// Function-like macro signature.
///
/// `input`/`input_len` is a postcard-serialized `WireTokenStream`.
/// `output`/`output_len` receive an allocated postcard-serialized
/// `WireExpansionResult` that the caller must free with `YelangFreeFn`.
///
/// The `C-unwind` ABI lets the server install a last-resort catch for any
/// unwinding that escapes the generated dylib wrapper. The normal path is for
/// panics to be caught inside the dylib and converted into error diagnostics.
pub type YelangFnLikeMacro = unsafe extern "C-unwind" fn(
    input: *const u8,
    input_len: usize,
    output: *mut *mut u8,
    output_len: *mut usize,
);

/// Attribute macro signature.
///
/// `args`/`args_len` and `item`/`item_len` are postcard-serialized
/// `WireTokenStream`s. `output`/`output_len` receive an allocated
/// postcard-serialized `WireExpansionResult`.
pub type YelangAttrMacro = unsafe extern "C-unwind" fn(
    args: *const u8,
    args_len: usize,
    item: *const u8,
    item_len: usize,
    output: *mut *mut u8,
    output_len: *mut usize,
);

/// Derive macro signature.
///
/// `item`/`item_len` is a postcard-serialized `WireTokenStream`.
/// `output`/`output_len` receive an allocated postcard-serialized
/// `WireExpansionResult`.
pub type YelangDeriveMacro = unsafe extern "C-unwind" fn(
    item: *const u8,
    item_len: usize,
    output: *mut *mut u8,
    output_len: *mut usize,
);

/// Allocator exposed by the dylib. Mirrors the C `malloc` signature.
pub type YelangAllocFn = unsafe extern "C" fn(size: usize) -> *mut u8;

/// Deallocator exposed by the dylib. Mirrors the C `free` signature.
pub type YelangFreeFn = unsafe extern "C" fn(ptr: *mut u8);

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

// The descriptor is intended to live in read-only dylib static data. Its raw
// pointer fields are immutable after load and the union is initialized by the
// dylib, so it is safe to share across threads.
unsafe impl Sync for YelangMacroDescriptor {}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct YelangMacroInvoke {
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
    pub alloc: YelangAllocFn,
    pub free: YelangFreeFn,
}

unsafe impl Sync for YelangProcMacroExports {}

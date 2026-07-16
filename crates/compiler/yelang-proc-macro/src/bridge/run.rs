//! Helpers used by the code generated for `#[yelang_proc_macro::macro_export]`.
//!
//! These functions bridge the C ABI to the public `TokenStream` API: they
//! deserialize the postcard input buffers, run the user-written macro, drain
//! any emitted diagnostics, and serialize the `WireExpansionResult` back into
//! a byte vector that the generated wrapper copies into dylib-allocated memory.

use yelang_proc_macro_bridge::protocol::token::WireTokenStream;

use super::{from_wire, result_into_wire};
use crate::api::diagnostic::drain_diagnostics;
use crate::{Diagnostic, TokenStream};

/// Allocate an output buffer of `size` bytes using the dylib allocator.
///
/// The returned pointer must be freed with the dylib's `yelang_free` function,
/// which uses the same underlying allocator (`libc::free`).
pub fn alloc_output_buffer(size: usize) -> *mut u8 {
    unsafe { libc::malloc(size) as *mut u8 }
}

/// Free a buffer previously allocated by `alloc_output_buffer`.
///
/// This is the implementation of the dylib's exported `yelang_free` symbol.
pub fn free_output_buffer(ptr: *mut u8) {
    if !ptr.is_null() {
        unsafe { libc::free(ptr as *mut libc::c_void) }
    }
}

/// Function pointer type accepted by the generated function-like macro wrapper.
pub type FnLikeMacroFn = fn(TokenStream) -> TokenStream;

/// Function pointer type accepted by the generated attribute macro wrapper.
pub type AttrMacroFn = fn(TokenStream, TokenStream) -> TokenStream;

/// Function pointer type accepted by the generated derive macro wrapper.
pub type DeriveMacroFn = fn(TokenStream) -> TokenStream;

/// Run a function-like procedural macro and write the serialized
/// `WireExpansionResult` into the caller-provided output buffer.
///
/// This helper performs all of the `unsafe` raw-pointer work on behalf of the
/// generated Yelang wrapper so that the wrapper can remain a safe function.
pub fn run_fn_like_macro(
    f: FnLikeMacroFn,
    input: *const u8,
    input_len: usize,
    output: *mut *mut u8,
    output_len: *mut usize,
) {
    let bytes = run_fn_like_macro_to_bytes(f, input, input_len);
    write_bytes_to_output(bytes, output, output_len);
}

/// Run an attribute procedural macro and write the serialized
/// `WireExpansionResult` into the caller-provided output buffer.
pub fn run_attr_macro(
    f: AttrMacroFn,
    args: *const u8,
    args_len: usize,
    item: *const u8,
    item_len: usize,
    output: *mut *mut u8,
    output_len: *mut usize,
) {
    let bytes = run_attr_macro_to_bytes(f, args, args_len, item, item_len);
    write_bytes_to_output(bytes, output, output_len);
}

/// Run a derive procedural macro and write the serialized
/// `WireExpansionResult` into the caller-provided output buffer.
pub fn run_derive_macro(
    f: DeriveMacroFn,
    item: *const u8,
    item_len: usize,
    output: *mut *mut u8,
    output_len: *mut usize,
) {
    let bytes = run_derive_macro_to_bytes(f, item, item_len);
    write_bytes_to_output(bytes, output, output_len);
}

fn write_bytes_to_output(bytes: Vec<u8>, output: *mut *mut u8, output_len: *mut usize) {
    let len = bytes.len();
    let ptr = alloc_output_buffer(len);
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, len);
        *output_len = len;
        *output = ptr;
    }
}

/// Run a function-like procedural macro and return the serialized
/// `WireExpansionResult`.
pub fn run_fn_like_macro_to_bytes(f: FnLikeMacroFn, input: *const u8, input_len: usize) -> Vec<u8> {
    let input = deserialize_input(input, input_len);
    let output = f(input);
    serialize_output(output)
}

/// Run an attribute procedural macro and return the serialized
/// `WireExpansionResult`.
pub fn run_attr_macro_to_bytes(
    f: AttrMacroFn,
    args: *const u8,
    args_len: usize,
    item: *const u8,
    item_len: usize,
) -> Vec<u8> {
    let args = deserialize_input(args, args_len);
    let item = deserialize_input(item, item_len);
    let output = f(args, item);
    serialize_output(output)
}

/// Run a derive procedural macro and return the serialized `WireExpansionResult`.
pub fn run_derive_macro_to_bytes(f: DeriveMacroFn, item: *const u8, item_len: usize) -> Vec<u8> {
    let item = deserialize_input(item, item_len);
    let output = f(item);
    serialize_output(output)
}

fn deserialize_input(input: *const u8, input_len: usize) -> TokenStream {
    let bytes = unsafe { std::slice::from_raw_parts(input, input_len) };
    let wire: WireTokenStream =
        postcard::from_bytes(bytes).expect("proc-macro server sent an invalid token stream");
    from_wire(wire)
}

fn serialize_output(output: TokenStream) -> Vec<u8> {
    let diagnostics: Vec<Diagnostic> = drain_diagnostics();
    let result = result_into_wire(output, diagnostics);
    postcard::to_allocvec(&result).expect("failed to serialize macro output")
}

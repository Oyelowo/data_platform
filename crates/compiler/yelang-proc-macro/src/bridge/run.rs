//! Helpers used by the code generated for `#[yelang_proc_macro::macro_export]`.
//!
//! These functions bridge the C ABI to the public `TokenStream` API: they
//! deserialize the postcard input buffers, run the user-written macro, drain
//! any emitted diagnostics, and serialize the `WireExpansionResult` back into
//! a byte vector that the generated wrapper copies into dylib-allocated memory.
//!
//! Panics inside the user-provided macro are caught inside the dylib and
//! converted into an error diagnostic. This keeps the server process stable and
//! avoids relying on cross-dylib unwind compatibility.

use std::cell::{Cell, RefCell};
use std::panic;

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
    match with_panic_capture(|| f(input)) {
        Ok(output) => serialize_output(output),
        Err(message) => serialize_panic(message),
    }
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
    match with_panic_capture(|| f(args, item)) {
        Ok(output) => serialize_output(output),
        Err(message) => serialize_panic(message),
    }
}

/// Run a derive procedural macro and return the serialized `WireExpansionResult`.
pub fn run_derive_macro_to_bytes(f: DeriveMacroFn, item: *const u8, item_len: usize) -> Vec<u8> {
    let item = deserialize_input(item, item_len);
    match with_panic_capture(|| f(item)) {
        Ok(output) => serialize_output(output),
        Err(message) => serialize_panic(message),
    }
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

/// Serialize a panic message as an error diagnostic.
fn serialize_panic(message: String) -> Vec<u8> {
    let mut diagnostics = drain_diagnostics();
    diagnostics.push(Diagnostic::error(format!("macro panicked: {message}")));
    let result = result_into_wire(TokenStream::new(), diagnostics);
    postcard::to_allocvec(&result).expect("failed to serialize panic result")
}

thread_local! {
    static CAPTURE_PANICS: Cell<bool> = const { Cell::new(false) };
    static CAPTURED_PANIC: RefCell<Option<String>> = const { RefCell::new(None) };
}

static INSTALL_PANIC_CAPTURE: std::sync::Once = std::sync::Once::new();

/// Capture panics inside the user macro and return the formatted message.
///
/// We capture the message through the panic hook rather than by downcasting the
/// `Box<dyn Any>` payload, because the concrete payload type can differ between
/// the dylib's panic runtime and the bridge crate (especially for `cdylib`
/// builds). The hook chains to the previously installed hook for panics that
/// occur outside an active capture region.
fn with_panic_capture<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    INSTALL_PANIC_CAPTURE.call_once(|| {
        let prev = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if CAPTURE_PANICS.with(|c| c.get()) {
                let message = info
                    .payload()
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_string())
                    .or_else(|| info.payload().downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "proc macro panicked".to_string());
                CAPTURED_PANIC.with(|c| *c.borrow_mut() = Some(message));
            } else {
                prev(info);
            }
        }));
    });

    CAPTURE_PANICS.with(|c| c.set(true));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    CAPTURE_PANICS.with(|c| c.set(false));

    match result {
        Ok(r) => Ok(r),
        Err(_) => Err(CAPTURED_PANIC
            .with(|c| c.borrow_mut().take())
            .unwrap_or_else(|| "proc macro panicked".to_string())),
    }
}

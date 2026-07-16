//! Invoke proc-macro functions across the stable serialized dylib ABI.

use yelang_proc_macro_bridge::protocol::token::{
    WireDiagnostic, WireExpansionResult, WireTokenStream,
};

use super::panic::payload_to_message;
use crate::server::library::{LoadedLibrary, LoadedMacro};

/// Failure during a single macro invocation.
#[derive(Debug, thiserror::Error)]
pub enum InvokeError {
    #[error("macro index out of bounds")]
    MacroIndexOutOfBounds,

    #[error("macro is not {expected}")]
    WrongKind { expected: &'static str },

    #[error("failed to serialize input: {0}")]
    InputSerialization(#[source] postcard::Error),

    #[error("macro returned null output")]
    NullOutput,

    #[error("failed to deserialize macro output: {0}")]
    OutputDeserialization(#[source] postcard::Error),

    #[error("macro panicked: {0}")]
    Panic(String),

    #[error("internal error: {0}")]
    Internal(String),
}

/// Invoke a function-like macro by index in a loaded library.
pub fn invoke_fn_like(
    library: &LoadedLibrary,
    macro_index: u32,
    input: WireTokenStream,
) -> Result<(WireTokenStream, Vec<WireDiagnostic>), InvokeError> {
    let mac = library
        .get_macro(macro_index as usize)
        .ok_or(InvokeError::MacroIndexOutOfBounds)?;

    let LoadedMacro::FunctionLike(fn_ptr) = mac else {
        return Err(InvokeError::WrongKind {
            expected: "function-like",
        });
    };

    let input_bytes = postcard::to_allocvec(&input).map_err(InvokeError::InputSerialization)?;

    let (output_ptr, output_len) = invoke_ffi(|| {
        let mut output_ptr: *mut u8 = std::ptr::null_mut();
        let mut output_len: usize = 0;
        unsafe {
            fn_ptr(
                input_bytes.as_ptr(),
                input_bytes.len(),
                &mut output_ptr,
                &mut output_len,
            );
        }
        (output_ptr, output_len)
    })?;

    decode_result(library, output_ptr, output_len)
}

/// Invoke an attribute macro.
pub fn invoke_attr(
    library: &LoadedLibrary,
    macro_index: u32,
    args: WireTokenStream,
    item: WireTokenStream,
) -> Result<(WireTokenStream, Vec<WireDiagnostic>), InvokeError> {
    let mac = library
        .get_macro(macro_index as usize)
        .ok_or(InvokeError::MacroIndexOutOfBounds)?;

    let LoadedMacro::Attribute(fn_ptr) = mac else {
        return Err(InvokeError::WrongKind {
            expected: "attribute",
        });
    };

    let args_bytes = postcard::to_allocvec(&args).map_err(InvokeError::InputSerialization)?;
    let item_bytes = postcard::to_allocvec(&item).map_err(InvokeError::InputSerialization)?;

    let (output_ptr, output_len) = invoke_ffi(|| {
        let mut output_ptr: *mut u8 = std::ptr::null_mut();
        let mut output_len: usize = 0;
        unsafe {
            fn_ptr(
                args_bytes.as_ptr(),
                args_bytes.len(),
                item_bytes.as_ptr(),
                item_bytes.len(),
                &mut output_ptr,
                &mut output_len,
            );
        }
        (output_ptr, output_len)
    })?;

    decode_result(library, output_ptr, output_len)
}

/// Invoke a derive macro.
pub fn invoke_derive(
    library: &LoadedLibrary,
    macro_index: u32,
    item: WireTokenStream,
) -> Result<(WireTokenStream, Vec<WireDiagnostic>), InvokeError> {
    let mac = library
        .get_macro(macro_index as usize)
        .ok_or(InvokeError::MacroIndexOutOfBounds)?;

    let LoadedMacro::Derive(fn_ptr) = mac else {
        return Err(InvokeError::WrongKind { expected: "derive" });
    };

    let item_bytes = postcard::to_allocvec(&item).map_err(InvokeError::InputSerialization)?;

    let (output_ptr, output_len) = invoke_ffi(|| {
        let mut output_ptr: *mut u8 = std::ptr::null_mut();
        let mut output_len: usize = 0;
        unsafe {
            fn_ptr(
                item_bytes.as_ptr(),
                item_bytes.len(),
                &mut output_ptr,
                &mut output_len,
            );
        }
        (output_ptr, output_len)
    })?;

    decode_result(library, output_ptr, output_len)
}

/// Call an FFI function and convert any panic into a clean `InvokeError::Panic`.
fn invoke_ffi<F>(f: F) -> Result<(*mut u8, usize), InvokeError>
where
    F: FnOnce() -> (*mut u8, usize),
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok((ptr, len)) => Ok((ptr, len)),
        Err(payload) => Err(InvokeError::Panic(payload_to_message(&payload))),
    }
}

/// Deserialize the buffer returned by a proc macro and free it with the dylib's
/// exported deallocator.
fn decode_result(
    library: &LoadedLibrary,
    output_ptr: *mut u8,
    output_len: usize,
) -> Result<(WireTokenStream, Vec<WireDiagnostic>), InvokeError> {
    if output_ptr.is_null() {
        return Err(InvokeError::NullOutput);
    }

    struct BufferGuard {
        ptr: *mut u8,
        free: yelang_proc_macro_bridge::abi::YelangFreeFn,
    }

    impl Drop for BufferGuard {
        fn drop(&mut self) {
            if !self.ptr.is_null() {
                unsafe {
                    (self.free)(self.ptr);
                }
            }
        }
    }

    let _guard = BufferGuard {
        ptr: output_ptr,
        free: library.free_fn(),
    };

    let output_bytes = unsafe { std::slice::from_raw_parts(output_ptr, output_len) };
    let result: WireExpansionResult =
        postcard::from_bytes(output_bytes).map_err(InvokeError::OutputDeserialization)?;

    Ok((result.output, result.diagnostics))
}

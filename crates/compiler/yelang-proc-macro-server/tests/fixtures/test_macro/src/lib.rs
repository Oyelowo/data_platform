//! Test proc-macro dynamic library for the proc-macro server integration tests.
//!
//! This crate manually implements the stable C ABI that
//! `#[yelang_proc_macro::macro_export]` will eventually generate automatically.
//! It exports four macros:
//!
//! - `make_answer` — function-like, returns the token stream `42`.
//! - `trace` — attribute, returns the item unchanged.
//! - `answer` — derive, returns the token stream `42`.
//! - `panic` — function-like, panics to test server-side panic recovery.

use yelang_proc_macro::{
    Literal, Span, TokenStream, TokenTree,
    bridge::{from_wire, result_into_wire},
};
use yelang_proc_macro_bridge::abi::{
    CURRENT_ABI_VERSION, YelangAttrMacro, YelangDeriveMacro, YelangFnLikeMacro,
    YelangMacroDescriptor, YelangMacroInvoke, YelangProcMacroExports, YelangProcMacroKind,
};
use yelang_proc_macro_bridge::protocol::token::WireExpansionResult;

static MAKE_ANSWER_NAME: &[u8] = b"make_answer";
static TRACE_NAME: &[u8] = b"trace";
static ANSWER_NAME: &[u8] = b"answer";
static PANIC_NAME: &[u8] = b"panic";

unsafe extern "C-unwind" fn null_fn_like(_: *const u8, _: usize, _: *mut *mut u8, _: *mut usize) {
    unreachable!("null fn-like invoke")
}

unsafe extern "C-unwind" fn null_attr(
    _: *const u8,
    _: usize,
    _: *const u8,
    _: usize,
    _: *mut *mut u8,
    _: *mut usize,
) {
    unreachable!("null attr invoke")
}

unsafe extern "C-unwind" fn null_derive(_: *const u8, _: usize, _: *mut *mut u8, _: *mut usize) {
    unreachable!("null derive invoke")
}

static MACROS: [YelangMacroDescriptor; 4] = [
    YelangMacroDescriptor {
        name: MAKE_ANSWER_NAME.as_ptr(),
        name_len: MAKE_ANSWER_NAME.len(),
        kind: YelangProcMacroKind::FunctionLike,
        invoke: YelangMacroInvoke {
            fn_like: make_answer,
            attr: null_attr,
            derive: null_derive,
        },
    },
    YelangMacroDescriptor {
        name: TRACE_NAME.as_ptr(),
        name_len: TRACE_NAME.len(),
        kind: YelangProcMacroKind::Attribute,
        invoke: YelangMacroInvoke {
            fn_like: null_fn_like,
            attr: trace,
            derive: null_derive,
        },
    },
    YelangMacroDescriptor {
        name: ANSWER_NAME.as_ptr(),
        name_len: ANSWER_NAME.len(),
        kind: YelangProcMacroKind::Derive,
        invoke: YelangMacroInvoke {
            fn_like: null_fn_like,
            attr: null_attr,
            derive: answer,
        },
    },
    YelangMacroDescriptor {
        name: PANIC_NAME.as_ptr(),
        name_len: PANIC_NAME.len(),
        kind: YelangProcMacroKind::FunctionLike,
        invoke: YelangMacroInvoke {
            fn_like: panic_macro,
            attr: null_attr,
            derive: null_derive,
        },
    },
];

static EXPORTS: YelangProcMacroExports = YelangProcMacroExports {
    abi_version: CURRENT_ABI_VERSION,
    macro_count: MACROS.len(),
    macros: MACROS.as_ptr(),
    alloc: yelang_alloc,
    free: yelang_free,
};

#[unsafe(no_mangle)]
pub extern "C" fn yelang_proc_macro_entry(_abi_version: u32) -> *const YelangProcMacroExports {
    &EXPORTS
}

#[unsafe(no_mangle)]
pub extern "C" fn yelang_alloc(size: usize) -> *mut u8 {
    unsafe { libc::malloc(size) as *mut u8 }
}

#[unsafe(no_mangle)]
pub extern "C" fn yelang_free(ptr: *mut u8) {
    if !ptr.is_null() {
        unsafe { libc::free(ptr as *mut libc::c_void) }
    }
}

unsafe extern "C-unwind" fn make_answer(
    input: *const u8,
    input_len: usize,
    output: *mut *mut u8,
    output_len: *mut usize,
) {
    let _ = deserialize_input(input, input_len);
    let mut ts = TokenStream::new();
    ts.push(TokenTree::Literal(Literal::integer(
        "42",
        Span::call_site(),
    )));
    serialize_output(result_into_wire(ts, Vec::new()), output, output_len);
}

unsafe extern "C-unwind" fn trace(
    args: *const u8,
    args_len: usize,
    item: *const u8,
    item_len: usize,
    output: *mut *mut u8,
    output_len: *mut usize,
) {
    let _args = deserialize_input(args, args_len);
    let item_wire = deserialize_input(item, item_len);
    let item = from_wire(item_wire);
    serialize_output(result_into_wire(item, Vec::new()), output, output_len);
}

unsafe extern "C-unwind" fn answer(
    item: *const u8,
    item_len: usize,
    output: *mut *mut u8,
    output_len: *mut usize,
) {
    let _item = deserialize_input(item, item_len);
    let mut ts = TokenStream::new();
    ts.push(TokenTree::Literal(Literal::integer(
        "42",
        Span::call_site(),
    )));
    serialize_output(result_into_wire(ts, Vec::new()), output, output_len);
}

unsafe extern "C-unwind" fn panic_macro(
    input: *const u8,
    input_len: usize,
    output: *mut *mut u8,
    output_len: *mut usize,
) {
    let _ = deserialize_input(input, input_len);
    let _ = output;
    let _ = output_len;
    panic!("intentional fixture panic");
}

fn deserialize_input(
    input: *const u8,
    input_len: usize,
) -> yelang_proc_macro_bridge::protocol::token::WireTokenStream {
    let bytes = unsafe { std::slice::from_raw_parts(input, input_len) };
    postcard::from_bytes(bytes).expect("fixture received invalid input")
}

fn serialize_output(result: WireExpansionResult, output: *mut *mut u8, output_len: *mut usize) {
    let bytes = postcard::to_allocvec(&result).expect("fixture failed to serialize output");
    let ptr = unsafe { libc::malloc(bytes.len()) as *mut u8 };
    if ptr.is_null() {
        panic!("fixture out of memory");
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        *output = ptr;
        *output_len = bytes.len();
    }
}

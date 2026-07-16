/*!
 * Conversion between the public proc-macro API and the wire format used for
 * out-of-process expansion.
 */

pub mod run;
pub mod serialize;

pub use run::{
    AttrMacroFn, DeriveMacroFn, FnLikeMacroFn, alloc_output_buffer, free_output_buffer,
    run_attr_macro, run_attr_macro_to_bytes, run_derive_macro, run_derive_macro_to_bytes,
    run_fn_like_macro, run_fn_like_macro_to_bytes,
};
pub use serialize::{
    clear_call_site, from_wire, into_wire, result_from_wire, result_into_wire,
    set_call_site_from_wire,
};

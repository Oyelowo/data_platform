//! Invoke proc-macro functions.

use yelang_proc_macro_bridge::protocol::{ProcMacroKind, token::WireDiagnostic};

use super::context::enter;
use super::convert::{core_to_wire, wire_to_core};
use crate::server::library::{LoadedLibrary, MacroResult, ProcMacro};

/// Invoke a function-like macro by index in a loaded library.
pub fn invoke_fn_like(
    library: &LoadedLibrary,
    macro_index: u32,
    input: yelang_proc_macro_bridge::protocol::token::WireTokenStream,
) -> Result<
    (
        yelang_proc_macro_bridge::protocol::token::WireTokenStream,
        Vec<WireDiagnostic>,
    ),
    String,
> {
    let mac = library
        .get_macro(macro_index as usize)
        .ok_or_else(|| "macro index out of bounds".to_string())?;
    if mac.kind() != ProcMacroKind::FunctionLike {
        return Err("macro is not function-like".to_string());
    }
    let input_ctx = wire_to_core(input);
    invoke(mac.as_ref(), |m| m.expand_fn_like(input_ctx.clone()))
}

/// Invoke an attribute macro.
pub fn invoke_attr(
    library: &LoadedLibrary,
    macro_index: u32,
    args: yelang_proc_macro_bridge::protocol::token::WireTokenStream,
    item: yelang_proc_macro_bridge::protocol::token::WireTokenStream,
) -> Result<
    (
        yelang_proc_macro_bridge::protocol::token::WireTokenStream,
        Vec<WireDiagnostic>,
    ),
    String,
> {
    let mac = library
        .get_macro(macro_index as usize)
        .ok_or_else(|| "macro index out of bounds".to_string())?;
    if mac.kind() != ProcMacroKind::Attribute {
        return Err("macro is not an attribute macro".to_string());
    }
    let args_ctx = wire_to_core(args);
    let item_ctx = wire_to_core(item);
    invoke(mac.as_ref(), |m| {
        m.expand_attr(args_ctx.clone(), item_ctx.clone())
    })
}

/// Invoke a derive macro.
pub fn invoke_derive(
    library: &LoadedLibrary,
    macro_index: u32,
    item: yelang_proc_macro_bridge::protocol::token::WireTokenStream,
) -> Result<
    (
        yelang_proc_macro_bridge::protocol::token::WireTokenStream,
        Vec<WireDiagnostic>,
    ),
    String,
> {
    let mac = library
        .get_macro(macro_index as usize)
        .ok_or_else(|| "macro index out of bounds".to_string())?;
    if mac.kind() != ProcMacroKind::Derive {
        return Err("macro is not a derive macro".to_string());
    }
    let item_ctx = wire_to_core(item);
    invoke(mac.as_ref(), |m| m.expand_derive(item_ctx.clone()))
}

fn invoke<F>(
    mac: &dyn ProcMacro,
    f: F,
) -> Result<
    (
        yelang_proc_macro_bridge::protocol::token::WireTokenStream,
        Vec<WireDiagnostic>,
    ),
    String,
>
where
    F: FnOnce(&dyn ProcMacro) -> MacroResult,
{
    let (result, diagnostics) = enter(|| f(mac));
    let output = result?;
    let wire_diagnostics = diagnostics
        .into_iter()
        .map(|d| diagnostic_to_wire(d))
        .collect();
    Ok((core_to_wire(output), wire_diagnostics))
}

fn diagnostic_to_wire(d: yelang_proc_macro::Diagnostic) -> WireDiagnostic {
    WireDiagnostic {
        level: match d.level {
            yelang_proc_macro::Level::Error => {
                yelang_proc_macro_bridge::protocol::token::WireLevel::Error
            }
            yelang_proc_macro::Level::Warning => {
                yelang_proc_macro_bridge::protocol::token::WireLevel::Warning
            }
            yelang_proc_macro::Level::Note => {
                yelang_proc_macro_bridge::protocol::token::WireLevel::Note
            }
            yelang_proc_macro::Level::Help => {
                yelang_proc_macro_bridge::protocol::token::WireLevel::Help
            }
        },
        message: d.message,
        spans: d
            .spans
            .into_iter()
            .map(
                |s| yelang_proc_macro_bridge::protocol::token::WireDiagnosticSpan {
                    span: super::convert::core_span_to_wire(s.span.into()),
                    label: s.label,
                },
            )
            .collect(),
    }
}

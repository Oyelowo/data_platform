//! Expand procedural macro invocations.

use yelang_interner::Interner;
use yelang_proc_macro::bridge::{from_wire, into_wire};
use yelang_proc_macro_bridge::protocol::WireTokenStream;
use yelang_proc_macro_bridge::protocol::token::{WireDiagnostic, WireHygienePayload, WireSpan};
use yelang_proc_macro_bridge::sandbox::Limits;

use super::ResolvedProcMacro;
use crate::error::ExpandError;

/// Convert a compiler-internal token stream to a wire token stream.
///
/// This uses the public `yelang_proc_macro::TokenStream` API as an intermediate
/// step so that interned symbols are resolved to text, delimiters/spacing are
/// mapped correctly, and literals are rendered consistently.
pub fn core_to_wire(
    stream: &yelang_macro_core::TokenStream,
    interner: &Interner,
) -> WireTokenStream {
    let proc_stream = yelang_proc_macro::TokenStream::from_core_stream(stream, interner);
    into_wire(proc_stream)
}

/// Convert a wire token stream back to a compiler-internal token stream.
///
/// Symbols are re-interned in the current compilation context while preserving
/// per-token spans and hygiene contexts from the proc macro output.
pub fn wire_to_core(
    stream: WireTokenStream,
    interner: &Interner,
    _span: yelang_lexer::Span,
) -> Result<yelang_macro_core::TokenStream, ExpandError> {
    let proc_stream = from_wire(stream);
    Ok(proc_stream.into_core_stream_with_interner(interner))
}

/// Expand a procedural macro invocation through the server.
pub fn expand_proc_macro(
    client: &mut super::ProcMacroClient,
    macro_def: &ResolvedProcMacro,
    args: Option<WireTokenStream>,
    item: Option<WireTokenStream>,
    span: yelang_lexer::Span,
    def_site: yelang_lexer::Span,
    hygiene: WireHygienePayload,
    limits: Limits,
) -> Result<(WireTokenStream, Vec<WireDiagnostic>, WireHygienePayload), ExpandError> {
    let call_site = span_to_wire(span);
    let def_site = span_to_wire(def_site);
    use yelang_proc_macro_bridge::protocol::ProcMacroKind;

    match macro_def.kind {
        ProcMacroKind::FunctionLike => {
            let input = args.ok_or_else(|| {
                ExpandError::malformed_macro_args("function-like macro missing input", span)
            })?;
            client
                .expand_fn_like(
                    macro_def.library,
                    macro_def.macro_index,
                    input,
                    call_site,
                    def_site,
                    hygiene,
                    limits,
                )
                .map_err(|e| ExpandError::malformed_macro_args(e.to_string(), span))
        }
        ProcMacroKind::Attribute => {
            let args = args.unwrap_or_else(|| WireTokenStream { trees: Vec::new() });
            let item = item.ok_or_else(|| {
                ExpandError::malformed_macro_args("attribute macro missing item", span)
            })?;
            client
                .expand_attr(
                    macro_def.library,
                    macro_def.macro_index,
                    args,
                    item,
                    call_site,
                    def_site,
                    hygiene,
                    limits,
                )
                .map_err(|e| ExpandError::malformed_macro_args(e.to_string(), span))
        }
        ProcMacroKind::Derive => {
            let item = item.ok_or_else(|| {
                ExpandError::malformed_macro_args("derive macro missing item", span)
            })?;
            client
                .expand_derive(
                    macro_def.library,
                    macro_def.macro_index,
                    item,
                    call_site,
                    def_site,
                    hygiene,
                    limits,
                )
                .map_err(|e| ExpandError::malformed_macro_args(e.to_string(), span))
        }
    }
}

fn span_to_wire(span: yelang_lexer::Span) -> WireSpan {
    WireSpan {
        lo: span.start().absolute as u32,
        hi: span.end().absolute as u32,
        file: span.file_id().raw(),
        syntax_context: span.syntax_context(),
    }
}

/// Convert server diagnostics into expansion errors.
pub fn wire_diagnostics_to_errors(
    diagnostics: &[WireDiagnostic],
    macro_name: &str,
    span: yelang_lexer::Span,
    backtrace: Vec<crate::error::BacktraceFrame>,
) -> Vec<ExpandError> {
    diagnostics
        .iter()
        .map(|diag| {
            let level = match diag.level {
                yelang_proc_macro_bridge::protocol::token::WireLevel::Error => "error",
                yelang_proc_macro_bridge::protocol::token::WireLevel::Warning => "warning",
                yelang_proc_macro_bridge::protocol::token::WireLevel::Note => "note",
                yelang_proc_macro_bridge::protocol::token::WireLevel::Help => "help",
            };
            ExpandError::malformed_macro_args(
                format!(
                    "proc macro `{}` emitted a diagnostic [{}]: {}",
                    macro_name, level, diag.message
                ),
                span,
            )
            .with_backtrace(backtrace.clone())
        })
        .collect()
}

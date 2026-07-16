//! Expand procedural macro invocations.

use yelang_proc_macro_bridge::protocol::{
    ProcMacroKind, WireTokenStream,
    token::{WireDelimiter, WireSpan, WireTokenTree},
};

use super::ProcMacroClient;
use crate::error::ExpandError;

/// Expand a procedural macro invocation.
pub fn expand_proc_macro(
    client: &mut ProcMacroClient,
    macro_def: &super::ProcMacroDef,
    args: Option<WireTokenStream>,
    item: Option<WireTokenStream>,
    span: yelang_lexer::Span,
) -> Result<WireTokenStream, ExpandError> {
    match macro_def.kind {
        ProcMacroKind::FunctionLike => {
            let input = args.ok_or_else(|| {
                ExpandError::malformed_macro_args("function-like macro missing input", span)
            })?;
            client
                .expand_fn_like(macro_def.library, macro_def.macro_index, input)
                .map_err(|e| ExpandError::malformed_macro_args(e.to_string(), span))
        }
        ProcMacroKind::Attribute => {
            let args = args.unwrap_or_else(|| WireTokenStream { trees: Vec::new() });
            let item = item.ok_or_else(|| {
                ExpandError::malformed_macro_args("attribute macro missing item", span)
            })?;
            client
                .expand_attr(macro_def.library, macro_def.macro_index, args, item)
                .map_err(|e| ExpandError::malformed_macro_args(e.to_string(), span))
        }
        ProcMacroKind::Derive => {
            let item = item.ok_or_else(|| {
                ExpandError::malformed_macro_args("derive macro missing item", span)
            })?;
            client
                .expand_derive(macro_def.library, macro_def.macro_index, item)
                .map_err(|e| ExpandError::malformed_macro_args(e.to_string(), span))
        }
    }
}

/// Convert a Yelang AST token stream to a wire token stream.
///
/// Used by the expander when dispatching to the proc-macro server.
#[allow(dead_code)]
pub fn ast_to_wire(stream: &yelang_macro_core::TokenStream) -> WireTokenStream {
    let mut trees = Vec::new();
    for tree in stream.iter() {
        if let Some(t) = ast_tree_to_wire(tree) {
            trees.push(t);
        }
    }
    WireTokenStream { trees }
}

#[allow(dead_code)]
fn ast_tree_to_wire(tree: &yelang_macro_core::TokenTree) -> Option<WireTokenTree> {
    Some(match tree {
        yelang_macro_core::TokenTree::Group(g) => WireTokenTree::Group {
            delimiter: match g.delimiter {
                yelang_macro_core::Delimiter::Parenthesis => WireDelimiter::Parenthesis,
                yelang_macro_core::Delimiter::Brace => WireDelimiter::Brace,
                yelang_macro_core::Delimiter::Bracket => WireDelimiter::Bracket,
                yelang_macro_core::Delimiter::None => WireDelimiter::None,
            },
            span: ast_span_to_wire(g.span),
            trees: ast_to_wire(&g.stream).trees,
        },
        yelang_macro_core::TokenTree::Ident(i) => WireTokenTree::Ident {
            // We cannot resolve the symbol here without an interner.
            text: format!("<symbol:{}>", i.sym.as_usize()),
            span: ast_span_to_wire(i.span),
            is_raw: i.is_raw,
        },
        yelang_macro_core::TokenTree::Punct(p) => WireTokenTree::Punct {
            ch: p.ch,
            spacing: match p.spacing {
                yelang_macro_core::Spacing::Alone => {
                    yelang_proc_macro_bridge::protocol::token::WireSpacing::Alone
                }
                yelang_macro_core::Spacing::Joint => {
                    yelang_proc_macro_bridge::protocol::token::WireSpacing::Joint
                }
            },
            span: ast_span_to_wire(p.span),
        },
        yelang_macro_core::TokenTree::Literal(l) => WireTokenTree::Literal {
            text: format!("{}", l),
            kind: match &l.kind {
                yelang_macro_core::LitKind::Int { .. } => {
                    yelang_proc_macro_bridge::protocol::token::WireLitKind::Int
                }
                yelang_macro_core::LitKind::Float { .. } => {
                    yelang_proc_macro_bridge::protocol::token::WireLitKind::Float
                }
                yelang_macro_core::LitKind::Str { .. } => {
                    yelang_proc_macro_bridge::protocol::token::WireLitKind::Str
                }
                yelang_macro_core::LitKind::Char(_) => {
                    yelang_proc_macro_bridge::protocol::token::WireLitKind::Char
                }
                yelang_macro_core::LitKind::Bool(_) => {
                    yelang_proc_macro_bridge::protocol::token::WireLitKind::Bool
                }
            },
            span: ast_span_to_wire(l.span),
        },
    })
}

#[allow(dead_code)]
fn ast_span_to_wire(span: yelang_macro_core::Span) -> WireSpan {
    WireSpan {
        lo: span.lo,
        hi: span.hi,
        file: span.file.raw(),
        syntax_context: 0,
    }
}

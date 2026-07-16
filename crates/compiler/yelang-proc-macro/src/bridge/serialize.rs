//! Convert between `TokenStream` and `WireTokenStream`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use yelang_proc_macro_bridge::protocol::token::{
    WireDelimiter, WireDiagnostic, WireDiagnosticSpan, WireExpansionResult, WireExpnData,
    WireExpnKind, WireHygienePayload, WireLevel, WireLitKind, WireSpacing, WireSpan,
    WireSyntaxContext, WireTokenStream, WireTokenTree, WireTransparency,
};

use crate::api::diagnostic::DiagnosticSpan;
use crate::{
    Delimiter, Diagnostic, Group, Ident, Level, Literal, Punct, Spacing, Span, TokenStream,
    TokenTree,
};

/// Convert a `WireTokenStream` (from the proc-macro server) into a public API
/// `TokenStream`.
pub fn from_wire(stream: WireTokenStream) -> TokenStream {
    stream
        .trees
        .into_iter()
        .filter_map(tree_from_wire)
        .collect()
}

/// Set the thread-local call-site span from its wire representation.
///
/// This is called by the proc-macro server before invoking the user macro so
/// that `Span::call_site()` returns the actual invocation site.
pub fn set_call_site_from_wire(span: WireSpan) {
    Span::set_call_site(span_from_wire(span));
}

/// Clear the thread-local call-site span after a macro invocation finishes.
pub fn clear_call_site() {
    Span::clear_call_site();
}

/// Set the thread-local definition-site span from its wire representation.
pub fn set_def_site_from_wire(span: WireSpan) {
    Span::set_def_site(span_from_wire(span));
}

/// Set the thread-local mixed-site span from its wire representation.
pub fn set_mixed_site_from_wire(span: WireSpan) {
    Span::set_mixed_site(span_from_wire(span));
}

/// Clear all thread-local site spans.
pub fn clear_sites() {
    Span::clear_sites();
}

thread_local! {
    /// Hygiene payload for the tokens currently being expanded, sent by the
    /// compiler. The output payload is built from the contexts actually used in
    /// the macro output plus their ancestor chains.
    static INPUT_HYGIENE: RefCell<Option<WireHygienePayload>> = const { RefCell::new(None) };
}

/// Load the input hygiene payload for the current macro invocation.
pub fn set_hygiene_from_wire(hygiene: WireHygienePayload) {
    INPUT_HYGIENE.with(|c| *c.borrow_mut() = Some(hygiene));
}

/// Clear the input hygiene payload after the macro invocation finishes.
pub fn clear_hygiene() {
    INPUT_HYGIENE.with(|c| *c.borrow_mut() = None);
}

/// Convert a public API `TokenStream` into a `WireTokenStream` for sending to
/// the proc-macro server.
pub fn into_wire(stream: TokenStream) -> WireTokenStream {
    WireTokenStream {
        trees: stream.into_iter().filter_map(tree_into_wire).collect(),
    }
}

fn tree_from_wire(tree: WireTokenTree) -> Option<TokenTree> {
    Some(match tree {
        WireTokenTree::Group {
            delimiter,
            span,
            trees,
        } => TokenTree::Group(Group::new(
            delimiter_from_wire(delimiter),
            from_wire(WireTokenStream { trees }),
            span_from_wire(span),
        )),
        WireTokenTree::Ident { text, span, is_raw } => {
            let mut ident = Ident::new(text, span_from_wire(span));
            ident.inner.is_raw = is_raw;
            TokenTree::Ident(ident)
        }
        WireTokenTree::Punct { ch, spacing, span } => TokenTree::Punct(Punct::new(
            ch,
            spacing_from_wire(spacing),
            span_from_wire(span),
        )),
        WireTokenTree::Literal { text, kind, span } => {
            TokenTree::Literal(literal_from_wire(text, kind, span_from_wire(span)))
        }
    })
}

fn tree_into_wire(tree: TokenTree) -> Option<WireTokenTree> {
    Some(match tree {
        TokenTree::Group(group) => WireTokenTree::Group {
            delimiter: delimiter_into_wire(group.delimiter),
            span: span_into_wire(group.span),
            trees: into_wire(group.stream).trees,
        },
        TokenTree::Ident(ident) => WireTokenTree::Ident {
            text: ident.value().to_string(),
            span: span_into_wire(ident.span()),
            is_raw: ident.inner.is_raw,
        },
        TokenTree::Punct(punct) => WireTokenTree::Punct {
            ch: punct.inner.ch,
            spacing: spacing_into_wire(punct.inner.spacing),
            span: span_into_wire(punct.span()),
        },
        TokenTree::Literal(lit) => {
            let span = span_into_wire(lit.span());
            WireTokenTree::Literal {
                text: lit.cached,
                kind: lit_kind_into_wire(&lit.inner.kind),
                span,
            }
        }
    })
}

fn span_from_wire(span: WireSpan) -> Span {
    // A file or syntax context of 0 on the wire means "no specific context"
    // (e.g. a synthesized call-site span). Map 0 to 1 because the internal ID
    // types reject 0.
    let file = if span.file == 0 { 1 } else { span.file };
    let syntax_context = if span.syntax_context == 0 {
        1
    } else {
        span.syntax_context
    };
    Span::from_inner(yelang_macro_core::Span::new(
        span.lo,
        span.hi,
        yelang_lexer::FileId::new(file),
        yelang_macro_core::SyntaxContextId::new(syntax_context),
    ))
}

fn span_into_wire(span: Span) -> WireSpan {
    let inner = span.into_inner();
    WireSpan {
        lo: inner.lo,
        hi: inner.hi,
        file: inner.file.raw(),
        syntax_context: inner.ctx.raw(),
    }
}

fn delimiter_from_wire(d: WireDelimiter) -> Delimiter {
    match d {
        WireDelimiter::Parenthesis => Delimiter::Parenthesis,
        WireDelimiter::Brace => Delimiter::Brace,
        WireDelimiter::Bracket => Delimiter::Bracket,
        WireDelimiter::None => Delimiter::None,
    }
}

fn delimiter_into_wire(d: Delimiter) -> WireDelimiter {
    match d {
        Delimiter::Parenthesis => WireDelimiter::Parenthesis,
        Delimiter::Brace => WireDelimiter::Brace,
        Delimiter::Bracket => WireDelimiter::Bracket,
        Delimiter::None => WireDelimiter::None,
    }
}

fn spacing_from_wire(s: WireSpacing) -> Spacing {
    match s {
        WireSpacing::Alone => Spacing::Alone,
        WireSpacing::Joint => Spacing::Joint,
    }
}

fn spacing_into_wire(s: Spacing) -> WireSpacing {
    match s {
        Spacing::Alone => WireSpacing::Alone,
        Spacing::Joint => WireSpacing::Joint,
    }
}

fn literal_from_wire(text: String, kind: WireLitKind, span: Span) -> Literal {
    match kind {
        WireLitKind::Int => Literal::integer(text, span),
        WireLitKind::Float => Literal::float(text, span),
        WireLitKind::Str => Literal::string(text, span),
        WireLitKind::Char => Literal::character(text.chars().next().unwrap_or('\0'), span),
        WireLitKind::Bool => Literal::boolean(text == "true", span),
        WireLitKind::ByteStr => Literal::byte_string(text, span),
        WireLitKind::Byte => Literal::byte(text.parse().unwrap_or(0), span),
    }
}

fn lit_kind_into_wire(kind: &yelang_macro_core::LitKind) -> WireLitKind {
    match kind {
        yelang_macro_core::LitKind::Int { .. } => WireLitKind::Int,
        yelang_macro_core::LitKind::Float { .. } => WireLitKind::Float,
        yelang_macro_core::LitKind::Str { .. } => WireLitKind::Str,
        yelang_macro_core::LitKind::Char(_) => WireLitKind::Char,
        yelang_macro_core::LitKind::Bool(_) => WireLitKind::Bool,
        yelang_macro_core::LitKind::ByteStr { .. } => WireLitKind::ByteStr,
        yelang_macro_core::LitKind::Byte(_) => WireLitKind::Byte,
    }
}

/// Convert a `WireExpansionResult` from the server into the public API
/// `(TokenStream, Vec<Diagnostic>, hygiene)` triple.
pub fn result_from_wire(
    result: WireExpansionResult,
) -> (TokenStream, Vec<Diagnostic>, WireHygienePayload) {
    (
        from_wire(result.output),
        result
            .diagnostics
            .into_iter()
            .map(diagnostic_from_wire)
            .collect(),
        result.hygiene,
    )
}

/// Convert a public API `(TokenStream, Vec<Diagnostic>)` pair into a
/// `WireExpansionResult` for sending to the server.
pub fn result_into_wire(output: TokenStream, diagnostics: Vec<Diagnostic>) -> WireExpansionResult {
    let output = into_wire(output);
    let hygiene = build_output_hygiene(&output);
    WireExpansionResult {
        output,
        diagnostics: diagnostics.into_iter().map(diagnostic_into_wire).collect(),
        hygiene,
    }
}

/// Build a hygiene payload describing every syntax context used in `output`.
///
/// The output payload contains the subset of the input payload reachable from
/// the contexts that appear on output tokens, including all ancestor contexts
/// and their associated expansion data.
fn build_output_hygiene(output: &WireTokenStream) -> WireHygienePayload {
    let mut used = HashSet::new();
    collect_syntax_contexts_from_stream(output, &mut used);

    let input = INPUT_HYGIENE
        .with(|c| c.borrow().clone())
        .unwrap_or_default();
    let context_map: HashMap<u32, WireSyntaxContext> =
        input.contexts.iter().map(|c| (c.id, *c)).collect();
    let expansion_map: HashMap<u64, WireExpnData> =
        input.expansions.iter().map(|e| (e.id, *e)).collect();

    let mut contexts = Vec::new();
    let mut expansions = Vec::new();
    let mut seen_ctx = HashSet::new();
    let mut seen_expn = HashSet::new();

    fn add_context(
        id: u32,
        context_map: &HashMap<u32, WireSyntaxContext>,
        expansion_map: &HashMap<u64, WireExpnData>,
        contexts: &mut Vec<WireSyntaxContext>,
        expansions: &mut Vec<WireExpnData>,
        seen_ctx: &mut HashSet<u32>,
        seen_expn: &mut HashSet<u64>,
    ) {
        if id == 0 || !seen_ctx.insert(id) {
            return;
        }

        let ctx = context_map.get(&id).copied().unwrap_or(WireSyntaxContext {
            id,
            parent: None,
            outer_expn: None,
            transparency: WireTransparency::Opaque,
        });
        contexts.push(ctx);

        if let Some(parent) = ctx.parent {
            add_context(
                parent,
                context_map,
                expansion_map,
                contexts,
                expansions,
                seen_ctx,
                seen_expn,
            );
        }
        if let Some(outer_expn) = ctx.outer_expn {
            add_expn(
                outer_expn,
                expansion_map,
                contexts,
                expansions,
                seen_ctx,
                seen_expn,
                context_map,
            );
        }
    }

    fn add_expn(
        id: u64,
        expansion_map: &HashMap<u64, WireExpnData>,
        contexts: &mut Vec<WireSyntaxContext>,
        expansions: &mut Vec<WireExpnData>,
        seen_ctx: &mut HashSet<u32>,
        seen_expn: &mut HashSet<u64>,
        context_map: &HashMap<u32, WireSyntaxContext>,
    ) {
        if id == 0 || !seen_expn.insert(id) {
            return;
        }

        let expn = expansion_map.get(&id).copied().unwrap_or(WireExpnData {
            id,
            parent: 0,
            call_site: WireSpan {
                lo: 0,
                hi: 0,
                file: 0,
                syntax_context: 1,
            },
            def_site: WireSpan {
                lo: 0,
                hi: 0,
                file: 0,
                syntax_context: 1,
            },
            kind: WireExpnKind::Macro,
        });
        expansions.push(expn);

        if expn.parent != 0 {
            add_expn(
                expn.parent,
                expansion_map,
                contexts,
                expansions,
                seen_ctx,
                seen_expn,
                context_map,
            );
        }
        add_context(
            expn.call_site.syntax_context,
            context_map,
            expansion_map,
            contexts,
            expansions,
            seen_ctx,
            seen_expn,
        );
        add_context(
            expn.def_site.syntax_context,
            context_map,
            expansion_map,
            contexts,
            expansions,
            seen_ctx,
            seen_expn,
        );
    }

    for id in used {
        add_context(
            id,
            &context_map,
            &expansion_map,
            &mut contexts,
            &mut expansions,
            &mut seen_ctx,
            &mut seen_expn,
        );
    }

    WireHygienePayload {
        contexts,
        expansions,
    }
}

fn collect_syntax_contexts_from_stream(stream: &WireTokenStream, out: &mut HashSet<u32>) {
    for tree in &stream.trees {
        collect_syntax_contexts_from_tree(tree, out);
    }
}

fn collect_syntax_contexts_from_tree(tree: &WireTokenTree, out: &mut HashSet<u32>) {
    match tree {
        WireTokenTree::Group { span, trees, .. } => {
            out.insert(span.syntax_context);
            collect_syntax_contexts_from_stream(
                &WireTokenStream {
                    trees: trees.clone(),
                },
                out,
            );
        }
        WireTokenTree::Ident { span, .. }
        | WireTokenTree::Punct { span, .. }
        | WireTokenTree::Literal { span, .. } => {
            out.insert(span.syntax_context);
        }
    }
}

fn diagnostic_from_wire(d: WireDiagnostic) -> Diagnostic {
    Diagnostic {
        level: level_from_wire(d.level),
        message: d.message,
        spans: d.spans.into_iter().map(diagnostic_span_from_wire).collect(),
    }
}

fn diagnostic_into_wire(d: Diagnostic) -> WireDiagnostic {
    WireDiagnostic {
        level: level_into_wire(d.level),
        message: d.message,
        spans: d.spans.into_iter().map(diagnostic_span_into_wire).collect(),
    }
}

fn diagnostic_span_from_wire(s: WireDiagnosticSpan) -> DiagnosticSpan {
    DiagnosticSpan {
        span: span_from_wire(s.span),
        label: s.label,
    }
}

fn diagnostic_span_into_wire(s: DiagnosticSpan) -> WireDiagnosticSpan {
    WireDiagnosticSpan {
        span: span_into_wire(s.span),
        label: s.label,
    }
}

fn level_from_wire(level: WireLevel) -> Level {
    match level {
        WireLevel::Error => Level::Error,
        WireLevel::Warning => Level::Warning,
        WireLevel::Note => Level::Note,
        WireLevel::Help => Level::Help,
    }
}

fn level_into_wire(level: Level) -> WireLevel {
    match level {
        Level::Error => WireLevel::Error,
        Level::Warning => WireLevel::Warning,
        Level::Note => WireLevel::Note,
        Level::Help => WireLevel::Help,
    }
}

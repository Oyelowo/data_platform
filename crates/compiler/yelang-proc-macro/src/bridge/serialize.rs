//! Convert between `TokenStream` and `WireTokenStream`.

use yelang_proc_macro_bridge::protocol::token::{
    WireDelimiter, WireDiagnostic, WireDiagnosticSpan, WireExpansionResult, WireLevel, WireLitKind,
    WireSpacing, WireSpan, WireTokenStream, WireTokenTree,
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
    Span::from_inner(yelang_macro_core::Span::new(
        span.lo,
        span.hi,
        yelang_lexer::FileId::new(span.file),
        yelang_macro_core::SyntaxContextId::new(span.syntax_context),
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
/// `(TokenStream, Vec<Diagnostic>)` pair.
pub fn result_from_wire(result: WireExpansionResult) -> (TokenStream, Vec<Diagnostic>) {
    (
        from_wire(result.output),
        result
            .diagnostics
            .into_iter()
            .map(diagnostic_from_wire)
            .collect(),
    )
}

/// Convert a public API `(TokenStream, Vec<Diagnostic>)` pair into a
/// `WireExpansionResult` for sending to the server.
pub fn result_into_wire(output: TokenStream, diagnostics: Vec<Diagnostic>) -> WireExpansionResult {
    WireExpansionResult {
        output: into_wire(output),
        diagnostics: diagnostics.into_iter().map(diagnostic_into_wire).collect(),
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

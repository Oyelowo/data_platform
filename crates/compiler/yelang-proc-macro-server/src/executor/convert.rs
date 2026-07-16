//! Convert between wire token streams and core token streams.

use yelang_macro_core::{
    Delimiter, Group, Ident, Literal, Punct, Span, TokenId, TokenStream, TokenTree,
};
use yelang_proc_macro_bridge::protocol::token::{
    WireDelimiter, WireLevel, WireLitKind, WireSpacing, WireSpan, WireTokenStream, WireTokenTree,
};

use crate::server::library::TokenStreamAndContext;

pub fn wire_to_core(stream: WireTokenStream) -> TokenStreamAndContext {
    let mut out = TokenStream::new();
    let mut call_site = Span::default();
    for tree in stream.trees {
        if let Some((tt, span)) = wire_tree_to_core(tree) {
            call_site = call_site.merged(span);
            out.push(tt);
        }
    }
    (out, call_site)
}

fn wire_tree_to_core(tree: WireTokenTree) -> Option<(TokenTree, Span)> {
    Some(match tree {
        WireTokenTree::Group {
            delimiter,
            span,
            trees,
        } => {
            let (inner, _) = wire_to_core(WireTokenStream { trees });
            let span = wire_span_to_core(span);
            let group = Group::new(wire_delimiter_to_core(delimiter), inner, span);
            (TokenTree::Group(group), span)
        }
        WireTokenTree::Ident { text, span, is_raw } => {
            let span = wire_span_to_core(span);
            let sym = with_server_interner(|i| i.get_or_intern(&text));
            let mut ident = Ident::new(sym, span);
            ident.is_raw = is_raw;
            (TokenTree::Ident(ident), span)
        }
        WireTokenTree::Punct { ch, spacing, span } => {
            let span = wire_span_to_core(span);
            let punct = Punct::new(ch, wire_spacing_to_core(spacing), span);
            (TokenTree::Punct(punct), span)
        }
        WireTokenTree::Literal { text, kind, span } => {
            let span = wire_span_to_core(span);
            let lit = wire_literal_to_core(&text, kind, span);
            (TokenTree::Literal(lit), span)
        }
    })
}

fn wire_span_to_core(span: WireSpan) -> Span {
    Span::new(
        span.lo,
        span.hi,
        yelang_lexer::FileId::new(span.file),
        yelang_macro_core::SyntaxContextId::from_arena_key(
            yelang_arena::ArenaKey::default(), // placeholder
        ),
    )
}

fn wire_delimiter_to_core(d: WireDelimiter) -> Delimiter {
    match d {
        WireDelimiter::Parenthesis => Delimiter::Parenthesis,
        WireDelimiter::Brace => Delimiter::Brace,
        WireDelimiter::Bracket => Delimiter::Bracket,
        WireDelimiter::None => Delimiter::None,
    }
}

fn wire_spacing_to_core(s: WireSpacing) -> yelang_macro_core::Spacing {
    match s {
        WireSpacing::Alone => yelang_macro_core::Spacing::Alone,
        WireSpacing::Joint => yelang_macro_core::Spacing::Joint,
    }
}

fn wire_literal_to_core(text: &str, kind: WireLitKind, span: Span) -> Literal {
    match kind {
        WireLitKind::Int => {
            let sym = with_server_interner(|i| i.get_or_intern(text));
            Literal::int(sym, span)
        }
        WireLitKind::Float => {
            let sym = with_server_interner(|i| i.get_or_intern(text));
            Literal::float(sym, span)
        }
        WireLitKind::Str => {
            let sym = with_server_interner(|i| i.get_or_intern(text));
            Literal::string(sym, span)
        }
        WireLitKind::Char => {
            let ch = text.chars().next().unwrap_or('\0');
            Literal::char(ch, span)
        }
        WireLitKind::Bool => {
            let value = text == "true";
            Literal::bool(value, span)
        }
    }
}

pub fn core_to_wire(stream: TokenStream) -> WireTokenStream {
    let mut trees = Vec::new();
    for tree in stream.iter() {
        if let Some(t) = core_tree_to_wire(tree.clone()) {
            trees.push(t);
        }
    }
    WireTokenStream { trees }
}

fn core_tree_to_wire(tree: TokenTree) -> Option<WireTokenTree> {
    Some(match tree {
        TokenTree::Group(g) => WireTokenTree::Group {
            delimiter: core_delimiter_to_wire(g.delimiter),
            span: core_span_to_wire(g.span),
            trees: core_to_wire(g.stream).trees,
        },
        TokenTree::Ident(i) => WireTokenTree::Ident {
            text: with_server_interner(|interner| interner.resolve(&i.sym).to_string()),
            span: core_span_to_wire(i.span),
            is_raw: i.is_raw,
        },
        TokenTree::Punct(p) => WireTokenTree::Punct {
            ch: p.ch,
            spacing: core_spacing_to_wire(p.spacing),
            span: core_span_to_wire(p.span),
        },
        TokenTree::Literal(l) => WireTokenTree::Literal {
            text: render_literal(&l),
            kind: core_lit_kind_to_wire(&l.kind),
            span: core_span_to_wire(l.span),
        },
    })
}

pub(crate) fn core_span_to_wire(span: Span) -> WireSpan {
    WireSpan {
        lo: span.lo,
        hi: span.hi,
        file: span.file.raw(),
        syntax_context: 0, // TODO: proper hygiene serialization
    }
}

fn core_delimiter_to_wire(d: Delimiter) -> WireDelimiter {
    match d {
        Delimiter::Parenthesis => WireDelimiter::Parenthesis,
        Delimiter::Brace => WireDelimiter::Brace,
        Delimiter::Bracket => WireDelimiter::Bracket,
        Delimiter::None => WireDelimiter::None,
    }
}

fn core_spacing_to_wire(s: yelang_macro_core::Spacing) -> WireSpacing {
    match s {
        yelang_macro_core::Spacing::Alone => WireSpacing::Alone,
        yelang_macro_core::Spacing::Joint => WireSpacing::Joint,
    }
}

fn core_lit_kind_to_wire(kind: &yelang_macro_core::LitKind) -> WireLitKind {
    match kind {
        yelang_macro_core::LitKind::Int { .. } => WireLitKind::Int,
        yelang_macro_core::LitKind::Float { .. } => WireLitKind::Float,
        yelang_macro_core::LitKind::Str { .. } => WireLitKind::Str,
        yelang_macro_core::LitKind::Char(_) => WireLitKind::Char,
        yelang_macro_core::LitKind::Bool(_) => WireLitKind::Bool,
    }
}

fn render_literal(lit: &Literal) -> String {
    use yelang_macro_core::LitKind;
    match &lit.kind {
        LitKind::Int { value, suffix } => {
            let mut s = with_server_interner(|i| i.resolve(value).to_string());
            if let Some(suf) = suffix {
                s.push_str(suf);
            }
            s
        }
        LitKind::Float { value, suffix } => {
            let mut s = with_server_interner(|i| i.resolve(value).to_string());
            if let Some(suf) = suffix {
                s.push_str(suf);
            }
            s
        }
        LitKind::Str { value, .. } => with_server_interner(|i| i.resolve(value).to_string()),
        LitKind::Char(c) => c.to_string(),
        LitKind::Bool(b) => b.to_string(),
    }
}

thread_local! {
    static SERVER_INTERNER: std::cell::RefCell<yelang_interner::Interner> =
        std::cell::RefCell::new(yelang_interner::Interner::new());
}

pub(crate) fn with_server_interner<R>(f: impl FnOnce(&yelang_interner::Interner) -> R) -> R {
    SERVER_INTERNER.with(|i| f(&*i.borrow()))
}

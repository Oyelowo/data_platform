//! Token streams.

use std::fmt;

use super::{Delimiter, TokenTree};

/// A sequence of token trees.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TokenStream {
    trees: Vec<TokenTree>,
}

impl TokenStream {
    /// Create an empty stream.
    pub fn new() -> Self {
        Self { trees: Vec::new() }
    }

    /// True if the stream contains no tokens.
    pub fn is_empty(&self) -> bool {
        self.trees.is_empty()
    }

    /// Number of top-level token trees.
    pub fn len(&self) -> usize {
        self.trees.len()
    }

    /// Append a token tree.
    pub fn push(&mut self, tree: TokenTree) {
        self.trees.push(tree);
    }

    /// Extend with another stream.
    pub fn extend(&mut self, other: TokenStream) {
        self.trees.extend(other.trees);
    }

    /// Iterate over token trees.
    pub fn iter(&self) -> Iter<'_> {
        Iter {
            inner: self.trees.iter(),
        }
    }

    /// Convert from a compiler-internal token stream, resolving symbol text with
    /// the provided interner.
    ///
    /// This is the entry point used by the compiler when it hands tokens to a
    /// procedural macro. It guarantees that every token carries usable textual
    /// source data, avoiding cross-interner symbol mismatches.
    pub fn from_core_stream(
        stream: &yelang_macro_core::TokenStream,
        interner: &yelang_interner::Interner,
    ) -> Self {
        let mut trees = Vec::new();
        for tree in stream.iter() {
            if let Some(t) = tree_to_proc_macro(tree, interner) {
                trees.push(t);
            }
        }
        Self { trees }
    }

    /// Convert back into a compiler-internal token stream.
    pub fn into_core_stream(self) -> yelang_macro_core::TokenStream {
        let mut inner = yelang_macro_core::TokenStream::new();
        for tree in self.trees {
            inner.push(tree.into_inner());
        }
        inner
    }

    /// Render this stream to source text using the provided interner for any
    /// symbol-backed tokens.
    ///
    /// Tokens created through the public API carry cached source text, so the
    /// interner is only consulted for tokens that originated from a core stream
    /// and were converted without going through the public constructors.
    pub fn render_source(&self, interner: &yelang_interner::Interner) -> String {
        let mut out = String::new();
        let mut prev: Option<String> = None;
        for tree in &self.trees {
            let s = render_tree(tree, interner);
            if let Some(ref p) = prev {
                if needs_space(p, &s) {
                    out.push(' ');
                }
            }
            out.push_str(&s);
            prev = Some(s);
        }
        out
    }
}

impl From<TokenTree> for TokenStream {
    fn from(tree: TokenTree) -> Self {
        let mut s = Self::new();
        s.push(tree);
        s
    }
}

impl FromIterator<TokenTree> for TokenStream {
    fn from_iter<I: IntoIterator<Item = TokenTree>>(iter: I) -> Self {
        let mut s = Self::new();
        for tree in iter {
            s.push(tree);
        }
        s
    }
}

impl fmt::Display for TokenStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, tree) in self.iter().enumerate() {
            if i > 0 {
                write!(f, " ")?;
            }
            write!(f, "{}", tree)?;
        }
        Ok(())
    }
}

/// Iterator over token trees in a stream.
pub struct Iter<'a> {
    inner: std::slice::Iter<'a, TokenTree>,
}

impl<'a> Iterator for Iter<'a> {
    type Item = TokenTree;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().cloned()
    }
}

fn tree_to_proc_macro(
    tree: &yelang_macro_core::TokenTree,
    interner: &yelang_interner::Interner,
) -> Option<TokenTree> {
    Some(match tree {
        yelang_macro_core::TokenTree::Group(g) => TokenTree::Group(super::Group::new(
            g.delimiter,
            TokenStream::from_core_stream(&g.stream, interner),
            super::Span::from_inner(g.span),
        )),
        yelang_macro_core::TokenTree::Ident(i) => {
            let text = interner.resolve(&i.sym).to_string();
            TokenTree::Ident(super::Ident::new(text, super::Span::from_inner(i.span)))
        }
        yelang_macro_core::TokenTree::Punct(p) => TokenTree::Punct(super::Punct::new(
            p.ch,
            p.spacing,
            super::Span::from_inner(p.span),
        )),
        yelang_macro_core::TokenTree::Literal(l) => {
            TokenTree::Literal(core_literal_to_proc_macro(l, interner))
        }
    })
}

fn core_literal_to_proc_macro(
    lit: &yelang_macro_core::Literal,
    interner: &yelang_interner::Interner,
) -> super::Literal {
    use yelang_macro_core::LitKind;
    let span = super::Span::from_inner(lit.span);
    match &lit.kind {
        LitKind::Int { value, suffix } => {
            let mut text = interner.resolve(value).to_string();
            if let Some(suffix) = suffix {
                text.push_str(suffix);
            }
            super::Literal::integer(text, span)
        }
        LitKind::Float { value, suffix } => {
            let mut text = interner.resolve(value).to_string();
            if let Some(suffix) = suffix {
                text.push_str(suffix);
            }
            super::Literal::float(text, span)
        }
        LitKind::Str { value, kind } => {
            let text = interner.resolve(value).to_string();
            match kind {
                yelang_macro_core::StrKind::Normal => super::Literal::string(text, span),
                yelang_macro_core::StrKind::Raw(n) => super::Literal::raw_string(text, *n, span),
            }
        }
        LitKind::Char(c) => super::Literal::character(*c, span),
        LitKind::Bool(b) => super::Literal::boolean(*b, span),
    }
}

fn render_tree(tree: &TokenTree, interner: &yelang_interner::Interner) -> String {
    match tree {
        TokenTree::Group(g) => render_group(g, interner),
        TokenTree::Ident(i) => i.value().to_string(),
        TokenTree::Punct(p) => p.as_char().to_string(),
        TokenTree::Literal(l) => render_literal(l, interner),
    }
}

fn render_group(group: &super::Group, interner: &yelang_interner::Interner) -> String {
    let (open, close) = match group.delimiter() {
        Delimiter::Parenthesis => ("(", ")"),
        Delimiter::Brace => ("{", "}"),
        Delimiter::Bracket => ("[", "]"),
        Delimiter::None => ("", ""),
    };
    format!(
        "{}{}{}",
        open,
        group.stream().render_source(interner),
        close
    )
}

fn render_literal(lit: &super::Literal, interner: &yelang_interner::Interner) -> String {
    // Public constructors always set cached text; prefer it. If a token somehow
    // arrived without cached text, fall back to the core renderer.
    if !lit.cached.is_empty() {
        return lit.cached.clone();
    }
    yelang_macro_core::token_tree::render::render_literal(&lit.inner, interner)
}

/// Decide whether a space is needed between two rendered token strings.
fn needs_space(prev: &str, next: &str) -> bool {
    let prev_last = prev.chars().last().unwrap_or(' ');
    let next_first = next.chars().next().unwrap_or(' ');

    let prev_is_word_like = prev_last.is_alphanumeric()
        || prev_last == '_'
        || prev_last == '"'
        || prev_last == '\''
        || prev_last == ')'
        || prev_last == ']'
        || prev_last == '}';
    let next_is_word_like = next_first.is_alphanumeric()
        || next_first == '_'
        || next_first == '"'
        || next_first == '\'';

    let prev_is_lonely_separator = prev == "," || prev == ";";

    (prev_is_word_like || prev_is_lonely_separator) && next_is_word_like
}

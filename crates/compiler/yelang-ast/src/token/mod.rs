pub mod convert;
pub mod group;
pub mod ident;
pub mod literal;
pub mod punct;
pub mod span;
pub mod token_id;

pub use group::{Delimiter, Group};
pub use ident::Ident;
pub use literal::{LitKind, Literal, StrKind};
pub use punct::{Punct, Spacing};
pub use span::Span;
pub use token_id::TokenId;

use std::fmt;

use yelang_interner::Interner;

/// A stream of tokens produced by the lexer or by a macro expansion.
///
/// This is the universal currency of macro expansion. It can be converted from
/// lexer tokens, parsed into AST, or produced by `quote!`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TokenStream {
    trees: Vec<TokenTree>,
}

impl TokenStream {
    pub fn new() -> Self {
        Self { trees: Vec::new() }
    }

    pub fn from_vec(trees: Vec<TokenTree>) -> Self {
        Self { trees }
    }

    pub fn is_empty(&self) -> bool {
        self.trees.is_empty()
    }

    pub fn len(&self) -> usize {
        self.trees.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &TokenTree> {
        self.trees.iter()
    }

    pub fn into_iter(self) -> impl Iterator<Item = TokenTree> {
        self.trees.into_iter()
    }

    pub fn push(&mut self, tree: TokenTree) {
        self.trees.push(tree);
    }

    pub fn extend(&mut self, other: TokenStream) {
        self.trees.extend(other.trees);
    }

    pub fn trees(&self) -> &[TokenTree] {
        &self.trees
    }

    pub fn trees_mut(&mut self) -> &mut Vec<TokenTree> {
        &mut self.trees
    }
}

impl From<TokenTree> for TokenStream {
    fn from(tree: TokenTree) -> Self {
        Self::from_vec(vec![tree])
    }
}

impl FromIterator<TokenTree> for TokenStream {
    fn from_iter<I: IntoIterator<Item = TokenTree>>(iter: I) -> Self {
        Self::from_vec(iter.into_iter().collect())
    }
}

/// A single token or delimited group.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenTree {
    Group(Group),
    Ident(Ident),
    Punct(Punct),
    Literal(Literal),
}

impl TokenTree {
    pub fn span(&self) -> Span {
        match self {
            TokenTree::Group(g) => g.span,
            TokenTree::Ident(i) => i.span,
            TokenTree::Punct(p) => p.span,
            TokenTree::Literal(l) => l.span,
        }
    }

    pub fn token_id(&self) -> TokenId {
        match self {
            TokenTree::Group(g) => g.id,
            TokenTree::Ident(i) => i.id,
            TokenTree::Punct(p) => p.id,
            TokenTree::Literal(l) => l.id,
        }
    }

    pub fn set_span(&mut self, span: Span) {
        match self {
            TokenTree::Group(g) => g.span = span,
            TokenTree::Ident(i) => i.span = span,
            TokenTree::Punct(p) => p.span = span,
            TokenTree::Literal(l) => l.span = span,
        }
    }
}

impl fmt::Display for TokenStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, tree) in self.trees.iter().enumerate() {
            if i > 0 {
                write!(f, " ")?;
            }
            write!(f, "{}", tree)?;
        }
        Ok(())
    }
}

impl fmt::Display for TokenTree {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenTree::Group(g) => write!(f, "{}", g),
            TokenTree::Ident(i) => write!(f, "{}", i),
            TokenTree::Punct(p) => write!(f, "{}", p),
            TokenTree::Literal(l) => write!(f, "{}", l),
        }
    }
}

impl TokenStream {
    /// Render this token stream to a source string using the interner to resolve
    /// identifier and string-literal symbols.
    pub fn render(&self, interner: &Interner) -> String {
        let mut out = String::new();
        let mut prev: Option<String> = None;
        for tree in &self.trees {
            let s = tree.render(interner);
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

impl TokenTree {
    /// Render this token tree to a source string.
    pub fn render(&self, interner: &Interner) -> String {
        match self {
            TokenTree::Group(g) => render_group(g, interner),
            TokenTree::Ident(i) => render_ident(i, interner),
            TokenTree::Punct(p) => p.ch.to_string(),
            TokenTree::Literal(l) => render_literal(l, interner),
        }
    }
}

fn render_group(group: &Group, interner: &Interner) -> String {
    let (open, close) = match group.delimiter {
        Delimiter::Parenthesis => ("(", ")"),
        Delimiter::Brace => ("{", "}"),
        Delimiter::Bracket => ("[", "]"),
        Delimiter::None => ("", ""),
    };
    let inner = group.stream.render(interner);
    format!("{}{}{}", open, inner, close)
}

fn render_ident(ident: &Ident, interner: &Interner) -> String {
    let name = interner.resolve(&ident.sym);
    if ident.is_raw {
        format!("r#{}", name)
    } else {
        name.to_string()
    }
}

fn render_literal(lit: &Literal, interner: &Interner) -> String {
    match &lit.kind {
        LitKind::Int { value, suffix } => {
            let mut s = interner.resolve(value).to_string();
            if let Some(suffix) = suffix {
                s.push_str(suffix);
            }
            s
        }
        LitKind::Float { value, suffix } => {
            let mut s = interner.resolve(value).to_string();
            if let Some(suffix) = suffix {
                s.push_str(suffix);
            }
            s
        }
        LitKind::Str { value, kind } => {
            let text = interner.resolve(value);
            match kind {
                StrKind::Normal => format!("\"{}\"", text),
                StrKind::Raw(n) => {
                    let hashes = "#".repeat(*n);
                    format!("r{}\"{}\"{}", hashes, text, hashes)
                }
            }
        }
        LitKind::Char(c) => format!("'{}'", c),
        LitKind::Bool(b) => b.to_string(),
    }
}

/// Decide whether a space is needed between two rendered token strings.
fn needs_space(prev: &str, next: &str) -> bool {
    let prev_last = prev.chars().last().unwrap_or(' ');
    let next_first = next.chars().next().unwrap_or(' ');

    // Word-like tokens (identifiers, literals, and closing delimiters) must be
    // separated from word-like following tokens to avoid merging into a single
    // identifier/literal.
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

    prev_is_word_like && next_is_word_like
}

impl crate::Codegen for TokenStream {
    fn codegen(&self, f: &mut dyn fmt::Write, interner: &Interner) -> fmt::Result {
        write!(f, "{}", self.render(interner))
    }
}

impl crate::Codegen for TokenTree {
    fn codegen(&self, f: &mut dyn fmt::Write, interner: &Interner) -> fmt::Result {
        write!(f, "{}", self.render(interner))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yelang_interner::Interner;

    #[test]
    fn token_stream_from_vec() {
        let mut interner = Interner::new();
        let sym = interner.get_or_intern("x");
        let span = Span::default();
        let ident = Ident::new(sym, span);
        let stream = TokenStream::from_vec(vec![TokenTree::Ident(ident)]);
        assert_eq!(stream.len(), 1);
    }

    #[test]
    fn token_stream_display_uses_symbol_indices() {
        let mut interner = Interner::new();
        let sym = interner.get_or_intern("x");
        let span = Span::default();
        let ident = TokenTree::Ident(Ident::new(sym, span));
        let stream = TokenStream::from_vec(vec![ident.clone(), ident]);
        // `Display` does not have access to the interner, so identifiers print
        // their symbol index. Use `TokenStream::render` for source text.
        assert_eq!(format!("{}", stream), "<symbol:0> <symbol:0>");
    }

    #[test]
    fn render_simple_expression() {
        let mut interner = Interner::new();
        let span = Span::default();
        let x = Ident::new(interner.get_or_intern("x"), span);
        let y = Ident::new(interner.get_or_intern("y"), span);
        let stream = TokenStream::from_vec(vec![
            TokenTree::Ident(x),
            TokenTree::Punct(Punct::new('+', Spacing::Alone, span)),
            TokenTree::Ident(y),
        ]);
        assert_eq!(stream.render(&interner), "x+y");
    }

    #[test]
    fn render_compound_operator_round_trips() {
        let mut interner = Interner::new();
        let span = Span::default();
        let x = Ident::new(interner.get_or_intern("x"), span);
        let y = Ident::new(interner.get_or_intern("y"), span);
        let stream = TokenStream::from_vec(vec![
            TokenTree::Ident(x),
            TokenTree::Punct(Punct::new('<', Spacing::Joint, span)),
            TokenTree::Punct(Punct::new('=', Spacing::Alone, span)),
            TokenTree::Ident(y),
        ]);
        assert_eq!(stream.render(&interner), "x<=y");
    }

    #[test]
    fn render_group_with_delimiters() {
        let mut interner = Interner::new();
        let span = Span::default();
        let inner = TokenStream::from_vec(vec![TokenTree::Ident(Ident::new(
            interner.get_or_intern("x"),
            span,
        ))]);
        let group = Group::new(Delimiter::Parenthesis, inner, span);
        let stream = TokenStream::from_vec(vec![TokenTree::Group(group)]);
        assert_eq!(stream.render(&interner), "(x)");
    }

    #[test]
    fn render_string_literal() {
        let mut interner = Interner::new();
        let span = Span::default();
        let lit = Literal::string(interner.get_or_intern("hello"), span);
        let stream = TokenStream::from_vec(vec![TokenTree::Literal(lit)]);
        assert_eq!(stream.render(&interner), "\"hello\"");
    }

    #[test]
    fn token_stream_round_trips_through_lexer() {
        let mut interner = Interner::new();
        let span = Span::default();
        let x = Ident::new(interner.get_or_intern("x"), span);
        let y = Ident::new(interner.get_or_intern("y"), span);
        let original = TokenStream::from_vec(vec![
            TokenTree::Ident(x),
            TokenTree::Punct(Punct::new('+', Spacing::Alone, span)),
            TokenTree::Ident(y),
        ]);

        let rendered = original.render(&interner);
        let mut local_interner = interner.clone();
        let mut lex = crate::TokenKind::tokenize(&rendered, &mut local_interner).unwrap();
        let tokens: Vec<yelang_lexer::Token<crate::tokenizer::TokenKind>> =
            std::iter::from_fn(|| lex.advance().map(|t| t.clone())).collect();
        let round_tripped = crate::token::convert::from_lexer_tokens(&tokens);

        assert_eq!(original.render(&interner), round_tripped.render(&interner));
    }
}

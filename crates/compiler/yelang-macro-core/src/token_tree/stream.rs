use std::fmt;

use yelang_interner::Interner;

use super::{TokenTree, render::needs_space};

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

    /// Render this token stream to a source string using the interner to resolve
    /// identifier and string-literal symbols.
    pub fn render(&self, interner: &Interner) -> String {
        let mut out = String::new();
        let mut prev: Option<String> = None;
        for tree in &self.trees {
            let s = tree.render(interner);
            if let Some(ref p) = prev
                && needs_space(p, &s)
            {
                out.push(' ');
            }
            out.push_str(&s);
            prev = Some(s);
        }
        out
    }
}

impl IntoIterator for TokenStream {
    type Item = TokenTree;
    type IntoIter = std::vec::IntoIter<TokenTree>;

    fn into_iter(self) -> Self::IntoIter {
        self.trees.into_iter()
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

#[cfg(test)]
mod tests {
    use super::super::{Delimiter, Group, Ident, Punct, Spacing, Span, TokenTree};
    use super::*;

    #[test]
    fn token_stream_from_vec() {
        let interner = Interner::new();
        let sym = interner.get_or_intern("x");
        let span = Span::default();
        let ident = Ident::new(sym, span);
        let stream = TokenStream::from_vec(vec![TokenTree::Ident(ident)]);
        assert_eq!(stream.len(), 1);
    }

    #[test]
    fn token_stream_display_uses_symbol_indices() {
        let interner = Interner::new();
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
        let interner = Interner::new();
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
        let interner = Interner::new();
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
        let interner = Interner::new();
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
        let interner = Interner::new();
        let span = Span::default();
        let lit = super::super::Literal::string(interner.get_or_intern("hello"), span);
        let stream = TokenStream::from_vec(vec![TokenTree::Literal(lit)]);
        assert_eq!(stream.render(&interner), "\"hello\"");
    }
}

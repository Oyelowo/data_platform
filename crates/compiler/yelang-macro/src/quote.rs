/// Quasi-quoting support.
///
/// For now this module provides programmatic helpers for constructing
/// `TokenStream`s. A full `quote!` macro will be added once the token API is
/// stable.
use yelang_interner::Interner;

use yelang_macro_core::token_tree::{
    Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree,
};

/// Create an identifier token.
pub fn ident(name: &str, span: Span, interner: &Interner) -> Ident {
    Ident::new(interner.get_or_intern(name), span)
}

/// Create a punctuation token.
pub fn punct(ch: char, span: Span) -> Punct {
    Punct::alone(ch, span)
}

/// Create a joint punctuation token.
pub fn punct_joint(ch: char, span: Span) -> Punct {
    Punct::new(ch, Spacing::Joint, span)
}

/// Create an integer literal token.
pub fn int_lit(value: &str, span: Span, interner: &Interner) -> Literal {
    Literal::int(interner.get_or_intern(value), span)
}

/// Create a string literal token.
pub fn str_lit(value: &str, span: Span, interner: &Interner) -> Literal {
    Literal::string(interner.get_or_intern(value), span)
}

/// Create a parenthesized group.
pub fn paren(stream: TokenStream, span: Span) -> Group {
    Group::new(Delimiter::Parenthesis, stream, span)
}

/// Create a braced group.
pub fn brace(stream: TokenStream, span: Span) -> Group {
    Group::new(Delimiter::Brace, stream, span)
}

/// Concatenate token streams.
pub fn concat(streams: Vec<TokenStream>) -> TokenStream {
    let mut out = TokenStream::new();
    for s in streams {
        out.extend(s);
    }
    out
}

/// Build a path expression token stream, e.g. `std::panic`.
pub fn path(segments: &[&str], span: Span, interner: &Interner) -> TokenStream {
    let mut out = TokenStream::new();
    for (i, seg) in segments.iter().enumerate() {
        if i > 0 {
            out.push(TokenTree::Punct(punct_joint(':', span)));
            out.push(TokenTree::Punct(punct_joint(':', span)));
        }
        out.push(TokenTree::Ident(ident(seg, span, interner)));
    }
    out
}

/// Build a call expression token stream: `callee(arg1, arg2, ...)`.
pub fn call(callee: TokenStream, args: Vec<TokenStream>, span: Span) -> TokenStream {
    let mut out = callee;
    out.push(TokenTree::Punct(punct('(', span)));
    for (i, arg) in args.into_iter().enumerate() {
        if i > 0 {
            out.push(TokenTree::Punct(punct(',', span)));
        }
        out.extend(arg);
    }
    out.push(TokenTree::Punct(punct(')', span)));
    out
}

/// Build a block token stream: `{ stmts... }`.
pub fn block(stmts: Vec<TokenStream>, span: Span) -> TokenStream {
    let mut inner = TokenStream::new();
    for stmt in stmts {
        inner.extend(stmt);
        inner.push(TokenTree::Punct(punct(';', span)));
    }
    TokenStream::from_vec(vec![TokenTree::Group(brace(inner, span))])
}

/// Build a let statement: `let name = init;`.
pub fn let_stmt(name: &str, init: TokenStream, span: Span, interner: &Interner) -> TokenStream {
    let mut out = TokenStream::new();
    out.push(TokenTree::Ident(ident("let", span, interner)));
    out.push(TokenTree::Ident(ident(name, span, interner)));
    out.push(TokenTree::Punct(punct('=', span)));
    out.extend(init);
    out
}

/// Build an if expression: `if cond { then_body }`.
pub fn if_expr(
    cond: TokenStream,
    then_body: TokenStream,
    span: Span,
    interner: &Interner,
) -> TokenStream {
    let mut out = TokenStream::new();
    out.push(TokenTree::Ident(ident("if", span, interner)));
    out.extend(cond);
    out.extend(then_body);
    out
}

/// Build a unary expression: `op expr`.
pub fn unary(op: char, expr: TokenStream, span: Span) -> TokenStream {
    let mut out = TokenStream::new();
    out.push(TokenTree::Punct(punct(op, span)));
    out.extend(expr);
    out
}

/// Build a binary expression: `left op right`.
pub fn binary(left: TokenStream, op: char, right: TokenStream, span: Span) -> TokenStream {
    let mut out = left;
    out.push(TokenTree::Punct(punct(op, span)));
    out.extend(right);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use yelang_interner::Interner;

    #[test]
    fn quote_ident_renders() {
        let interner = Interner::new();
        let span = Span::default();
        let id = ident("foo", span, &interner);
        assert_eq!(id.resolve(&interner), "foo");
    }

    #[test]
    fn quote_call_renders() {
        let interner = Interner::new();
        let span = Span::default();
        let callee = path(&["std", "panic"], span, &interner);
        let arg = TokenStream::from_vec(vec![TokenTree::Literal(str_lit("oops", span, &interner))]);
        let stream = call(callee, vec![arg], span);
        assert_eq!(stream.render(&interner), "std::panic(\"oops\")");
    }

    #[test]
    fn quote_block_renders() {
        let interner = Interner::new();
        let span = Span::default();
        let stmt = let_stmt(
            "x",
            TokenStream::from_vec(vec![TokenTree::Literal(int_lit("42", span, &interner))]),
            span,
            &interner,
        );
        let stream = block(vec![stmt], span);
        assert_eq!(stream.render(&interner), "{let x=42;}");
    }

    #[test]
    fn quote_if_renders() {
        let interner = Interner::new();
        let span = Span::default();
        let cond = unary('!', path(&["x"], span, &interner), span);
        let then_body = block(
            vec![call(path(&["panic"], span, &interner), vec![], span)],
            span,
        );
        let stream = if_expr(cond, then_body, span, &interner);
        assert_eq!(stream.render(&interner), "if!x{panic();}");
    }
}

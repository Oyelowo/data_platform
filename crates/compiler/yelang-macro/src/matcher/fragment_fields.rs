//! Extraction of fragment fields for `$name.field` metavariable syntax.
//!
//! Fragment fields (RFC 3714) allow a declarative macro to access syntactic
//! components of a captured fragment without re-parsing the token tree by hand.
//! This module parses the captured token stream into the appropriate AST node
//! and extracts the requested components as macro token streams.
//!
//! Extracted fields are generated fresh from AST nodes, so they do not retain
//! the original captured tokens' hygiene contexts. This is acceptable for the
//! first implementation; precise span preservation can be added later.

use yelang_ast::{Codegen, Expr, ExprKind, Item, ItemKind, Type, TypeKind};
use yelang_interner::Interner;
use yelang_macro_core::token_tree::TokenStream;

use crate::matcher::types::FragmentFields;

/// Extract fragment fields from a captured identifier token tree.
pub fn from_ident(stream: &TokenStream) -> FragmentFields {
    FragmentFields {
        name: Some(stream.clone()),
        ..FragmentFields::default()
    }
}

/// Extract fragment fields from a captured expression token stream.
pub fn from_expr(stream: &TokenStream, interner: &Interner) -> Result<FragmentFields, String> {
    let expr: Expr = parse_fragment(stream, interner)?;
    let mut fields = FragmentFields::default();
    if let ExprKind::TypeAscription(asc) = &expr.kind {
        fields.ty = Some(ast_to_tokens(&asc.ty, interner)?);
    }
    Ok(fields)
}

/// Extract fragment fields from a captured type token stream.
pub fn from_ty(stream: &TokenStream, interner: &Interner) -> Result<FragmentFields, String> {
    let ty: Type = parse_fragment(stream, interner)?;
    let mut fields = FragmentFields::default();
    if let TypeKind::Named(path) = &ty.kind
        && !path.segments.is_empty()
    {
        let last_idx = path.segments.len() - 1;
        let base_segments = &path.segments[..last_idx + 1];
        let last = &path.segments[last_idx];

        let mut name_rendered = String::new();
        for (i, seg) in base_segments.iter().enumerate() {
            if i > 0 {
                name_rendered.push_str("::");
            }
            seg.ident
                .codegen(&mut name_rendered, interner)
                .map_err(|e| e.to_string())?;
        }
        fields.type_name = Some(tokenize_rendered(&name_rendered, interner)?);

        if let Some(args) = &last.args {
            let mut args_rendered = String::new();
            args.codegen(&mut args_rendered, interner)
                .map_err(|e| e.to_string())?;
            fields.type_args = Some(tokenize_rendered(&args_rendered, interner)?);
        }
    }
    Ok(fields)
}

/// Extract fragment fields from a captured item token stream.
pub fn from_item(stream: &TokenStream, interner: &Interner) -> Result<FragmentFields, String> {
    let item: Item = parse_fragment(stream, interner)?;
    let mut fields = FragmentFields::default();

    // Visibility.
    if !item.visibility.is_private() {
        fields.vis = Some(ast_to_tokens(&item.visibility, interner)?);
    }

    // Attributes.
    if !item.attributes.is_empty() {
        let mut rendered = String::new();
        for attr in &item.attributes {
            attr.codegen(&mut rendered, interner)
                .map_err(|e| e.to_string())?;
            rendered.push(' ');
        }
        fields.attrs = Some(tokenize_rendered(&rendered, interner)?);
    }

    // Item name.
    if let Some(name) = item_name(&item) {
        fields.item_name = Some(ast_to_tokens(name, interner)?);
    }

    Ok(fields)
}

fn item_name(item: &Item) -> Option<&yelang_ast::Ident> {
    match &item.kind {
        ItemKind::Struct(s) => Some(&s.name),
        ItemKind::Enum(e) => Some(&e.name),
        ItemKind::Fn(f) => Some(&f.name),
        ItemKind::Trait(t) => Some(&t.name),
        ItemKind::TypeAlias(t) => Some(&t.name),
        ItemKind::Const(c) => Some(&c.name),
        ItemKind::Static(s) => Some(&s.name),
        ItemKind::Module(m) => Some(&m.name),
        _ => None,
    }
}

fn parse_fragment<T>(stream: &TokenStream, interner: &Interner) -> Result<T, String>
where
    T: yelang_lexer::ParseTokenStream<yelang_ast::tokenizer::TokenKind>,
{
    let rendered = stream.render(interner);
    let local_interner = interner.clone();
    let mut lex = yelang_ast::TokenKind::tokenize(&rendered, &local_interner)
        .map_err(|e| format!("tokenize: {}", e))?;
    let value = lex.parse::<T>().map_err(|e| e.to_string())?;
    if !lex.is_eof() {
        return Err("trailing tokens after fragment".to_string());
    }
    let _ = local_interner;
    Ok(value)
}

fn ast_to_tokens<T>(value: &T, interner: &Interner) -> Result<TokenStream, String>
where
    T: Codegen,
{
    let mut rendered = String::new();
    value
        .codegen(&mut rendered, interner)
        .map_err(|e| e.to_string())?;
    tokenize_rendered(&rendered, interner)
}

fn tokenize_rendered(rendered: &str, interner: &Interner) -> Result<TokenStream, String> {
    let local_interner = interner.clone();
    let lex = yelang_ast::TokenKind::tokenize(rendered, &local_interner)
        .map_err(|e| format!("tokenize: {}", e))?;
    let tokens: Vec<_> = lex.tokens.iter().cloned().collect();
    let _ = local_interner;
    Ok(yelang_ast::expr::convert::from_lexer_tokens(
        &tokens, interner,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_ty_extracts_name_and_args() {
        let interner = Interner::new();
        let mut stream = yelang_ast::TokenKind::tokenize("Vec<i32>", &interner.clone()).unwrap();
        let tokens: Vec<_> = std::iter::from_fn(|| stream.advance().cloned()).collect();
        let tt_stream = yelang_ast::expr::convert::from_lexer_tokens(&tokens, &interner);
        let fields = from_ty(&tt_stream, &interner).unwrap();
        assert_eq!(
            fields.type_name.as_ref().map(|s| s.render(&interner)),
            Some("Vec".to_string())
        );
        assert_eq!(
            fields.type_args.as_ref().map(|s| s.render(&interner)),
            Some("<i32>".to_string())
        );
    }

    #[test]
    fn from_ty_without_args_has_none_type_args() {
        let interner = Interner::new();
        let mut stream = yelang_ast::TokenKind::tokenize("i32", &interner.clone()).unwrap();
        let tokens: Vec<_> = std::iter::from_fn(|| stream.advance().cloned()).collect();
        let tt_stream = yelang_ast::expr::convert::from_lexer_tokens(&tokens, &interner);
        let fields = from_ty(&tt_stream, &interner).unwrap();
        assert_eq!(
            fields.type_name.as_ref().map(|s| s.render(&interner)),
            Some("i32".to_string())
        );
        assert!(fields.type_args.is_none());
    }
}

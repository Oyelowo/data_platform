/*
 * AST Snapshot Tests
 *
 * This test suite validates the robustness and correctness of AST parsing
 * using snapshot testing. It ensures that the AST structure is correct
 * for various inputs, including operator precedence, complex expressions,
 * and edge cases.
 */

pub(super) use crate::{
    Codegen, Expr, FieldAssign, FnDef, Folder, Interner, Object, ObjectField, Stmt, TokenKind,
};

pub(super) use yelang_lexer::ParseTokenStream;

pub(super) use super::other::{assert_round_trip, assert_snapshot, parse_pattern};

// 1. Define the SpanClearer
#[allow(dead_code)]
pub(super) struct SpanClearer;
impl crate::visit::fold::Folder for SpanClearer {
    // We only need to override the methods for nodes that have spans we want to clear.
    // Since Folder recursively calls default impls, we just modify the result.

    fn fold_stmt(&mut self, node: Stmt) -> Stmt {
        let mut n = crate::visit::fold::stmt::fold_stmt(self, node);
        n.span = yelang_lexer::Span::default();
        n
    }

    fn fold_expr(&mut self, node: Expr) -> Expr {
        let mut n = crate::visit::fold::expr::fold_expr(self, node);
        n.span = yelang_lexer::Span::default();
        n
    }

    fn fold_item(&mut self, node: crate::Item) -> crate::Item {
        let mut n = crate::visit::fold::item::fold_item(self, node);
        n.span = yelang_lexer::Span::default();
        n
    }

    // Add other nodes if they cause failures (e.g., Ident, Path)
    fn fold_path(&mut self, node: crate::Path) -> crate::Path {
        let mut n = crate::visit::fold::expr::fold_path(self, node);
        n.span = yelang_lexer::Span::default();
        n
    }

    fn fold_object(&mut self, node: Object) -> Object {
        let mut fields = Vec::new();
        for field in node.fields {
            let mut key = field.key;
            key.span = yelang_lexer::Span::default(); // Clear Ident span
            let val = self.fold_expr(field.val);
            fields.push(ObjectField { key, val });
        }
        Object {
            fields,
            span: yelang_lexer::Span::default(), // Clear Object span
        }
    }

    fn fold_ident(&mut self, node: crate::Ident) -> crate::Ident {
        let mut n = node;
        n.span = yelang_lexer::Span::default();
        n
    }

    fn fold_attribute(&mut self, node: crate::item::Attribute) -> crate::item::Attribute {
        let mut n = crate::visit::fold::item::fold_attribute(self, node);
        n.span = yelang_lexer::Span::default();
        n
    }

    fn fold_struct(&mut self, node: crate::item::Struct) -> crate::item::Struct {
        let mut n = crate::visit::fold::item::fold_struct(self, node);
        n.name = self.fold_ident(n.name);
        n.span = yelang_lexer::Span::default();
        n.generics.span = yelang_lexer::Span::default();
        n
    }

    fn fold_field_assign(&mut self, node: FieldAssign) -> FieldAssign {
        let mut n = crate::visit::fold::expr::fold_field_assign(self, node);
        n.span = yelang_lexer::Span::default();
        n.name.span = yelang_lexer::Span::default();
        n
    }
}

/// Helper to parse an expression and return the AST
#[allow(dead_code)]
pub(super) fn parse_expr(input: &str) -> Expr {
    let mut interner = Interner::new();
    let mut token_stream = TokenKind::tokenize(input, &mut interner).expect("Tokenization failed");
    // println!("Tokens for '{}': {:?}", input, token_stream);
    token_stream.parse::<Expr>().expect("Parsing failed")
}

/// Helper to parse a statement and return the AST
pub(super) fn parse_stmt(input: &str) -> Stmt {
    let mut interner = Interner::new();
    let mut token_stream = TokenKind::tokenize(input, &mut interner).expect("Tokenization failed");
    token_stream.parse::<Stmt>().expect("Parsing failed")
}

use super::AstValidationError;
use crate::visit::Visitor;
use crate::{Expr, ExprKind, Ident, UpdateMutation, UpdateQ};
use std::ops::ControlFlow;

/// Validates UPDATE `set` semantics at the AST level.
///
/// Rule: each setter LHS must be rooted on the statement's `@item` binder,
/// and must contain at least one postfix access (e.g. `u.name`, `u.tags[0]`, `u.{...}`).
pub fn validate_update_stmt(stmt: &UpdateQ) -> Vec<AstValidationError> {
    let mut v = UpdateValidationVisitor { errors: Vec::new() };
    let _ = v.visit_update_stmt(stmt);
    v.errors
}

/// Validates the whole program using the AST walk Visitor.
pub fn validate_program(program: &crate::Program) -> Vec<AstValidationError> {
    let mut v = UpdateValidationVisitor { errors: Vec::new() };
    let _ = v.visit_program(program);
    v.errors
}

struct UpdateValidationVisitor {
    errors: Vec<AstValidationError>,
}

impl Visitor for UpdateValidationVisitor {
    fn visit_update_stmt(&mut self, stmt: &UpdateQ) -> ControlFlow<()> {
        if let UpdateMutation::Set(setters) = &stmt.mutation {
            for setter in setters {
                if !is_setter_path_rooted_on_binding(&setter.path, stmt.binding) {
                    self.errors.push(AstValidationError::new(
                        "UPDATE `set` LHS must be rooted on the @item binder (e.g. `u.name = ...`)",
                        setter.path.span,
                    ));
                }
            }
        }

        crate::visit::walk::walk_update_stmt(self, stmt)
    }
}

fn is_setter_path_rooted_on_binding(expr: &Expr, binding: Ident) -> bool {
    struct SetterPathRootedOnBindingVisitor {
        binding: Ident,
        saw_postfix: bool,
        ok: bool,
    }

    impl Visitor for SetterPathRootedOnBindingVisitor {
        fn visit_expr(&mut self, expr: &Expr) -> ControlFlow<()> {
            match &expr.kind {
                ExprKind::MemberAccess(ma) => {
                    self.saw_postfix = true;
                    self.visit_expr(ma.base())
                }
                ExprKind::ArrayAccess(aa) => {
                    self.saw_postfix = true;
                    self.visit_expr(aa.base())
                }
                ExprKind::DocumentAccess(da) => {
                    self.saw_postfix = true;
                    self.visit_expr(da.base())
                }
                ExprKind::Try(tsa) => {
                    self.saw_postfix = true;
                    self.visit_expr(&tsa.base)
                }
                ExprKind::Grouped(g) => self.visit_expr(g.expr()),
                ExprKind::Path(path) => {
                    self.ok = self.saw_postfix
                        && path
                            .standalone_ident()
                            .is_some_and(|id| *id == self.binding);
                    ControlFlow::Break(())
                }
                _ => {
                    self.ok = false;
                    ControlFlow::Break(())
                }
            }
        }
    }

    let mut v = SetterPathRootedOnBindingVisitor {
        binding,
        saw_postfix: false,
        ok: false,
    };
    let _ = v.visit_expr(expr);
    v.ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Interner;
    use crate::tokenizer::TokenKind;
    use yelang_lexer::ParseTokenStream;

    #[test]
    fn update_setter_requires_binding_root() {
        let input = "update users@u:User set name = 'Jane' where u.id == 1";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let q = crate::UpdateQ::parse(&mut stream).expect("update should parse");

        let errs = validate_update_stmt(&q);
        assert!(!errs.is_empty(), "expected validation error");
    }

    #[test]
    fn update_setter_allows_binding_field_access() {
        let input = "update users@u:User set u.name = 'Jane' where u.id == 1";
        let mut interner = Interner::new();
        let mut stream = TokenKind::tokenize(input, &mut interner).unwrap();
        let q = crate::UpdateQ::parse(&mut stream).expect("update should parse");

        let errs = validate_update_stmt(&q);
        assert!(errs.is_empty(), "unexpected validation errors: {errs:?}");
    }
}

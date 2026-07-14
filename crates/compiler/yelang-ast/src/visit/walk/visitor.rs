/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2025 Oyelowo Oyedayo
 */

use crate::*;
use std::ops::ControlFlow;

use super::{expr::*, item::*, query::*, stmt::*};

/// The Visitor Trait
///
/// - Methods are named `visit_<node>`.
/// - Default implementation calls `walk_<node>`.
/// - Users override `visit_<node>` to add logic.
/// - Users call `walk_<node>(self, node)` within their override to continue recursion.
pub trait Visitor: Sized {
    // ========================================================================
    // ENTRY POINTS
    // ========================================================================

    fn visit_program(&mut self, program: &Program) -> ControlFlow<()> {
        walk_program(self, program)
    }

    fn visit_stmt(&mut self, stmt: &Stmt) -> ControlFlow<()> {
        walk_stmt(self, stmt)
    }

    fn visit_let_stmt(&mut self, let_stmt: &stmt::LetStmt) -> ControlFlow<()> {
        walk_let_stmt(self, let_stmt)
    }

    fn visit_expr(&mut self, expr: &Expr) -> ControlFlow<()> {
        walk_expr(self, expr)
    }

    fn visit_item(&mut self, item: &Item) -> ControlFlow<()> {
        walk_item(self, item)
    }

    fn visit_type(&mut self, ty: &Type) -> ControlFlow<()> {
        walk_type(self, ty)
    }

    fn visit_pattern(&mut self, pattern: &Pattern) -> ControlFlow<()> {
        walk_pattern(self, pattern)
    }

    fn visit_query(&mut self, query: &Query) -> ControlFlow<()> {
        walk_query(self, query)
    }

    // ========================================================================
    // ITEMS (Granular)
    // ========================================================================

    fn visit_fn(&mut self, func: &item::FnDef) -> ControlFlow<()> {
        walk_fn(self, func)
    }

    fn visit_struct(&mut self, structure: &item::Struct) -> ControlFlow<()> {
        walk_struct(self, structure)
    }

    fn visit_enum(&mut self, enum_def: &item::Enum) -> ControlFlow<()> {
        walk_enum(self, enum_def)
    }

    fn visit_trait(&mut self, trait_def: &item::Trait) -> ControlFlow<()> {
        walk_trait(self, trait_def)
    }

    fn visit_trait_bound(&mut self, bound: &item::TraitBound) -> ControlFlow<()> {
        super::item::walk_trait_bound(self, bound)
    }

    fn visit_impl(&mut self, impl_def: &item::Impl) -> ControlFlow<()> {
        walk_impl(self, impl_def)
    }

    fn visit_module(&mut self, module: &item::ModDef) -> ControlFlow<()> {
        walk_module(self, module)
    }

    fn visit_type_alias(&mut self, type_alias: &item::TypeAlias) -> ControlFlow<()> {
        walk_type_alias(self, type_alias)
    }

    fn visit_const(&mut self, const_item: &item::Const) -> ControlFlow<()> {
        walk_const(self, const_item)
    }

    fn visit_static(&mut self, static_item: &item::Static) -> ControlFlow<()> {
        walk_static(self, static_item)
    }

    // ========================================================================
    // ITEM COMPONENTS (Granular)
    // ========================================================================

    fn visit_generics(&mut self, generics: &item::Generics) -> ControlFlow<()> {
        walk_generics(self, generics)
    }

    fn visit_where_clause(&mut self, wc: &item::WhereClause) -> ControlFlow<()> {
        walk_where_clause(self, wc)
    }

    fn visit_attribute(&mut self, attr: &item::Attribute) -> ControlFlow<()> {
        walk_attribute(self, attr)
    }

    fn visit_impl_item(&mut self, item: &item::ImplItem) -> ControlFlow<()> {
        walk_impl_item(self, item)
    }

    fn visit_trait_item(&mut self, item: &item::TraitItem) -> ControlFlow<()> {
        walk_trait_item(self, item)
    }

    fn visit_path_segment(&mut self, segment: &expr::PathSegment) -> ControlFlow<()> {
        walk_path_segment(self, segment)
    }

    fn visit_param(&mut self, param: &item::Param) -> ControlFlow<()> {
        walk_param(self, param)
    }

    fn visit_generic_param(&mut self, param: &item::GenericParam) -> ControlFlow<()> {
        match param {
            item::GenericParam::Type(t) => self.visit_type_param(t),
            item::GenericParam::Const(c) => self.visit_const_param(c),
        }
    }

    fn visit_type_param(&mut self, param: &item::TypeParam) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    fn visit_const_param(&mut self, param: &item::ConstParam) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    fn visit_field_def(&mut self, field: &item::FieldDef) -> ControlFlow<()> {
        walk_field_def(self, field)
    }

    fn visit_field_assign(&mut self, field: &expr::FieldAssign) -> ControlFlow<()> {
        walk_field_assign(self, field)
    }

    fn visit_field_pattern(&mut self, field: &FieldPattern) -> ControlFlow<()> {
        walk_field_pattern(self, field)
    }

    fn visit_generic_args(&mut self, args: &expr::GenericArgs) -> ControlFlow<()> {
        walk_generic_args(self, args)
    }

    fn visit_use_tree(&mut self, tree: &item::UseTree) -> ControlFlow<()> {
        walk_use_tree(self, tree)
    }

    fn visit_use(&mut self, u: &item::Use) -> ControlFlow<()> {
        self.visit_use_tree(&u.tree)
    }

    // ========================================================================
    // EXPRESSIONS
    // ========================================================================

    fn visit_binary_expr(&mut self, bin: &BinaryExpr) -> ControlFlow<()> {
        walk_binary_expr(self, bin)
    }

    fn visit_unary_expr(&mut self, unary: &UnaryExpr) -> ControlFlow<()> {
        walk_unary_expr(self, unary)
    }

    fn visit_if_expr(&mut self, if_expr: &IfExpr) -> ControlFlow<()> {
        walk_if_expr(self, if_expr)
    }

    fn visit_let_expr(&mut self, let_expr: &LetExpr) -> ControlFlow<()> {
        walk_let_expr(self, let_expr)
    }

    fn visit_block_expr(&mut self, block: &BlockExpr) -> ControlFlow<()> {
        walk_block_expr(self, block)
    }

    fn visit_call_expr(&mut self, call: &CallExpr) -> ControlFlow<()> {
        walk_call_expr(self, call)
    }

    fn visit_async_expr(&mut self, async_expr: &AsyncExpr) -> ControlFlow<()> {
        walk_async_expr(self, async_expr)
    }

    fn visit_member_access(&mut self, access: &MemberAccess) -> ControlFlow<()> {
        walk_member_access(self, access)
    }

    fn visit_array_access(&mut self, access: &ArrayAccess) -> ControlFlow<()> {
        walk_array_access(self, access)
    }

    fn visit_array(&mut self, array: &Array) -> ControlFlow<()> {
        walk_array(self, array)
    }

    fn visit_object(&mut self, object: &Object) -> ControlFlow<()> {
        walk_object(self, object)
    }

    fn visit_ternary_expr(&mut self, ternary: &TernaryExpr) -> ControlFlow<()> {
        walk_ternary_expr(self, ternary)
    }

    fn visit_tuple_expr(&mut self, exprs: &[Expr]) -> ControlFlow<()> {
        walk_tuple_expr(self, exprs)
    }

    fn visit_loop_expr(&mut self, loop_expr: &LoopExpr) -> ControlFlow<()> {
        walk_loop_expr(self, loop_expr)
    }

    fn visit_while_expr(&mut self, while_expr: &WhileExpr) -> ControlFlow<()> {
        walk_while_expr(self, while_expr)
    }

    fn visit_assign_eq_expr(&mut self, assign: &AssignEqExpr) -> ControlFlow<()> {
        walk_assign_eq_expr(self, assign)
    }

    fn visit_assign_op_expr(&mut self, assign: &AssignOpExpr) -> ControlFlow<()> {
        walk_assign_op_expr(self, assign)
    }

    fn visit_destructure_assign_expr(&mut self, assign: &DestructureAssignExpr) -> ControlFlow<()> {
        walk_destructure_assign_expr(self, assign)
    }

    fn visit_return_expr(&mut self, expr: &Option<Box<Expr>>) -> ControlFlow<()> {
        walk_return_expr(self, expr)
    }

    fn visit_break_expr(&mut self, expr: &Option<Box<Expr>>) -> ControlFlow<()> {
        walk_break_expr(self, expr)
    }

    fn visit_break_expr_full(&mut self, break_expr: &expr::BreakExpr) -> ControlFlow<()> {
        walk_break_expr_full(self, break_expr)
    }

    fn visit_continue_expr(&mut self, continue_expr: &expr::ContinueExpr) -> ControlFlow<()> {
        walk_continue_expr(self, continue_expr)
    }

    fn visit_gen_expr(&mut self, expr: &Expr) -> ControlFlow<()> {
        walk_gen_expr(self, expr)
    }

    fn visit_await_expr(&mut self, expr: &Expr) -> ControlFlow<()> {
        walk_await_expr(self, expr)
    }

    fn visit_macro_invocation(&mut self, inv: &MacroInvocation) -> ControlFlow<()> {
        walk_macro_invocation(self, inv)
    }

    fn visit_grouped_expr(&mut self, grouped: &GroupedExpr) -> ControlFlow<()> {
        walk_grouped_expr(self, grouped)
    }

    fn visit_range_expr(&mut self, range: &RangeExpr) -> ControlFlow<()> {
        walk_range_expr(self, range)
    }

    fn visit_document_access(&mut self, access: &DocumentAccess) -> ControlFlow<()> {
        walk_document_access(self, access)
    }

    fn visit_document(&mut self, doc: &Document) -> ControlFlow<()> {
        walk_document(self, doc)
    }

    fn visit_literal(&mut self, _lit: &Literal) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    fn visit_interpolated_string(&mut self, parts: &[StringPart]) -> ControlFlow<()> {
        walk_interpolated_string(self, parts)
    }

    fn visit_path(&mut self, path: &Path) -> ControlFlow<()> {
        walk_path(self, path)
    }

    fn visit_ident(&mut self, _ident: &Ident) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    fn visit_bind_at(&mut self, bind_at: &BindAtExpr) -> ControlFlow<()> {
        walk_bind_at(self, bind_at)
    }

    fn visit_for_loop_expr(&mut self, for_loop: &ForLoopExpr) -> ControlFlow<()> {
        walk_for_loop_expr(self, for_loop)
    }

    fn visit_is_type_expr(&mut self, is_type: &IsTypeExpr) -> ControlFlow<()> {
        walk_is_type_expr(self, is_type)
    }

    fn visit_type_cast(&mut self, type_cast: &TypeCast) -> ControlFlow<()> {
        walk_type_cast(self, type_cast)
    }

    fn visit_type_ascription(&mut self, type_ascription: &TypeAscription) -> ControlFlow<()> {
        walk_type_ascription(self, type_ascription)
    }

    fn visit_try_safe_access(&mut self, try_safe: &TrySafeAccess) -> ControlFlow<()> {
        walk_try_safe_access(self, try_safe)
    }

    fn visit_lambda_expr(&mut self, lambda: &LambdaExpr) -> ControlFlow<()> {
        walk_lambda_expr(self, lambda)
    }

    fn visit_struct_expr(&mut self, struct_expr: &StructExpr) -> ControlFlow<()> {
        walk_struct_expr(self, struct_expr)
    }

    fn visit_comprehension_expr(&mut self, comp: &ComprehensionExpr) -> ControlFlow<()> {
        walk_comprehension_expr(self, comp)
    }

    fn visit_match_expr(&mut self, match_expr: &MatchExpr) -> ControlFlow<()> {
        walk_match_expr(self, match_expr)
    }

    fn visit_method_call_expr(&mut self, method_call: &MethodCallExpr) -> ControlFlow<()> {
        walk_method_call_expr(self, method_call)
    }

    // ========================================================================
    // QUERIES
    // ========================================================================

    fn visit_select_stmt(&mut self, stmt: &SelectQ) -> ControlFlow<()> {
        walk_select_stmt(self, stmt)
    }

    fn visit_create_stmt(&mut self, stmt: &CreateQ) -> ControlFlow<()> {
        walk_create_stmt(self, stmt)
    }

    fn visit_upsert_stmt(&mut self, stmt: &UpsertQ) -> ControlFlow<()> {
        walk_upsert_stmt(self, stmt)
    }

    fn visit_update_stmt(&mut self, stmt: &UpdateQ) -> ControlFlow<()> {
        walk_update_stmt(self, stmt)
    }

    fn visit_unlink_stmt(&mut self, stmt: &UnlinkQ) -> ControlFlow<()> {
        walk_unlink_stmt(self, stmt)
    }

    // Select statement components
    fn visit_from_node(&mut self, node: &query::FromNode) -> ControlFlow<()> {
        walk_from_node(self, node)
    }

    fn visit_select_node(&mut self, node: &query::Node) -> ControlFlow<()> {
        walk_select_node(self, node)
    }

    fn visit_select_linkpath(&mut self, path: &query::LinkPath) -> ControlFlow<()> {
        walk_select_linkpath(self, path)
    }

    fn visit_select_linksegment(&mut self, segment: &query::LinkSegment) -> ControlFlow<()> {
        walk_select_linksegment(self, segment)
    }

    fn visit_select_edge(&mut self, edge: &query::Edge) -> ControlFlow<()> {
        walk_select_edge(self, edge)
    }

    fn visit_select_order_by_part(&mut self, part: &query::OrderByPart) -> ControlFlow<()> {
        walk_select_order_by_part(self, part)
    }

    fn visit_select_range(&mut self, range: &query::Range) -> ControlFlow<()> {
        if let Some(start) = &range.start {
            self.visit_expr(start)?;
        }
        if let Some(end) = &range.end {
            self.visit_expr(end)?;
        }
        ControlFlow::Continue(())
    }

    fn visit_select_modifiers(&mut self, mods: &query::Modifiers) -> ControlFlow<()> {
        walk_select_modifiers(self, mods)
    }

    fn visit_hop_range(&mut self, range: &query::HopRange) -> ControlFlow<()> {
        walk_hop_range(self, range)
    }

    fn visit_link_stmt(&mut self, stmt: &LinkQ) -> ControlFlow<()> {
        walk_link_stmt(self, stmt)
    }

    fn visit_create_path(&mut self, path: &CreatePath) -> ControlFlow<()> {
        walk_create_path(self, path)
    }

    fn visit_create_path_segment(&mut self, segment: &CreatePathSegment) -> ControlFlow<()> {
        walk_create_path_segment(self, segment)
    }

    fn visit_create_edge(&mut self, edge: &CreateEdge) -> ControlFlow<()> {
        walk_create_edge(self, edge)
    }

    fn visit_delete_stmt(&mut self, stmt: &DeleteQ) -> ControlFlow<()> {
        walk_delete_stmt(self, stmt)
    }
}

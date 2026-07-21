/*
 * Author: Oyelowo Oyedayo
 * Email: oyelowo.oss@gmail.com
 * Copyright (c) 2025 Oyelowo Oyedayo
 */

use super::{expr::*, item::*, query::*, stmt::*};
use crate::Program as ItemProgram;
use crate::*;

pub trait Folder: Sized {
    // ========================================================================
    // ENTRY POINTS
    // ========================================================================

    fn fold_program(&mut self, node: ItemProgram) -> ItemProgram {
        fold_program(self, node)
    }

    fn fold_stmt(&mut self, node: Stmt) -> Stmt {
        fold_stmt(self, node)
    }

    fn fold_expr(&mut self, node: Expr) -> Expr {
        fold_expr(self, node)
    }

    fn fold_item(&mut self, node: Item) -> Item {
        fold_item(self, node)
    }

    fn fold_query(&mut self, node: Query) -> Query {
        fold_query(self, node)
    }

    // ========================================================================
    // ITEMS (Granular)
    // ========================================================================

    fn fold_fn(&mut self, node: item::FnDef) -> item::FnDef {
        fold_fn(self, node)
    }

    fn fold_struct(&mut self, node: item::Struct) -> item::Struct {
        fold_struct(self, node)
    }

    fn fold_enum(&mut self, node: item::Enum) -> item::Enum {
        fold_enum(self, node)
    }

    fn fold_trait(&mut self, node: item::Trait) -> item::Trait {
        fold_trait(self, node)
    }

    fn fold_trait_bound(&mut self, node: item::TraitBound) -> item::TraitBound {
        super::item::fold_trait_bound(self, node)
    }

    fn fold_impl(&mut self, node: item::Impl) -> item::Impl {
        fold_impl(self, node)
    }

    fn fold_module(&mut self, node: item::ModDef) -> item::ModDef {
        fold_module(self, node)
    }

    fn fold_type_alias(&mut self, node: item::TypeAlias) -> item::TypeAlias {
        fold_type_alias(self, node)
    }

    fn fold_const(&mut self, node: item::Const) -> item::Const {
        fold_const(self, node)
    }

    fn fold_static(&mut self, node: item::Static) -> item::Static {
        fold_static(self, node)
    }

    // ========================================================================
    // ITEM COMPONENTS (Granular)
    // ========================================================================

    fn fold_generics(&mut self, node: item::Generics) -> item::Generics {
        fold_generics(self, node)
    }

    fn fold_where_clause(&mut self, node: item::WhereClause) -> item::WhereClause {
        fold_where_clause(self, node)
    }

    fn fold_attribute(&mut self, node: item::Attribute) -> item::Attribute {
        fold_attribute(self, node)
    }

    fn fold_impl_item(&mut self, node: item::ImplItem) -> item::ImplItem {
        super::item::fold_impl_item_node(self, node)
    }

    fn fold_trait_item(&mut self, node: item::TraitItem) -> item::TraitItem {
        super::item::fold_trait_item_node(self, node)
    }

    fn fold_path_segment(&mut self, node: expr::PathSegment) -> expr::PathSegment {
        fold_path_segment(self, node)
    }

    fn fold_ident(&mut self, node: Ident) -> Ident {
        fold_ident(self, node)
    }

    fn fold_param(&mut self, node: item::Param) -> item::Param {
        fold_param(self, node)
    }

    fn fold_generic_param(&mut self, node: item::GenericParam) -> item::GenericParam {
        match node {
            item::GenericParam::Type(t) => item::GenericParam::Type(self.fold_type_param(t)),
            item::GenericParam::Const(c) => item::GenericParam::Const(self.fold_const_param(c)),
        }
    }

    fn fold_type_param(&mut self, node: item::TypeParam) -> item::TypeParam {
        item::TypeParam {
            name: node.name,
            bounds: node
                .bounds
                .into_iter()
                .map(|b| item::TraitBound {
                    binder: b.binder,
                    path: self.fold_path(b.path),
                    span: b.span,
                })
                .collect(),
            default: node.default.map(|ty| self.fold_type(ty)),
            span: node.span,
        }
    }

    fn fold_const_param(&mut self, node: item::ConstParam) -> item::ConstParam {
        item::ConstParam {
            name: node.name,
            ty: self.fold_type(node.ty),
            default: node.default.map(|e| self.fold_expr(e)),
            span: node.span,
        }
    }

    fn fold_field_def(&mut self, node: item::FieldDef) -> item::FieldDef {
        fold_field_def(self, node)
    }

    fn fold_field_assign(&mut self, node: expr::FieldAssign) -> expr::FieldAssign {
        super::expr::fold_field_assign(self, node)
    }

    fn fold_field_pattern(&mut self, node: FieldPattern) -> FieldPattern {
        super::item::fold_field_pattern(self, node)
    }

    fn fold_generic_args(&mut self, node: expr::GenericArgs) -> expr::GenericArgs {
        fold_generic_args(self, node)
    }

    fn fold_use_tree(&mut self, node: item::UseTree) -> item::UseTree {
        super::item::fold_use_tree(self, node)
    }

    fn fold_use(&mut self, node: item::Use) -> item::Use {
        item::Use {
            tree: self.fold_use_tree(node.tree),
            span: node.span,
        }
    }

    fn fold_fn_sig(&mut self, node: item::FnSig) -> item::FnSig {
        super::item::fold_fn_sig(self, node)
    }

    fn fold_variant_def(&mut self, node: item::VariantDef) -> item::VariantDef {
        super::item::fold_variant_def(self, node)
    }

    fn fold_let_stmt(&mut self, node: LetStmt) -> LetStmt {
        super::stmt::fold_let_stmt(self, node)
    }

    // ========================================================================
    // EXPRESSIONS
    // ========================================================================

    fn fold_binary_expr(&mut self, node: BinaryExpr) -> BinaryExpr {
        fold_binary_expr(self, node)
    }

    fn fold_unary_expr(&mut self, node: UnaryExpr) -> UnaryExpr {
        fold_unary_expr(self, node)
    }

    fn fold_if_expr(&mut self, node: IfExpr) -> IfExpr {
        fold_if_expr(self, node)
    }

    fn fold_let_expr(&mut self, node: LetExpr) -> LetExpr {
        fold_let_expr(self, node)
    }

    fn fold_block_expr(&mut self, node: BlockExpr) -> BlockExpr {
        fold_block_expr(self, node)
    }

    fn fold_call_expr(&mut self, node: CallExpr) -> CallExpr {
        fold_call_expr(self, node)
    }

    fn fold_async_expr(&mut self, node: AsyncExpr) -> AsyncExpr {
        fold_async_expr(self, node)
    }

    fn fold_member_access(&mut self, node: MemberAccess) -> MemberAccess {
        fold_member_access(self, node)
    }

    fn fold_array_access(&mut self, node: ArrayAccess) -> ArrayAccess {
        fold_array_access(self, node)
    }

    fn fold_array(&mut self, node: Array) -> Array {
        fold_array(self, node)
    }

    fn fold_object(&mut self, node: Object) -> Object {
        fold_object(self, node)
    }

    fn fold_ternary_expr(&mut self, node: TernaryExpr) -> TernaryExpr {
        fold_ternary_expr(self, node)
    }

    fn fold_tuple_expr(&mut self, exprs: Vec<Expr>) -> Vec<Expr> {
        fold_tuple_expr(self, exprs)
    }

    fn fold_loop_expr(&mut self, node: LoopExpr) -> LoopExpr {
        fold_loop_expr(self, node)
    }

    fn fold_while_expr(&mut self, node: WhileExpr) -> WhileExpr {
        fold_while_expr(self, node)
    }

    fn fold_assign_eq_expr(&mut self, node: AssignEqExpr) -> AssignEqExpr {
        fold_assign_eq_expr(self, node)
    }

    fn fold_assign_op_expr(&mut self, node: AssignOpExpr) -> AssignOpExpr {
        fold_assign_op_expr(self, node)
    }

    fn fold_destructure_assign_expr(
        &mut self,
        node: DestructureAssignExpr,
    ) -> DestructureAssignExpr {
        fold_destructure_assign_expr(self, node)
    }

    fn fold_return_expr(&mut self, expr: Option<Box<Expr>>) -> Option<Box<Expr>> {
        fold_return_expr(self, expr)
    }

    fn fold_break_expr(&mut self, expr: Option<Box<Expr>>) -> Option<Box<Expr>> {
        fold_break_expr(self, expr)
    }

    fn fold_gen_expr(&mut self, expr: Box<Expr>) -> Box<Expr> {
        fold_gen_expr(self, expr)
    }

    fn fold_await_expr(&mut self, expr: Box<Expr>) -> Box<Expr> {
        fold_await_expr(self, expr)
    }

    fn fold_intrinsic_expr(&mut self, node: crate::IntrinsicExpr) -> crate::IntrinsicExpr {
        fold_intrinsic_expr(self, node)
    }

    fn fold_grouped_expr(&mut self, node: GroupedExpr) -> GroupedExpr {
        fold_grouped_expr(self, node)
    }

    fn fold_range_expr(&mut self, node: RangeExpr) -> RangeExpr {
        fold_range_expr(self, node)
    }

    fn fold_bind_at(&mut self, node: BindAtExpr) -> BindAtExpr {
        fold_bind_at(self, node)
    }

    fn fold_for_loop_expr(&mut self, node: ForLoopExpr) -> ForLoopExpr {
        fold_for_loop_expr(self, node)
    }

    fn fold_is_type_expr(&mut self, node: IsTypeExpr) -> IsTypeExpr {
        fold_is_type_expr(self, node)
    }

    fn fold_type_cast(&mut self, node: TypeCast) -> TypeCast {
        fold_type_cast(self, node)
    }

    fn fold_type_ascription(&mut self, node: TypeAscription) -> TypeAscription {
        fold_type_ascription(self, node)
    }

    fn fold_try_safe_access(&mut self, node: TrySafeAccess) -> TrySafeAccess {
        fold_try_safe_access(self, node)
    }

    fn fold_lambda_expr(&mut self, node: LambdaExpr) -> LambdaExpr {
        fold_lambda_expr(self, node)
    }

    fn fold_struct_expr(&mut self, node: StructExpr) -> StructExpr {
        fold_struct_expr(self, node)
    }

    fn fold_comprehension_expr(&mut self, node: ComprehensionExpr) -> ComprehensionExpr {
        fold_comprehension_expr(self, node)
    }

    fn fold_match_expr(&mut self, node: MatchExpr) -> MatchExpr {
        fold_match_expr(self, node)
    }

    fn fold_document_access(&mut self, node: DocumentAccess) -> DocumentAccess {
        super::expr::fold_document_access(self, node)
    }

    fn fold_document(&mut self, node: Document) -> Document {
        super::expr::fold_document(self, node)
    }

    fn fold_method_call_expr(&mut self, node: MethodCallExpr) -> MethodCallExpr {
        super::expr::fold_method_call_expr(self, node)
    }

    // ========================================================================
    // TYPES AND PATTERNS
    // ========================================================================

    fn fold_type(&mut self, node: Type) -> Type {
        super::item::fold_type(self, node)
    }

    fn fold_pattern(&mut self, node: Pattern) -> Pattern {
        super::item::fold_pattern(self, node)
    }

    fn fold_path(&mut self, node: Path) -> Path {
        super::expr::fold_path(self, node)
    }

    // ========================================================================
    // QUERIES
    // ========================================================================

    fn fold_select_stmt(&mut self, node: SelectQ) -> SelectQ {
        fold_select_stmt(self, node)
    }

    fn fold_create_stmt(&mut self, node: CreateQ) -> CreateQ {
        fold_create_stmt(self, node)
    }

    fn fold_upsert_stmt(&mut self, node: UpsertQ) -> UpsertQ {
        fold_upsert_stmt(self, node)
    }

    fn fold_update_stmt(&mut self, node: UpdateQ) -> UpdateQ {
        fold_update_stmt(self, node)
    }

    fn fold_unlink_stmt(&mut self, node: UnlinkQ) -> UnlinkQ {
        fold_unlink_stmt(self, node)
    }

    fn fold_link_stmt(&mut self, node: LinkQ) -> LinkQ {
        fold_link_stmt(self, node)
    }

    fn fold_create_path(&mut self, path: CreatePath) -> CreatePath {
        fold_create_path(self, path)
    }

    fn fold_create_path_segment(&mut self, segment: CreatePathSegment) -> CreatePathSegment {
        fold_create_path_segment(self, segment)
    }

    fn fold_create_edge(&mut self, edge: CreateEdge) -> CreateEdge {
        fold_create_edge(self, edge)
    }

    fn fold_delete_stmt(&mut self, node: DeleteQ) -> DeleteQ {
        fold_delete_stmt(self, node)
    }

    // Select statement components
    fn fold_from_node(&mut self, node: query::FromNode) -> query::FromNode {
        query::FromNode {
            var: node.var,
            bind: node.bind,
            ty: node.ty.map(|t| self.fold_type(t)),
            modifiers: self.fold_modifiers(node.modifiers),
        }
    }

    fn fold_select_node(&mut self, node: query::Node) -> query::Node {
        query::Node {
            var: node.var,
            bind: node.bind,
            ty: node.ty.map(|t| self.fold_type(t)),
            modifiers: self.fold_modifiers(node.modifiers),
        }
    }

    fn fold_modifiers(&mut self, node: query::Modifiers) -> query::Modifiers {
        query::Modifiers {
            filter: node.filter.map(|e| self.fold_expr(e)),
            order: node.order.map(|o| {
                o.into_iter()
                    .map(|p| self.fold_select_order_by_part(p))
                    .collect()
            }),
            range: node.range.map(|r| self.fold_select_range(r)),
        }
    }

    fn fold_select_linkpath(&mut self, node: query::LinkPath) -> query::LinkPath {
        query::LinkPath {
            start: self.fold_select_node(node.start),
            segments: node
                .segments
                .into_iter()
                .map(|s| self.fold_select_linksegment(s))
                .collect(),
        }
    }

    fn fold_select_linksegment(&mut self, node: query::LinkSegment) -> query::LinkSegment {
        query::LinkSegment {
            edge: self.fold_select_edge(node.edge),
            target: self.fold_select_node(node.target),
        }
    }

    fn fold_select_edge(&mut self, node: query::Edge) -> query::Edge {
        query::Edge {
            var: node.var,
            bind: node.bind,
            ty: node.ty.map(|t| self.fold_type(t)),
            hops: node.hops.map(|h| self.fold_hop_range(h)),
            modifiers: self.fold_modifiers(node.modifiers),
            direction: node.direction,
        }
    }

    fn fold_select_order_by_part(&mut self, node: query::OrderByPart) -> query::OrderByPart {
        query::OrderByPart {
            field: self.fold_expr(node.field),
            direction: node.direction,
        }
    }

    fn fold_select_range(&mut self, node: query::Range) -> query::Range {
        query::Range {
            start: node.start.map(|e| self.fold_expr(e)),
            end: node.end.map(|e| self.fold_expr(e)),
            inclusive: node.inclusive,
        }
    }

    fn fold_hop_range(&mut self, range: query::HopRange) -> query::HopRange {
        query::HopRange {
            start: range.start.map(|e| self.fold_expr(e)),
            end: range.end.map(|e| self.fold_expr(e)),
            inclusive: range.inclusive,
        }
    }
}

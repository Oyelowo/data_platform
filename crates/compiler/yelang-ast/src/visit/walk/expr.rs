use crate::{item, visit::walk::visitor::Visitor, *};
use std::ops::ControlFlow;

pub fn walk_object<V: Visitor>(v: &mut V, obj: &Object) -> ControlFlow<()> {
    for field in &obj.fields {
        v.visit_ident(&field.key)?;
        v.visit_expr(&field.val)?;
    }
    ControlFlow::Continue(())
}

pub fn walk_expr<V: Visitor>(v: &mut V, expr: &Expr) -> ControlFlow<()> {
    match &expr.kind {
        ExprKind::Binary(b) => v.visit_binary_expr(b),
        ExprKind::Unary(u) => v.visit_unary_expr(u),
        ExprKind::If(i) => v.visit_if_expr(i),
        ExprKind::Let(l) => v.visit_let_expr(l),
        ExprKind::Block(b) => v.visit_block_expr(b),
        ExprKind::Call(c) => v.visit_call_expr(c),
        ExprKind::Async(a) => v.visit_async_expr(a),
        ExprKind::Literal(l) => v.visit_literal(l),
        ExprKind::InterpolatedString(parts) => v.visit_interpolated_string(parts),
        ExprKind::Path(p) => v.visit_path(p),
        ExprKind::MemberAccess(m) => v.visit_member_access(m),
        ExprKind::ArrayAccess(a) => v.visit_array_access(a),
        ExprKind::Query(q) => v.visit_query(q),
        ExprKind::Array(arr) => v.visit_array(arr),
        ExprKind::Object(obj) => v.visit_object(obj),
        ExprKind::Tuple(exprs) => v.visit_tuple_expr(exprs),
        ExprKind::Range(r) => v.visit_range_expr(r),
        ExprKind::Return(opt) => v.visit_return_expr(opt),
        ExprKind::AssignEq(a) => v.visit_assign_eq_expr(a),
        ExprKind::AssignOp(a) => v.visit_assign_op_expr(a),
        ExprKind::DestructureAssign(a) => v.visit_destructure_assign_expr(a),
        ExprKind::Ternary(t) => v.visit_ternary_expr(t),
        ExprKind::Loop(l) => v.visit_loop_expr(l),
        ExprKind::While(w) => v.visit_while_expr(w),
        ExprKind::BindAt(b) => v.visit_bind_at(b),
        ExprKind::DocumentAccess(d) => v.visit_document_access(d),
        ExprKind::ForLoop(f) => v.visit_for_loop_expr(f),
        ExprKind::IsType(i) => v.visit_is_type_expr(i),
        ExprKind::TypeCast(t) => v.visit_type_cast(t),
        ExprKind::TypeAscription(t) => v.visit_type_ascription(t),
        ExprKind::Try(t) => v.visit_try_safe_access(t),
        ExprKind::Lambda(l) => v.visit_lambda_expr(l),
        ExprKind::Struct(s) => v.visit_struct_expr(s),
        ExprKind::Comprehension(c) => v.visit_comprehension_expr(c),
        ExprKind::Match(m) => v.visit_match_expr(m),
        ExprKind::Grouped(g) => v.visit_grouped_expr(g),
        ExprKind::Underscore => ControlFlow::Continue(()),
        ExprKind::Break(b) => v.visit_break_expr_full(b),
        ExprKind::Continue(c) => v.visit_continue_expr(c),
        ExprKind::MethodCall(m) => v.visit_method_call_expr(m),
        ExprKind::Err => ControlFlow::Continue(()),
        ExprKind::Dummy => ControlFlow::Continue(()),
        ExprKind::Gen(g) => v.visit_gen_expr(g),
        ExprKind::Await(a) => v.visit_await_expr(a),
        ExprKind::MacroInvocation(m) => v.visit_macro_invocation(m),
    }
}

// --- Specific Expression Walkers ---

pub fn walk_interpolated_string<V: Visitor>(v: &mut V, parts: &[StringPart]) -> ControlFlow<()> {
    for part in parts {
        if let StringPart::Expr(e) = part {
            v.visit_expr(e)?;
        }
    }
    ControlFlow::Continue(())
}

pub fn walk_binary_expr<V: Visitor>(v: &mut V, bin: &BinaryExpr) -> ControlFlow<()> {
    v.visit_expr(&bin.left)?;
    v.visit_expr(&bin.right)
}

pub fn walk_unary_expr<V: Visitor>(v: &mut V, unary: &UnaryExpr) -> ControlFlow<()> {
    v.visit_expr(&unary.expr)
}

pub fn walk_if_expr<V: Visitor>(v: &mut V, if_expr: &IfExpr) -> ControlFlow<()> {
    v.visit_expr(&if_expr.condition)?;
    v.visit_block_expr(&if_expr.then_block)?;
    if let Some(else_expr) = &if_expr.else_expr {
        v.visit_expr(else_expr)?;
    }
    ControlFlow::Continue(())
}

pub fn walk_let_expr<V: Visitor>(v: &mut V, let_expr: &LetExpr) -> ControlFlow<()> {
    v.visit_pattern(&let_expr.pattern)?;
    v.visit_expr(&let_expr.expr)
}

pub fn walk_block_expr<V: Visitor>(v: &mut V, block: &BlockExpr) -> ControlFlow<()> {
    for stmt in &block.statements {
        v.visit_stmt(stmt)?;
    }
    ControlFlow::Continue(())
}

pub fn walk_call_expr<V: Visitor>(v: &mut V, call: &CallExpr) -> ControlFlow<()> {
    v.visit_expr(&call.callee)?;
    for arg in &call.args {
        match arg {
            CallArgument::Positional(e) => v.visit_expr(e)?,
            CallArgument::Named(id, e) => {
                v.visit_ident(id)?;
                v.visit_expr(e)?;
            }
        }
    }
    ControlFlow::Continue(())
}

pub fn walk_async_expr<V: Visitor>(v: &mut V, async_expr: &AsyncExpr) -> ControlFlow<()> {
    v.visit_block_expr(&async_expr.block)
}

pub fn walk_member_access<V: Visitor>(v: &mut V, access: &MemberAccess) -> ControlFlow<()> {
    v.visit_expr(&access.base)?;
    v.visit_ident(&access.member)
}

pub fn walk_array_access<V: Visitor>(v: &mut V, access: &ArrayAccess) -> ControlFlow<()> {
    v.visit_expr(&access.base)?;
    walk_array_index(v, &access.index)
}

pub fn walk_array_index<V: Visitor>(v: &mut V, index: &ArrayIndex) -> ControlFlow<()> {
    match index {
        ArrayIndex::Single(idx) => v.visit_expr(idx.expr())?,
        ArrayIndex::Range(r) => {
            if let Some(s) = &r.start {
                v.visit_expr(s)?;
            }
            if let Some(e) = &r.end {
                v.visit_expr(e)?;
            }
        }
        ArrayIndex::Filter(e) => v.visit_expr(e)?,
        ArrayIndex::OrderBy(clause) => {
            for part in &clause.orders {
                v.visit_expr(&part.field)?;
            }
        }
        ArrayIndex::GroupBy(selector) => {
            for key in &selector.keys {
                v.visit_ident(&key.name)?;
                v.visit_expr(&key.expr)?;
            }
        }
        ArrayIndex::DistinctBy(expr) => v.visit_expr(expr)?,
        ArrayIndex::Stars { .. } => {}
        ArrayIndex::Enumerate | ArrayIndex::Distinct => {}
    }

    ControlFlow::Continue(())
}

pub fn walk_array<V: Visitor>(v: &mut V, array: &Array) -> ControlFlow<()> {
    use crate::expr::ArrayKind;
    match &array.kind {
        ArrayKind::List(elements) => {
            for elem in elements {
                v.visit_expr(elem)?;
            }
        }
        ArrayKind::Repeat { value, count } => {
            v.visit_expr(value)?;
            v.visit_expr(count)?;
        }
    }
    ControlFlow::Continue(())
}

pub fn walk_ternary_expr<V: Visitor>(v: &mut V, ternary: &TernaryExpr) -> ControlFlow<()> {
    v.visit_expr(&ternary.condition)?;
    v.visit_expr(&ternary.if_true)?;
    v.visit_expr(&ternary.if_false)
}

pub fn walk_grouped_expr<V: Visitor>(v: &mut V, grouped: &GroupedExpr) -> ControlFlow<()> {
    v.visit_expr(&grouped.expr)
}

pub fn walk_range_expr<V: Visitor>(v: &mut V, range: &RangeExpr) -> ControlFlow<()> {
    if let Some(start) = &range.start {
        v.visit_expr(start)?;
    }
    if let Some(end) = &range.end {
        v.visit_expr(end)?;
    }
    ControlFlow::Continue(())
}

pub fn walk_document_access<V: Visitor>(v: &mut V, access: &DocumentAccess) -> ControlFlow<()> {
    v.visit_expr(&access.base)?;
    v.visit_document(&access.object)
}

pub fn walk_document<V: Visitor>(v: &mut V, doc: &Document) -> ControlFlow<()> {
    for field in &doc.fields {
        match field {
            DocumentField::KeyVal(kv) => {
                v.visit_ident(&kv.key)?;
                v.visit_expr(&kv.value)?;
            }
            DocumentField::KeyOnly(ko) => {
                v.visit_ident(&ko.key)?;
            }
            DocumentField::Spread(s) => {
                v.visit_expr(&s.expr)?;
            }
        }
    }
    ControlFlow::Continue(())
}

pub fn walk_bind_at<V: Visitor>(v: &mut V, bind_at: &BindAtExpr) -> ControlFlow<()> {
    v.visit_expr(&bind_at.base)?;
    v.visit_ident(&bind_at.at)
}

pub fn walk_for_loop_expr<V: Visitor>(v: &mut V, for_loop: &ForLoopExpr) -> ControlFlow<()> {
    v.visit_pattern(&for_loop.pat)?;
    v.visit_expr(&for_loop.iter)?;
    v.visit_block_expr(&for_loop.body)
}

pub fn walk_is_type_expr<V: Visitor>(v: &mut V, is_type: &IsTypeExpr) -> ControlFlow<()> {
    v.visit_expr(&is_type.expr)?;
    v.visit_type(&is_type.ty)
}

pub fn walk_type_cast<V: Visitor>(v: &mut V, type_cast: &TypeCast) -> ControlFlow<()> {
    v.visit_expr(&type_cast.base)?;
    v.visit_type(&type_cast.ty)
}

pub fn walk_type_ascription<V: Visitor>(
    v: &mut V,
    type_ascription: &TypeAscription,
) -> ControlFlow<()> {
    v.visit_expr(&type_ascription.expr)?;
    v.visit_type(&type_ascription.ty)
}

pub fn walk_try_safe_access<V: Visitor>(v: &mut V, try_safe: &TrySafeAccess) -> ControlFlow<()> {
    v.visit_expr(&try_safe.base)
}

pub fn walk_lambda_expr<V: Visitor>(v: &mut V, lambda: &LambdaExpr) -> ControlFlow<()> {
    // Visit parameters (pattern and type)
    for param in &lambda.fn_sig.params {
        v.visit_pattern(&param.pattern)?;
        v.visit_type(&param.ty)?;
    }
    // Visit return type if present
    match &lambda.fn_sig.return_type {
        item::FnRefType::Type(ty) => v.visit_type(ty)?,
        item::FnRefType::Default(_) => {}
    }
    v.visit_expr(&lambda.body)
}

pub fn walk_path<V: Visitor>(v: &mut V, path: &Path) -> ControlFlow<()> {
    if let Some(qself) = &path.qself {
        v.visit_type(&qself.ty)?;
        if let Some(trait_path) = &qself.as_trait {
            v.visit_path(trait_path)?;
        }
    }
    for segment in &path.segments {
        v.visit_path_segment(segment)?;
    }
    ControlFlow::Continue(())
}

pub fn walk_struct_expr<V: Visitor>(v: &mut V, struct_expr: &StructExpr) -> ControlFlow<()> {
    v.visit_path(&struct_expr.path)?;
    for field in &struct_expr.fields {
        v.visit_field_assign(field)?;
    }
    if let Some(rest) = &struct_expr.rest {
        v.visit_expr(rest)?;
    }
    ControlFlow::Continue(())
}

pub fn walk_field_assign<V: Visitor>(v: &mut V, field: &FieldAssign) -> ControlFlow<()> {
    v.visit_ident(&field.name)?;
    v.visit_expr(&field.value)
}

pub fn walk_comprehension_expr<V: Visitor>(v: &mut V, comp: &ComprehensionExpr) -> ControlFlow<()> {
    v.visit_expr(&comp.element)?;
    for var in &comp.variables {
        v.visit_pattern(&var.pattern)?;
        v.visit_expr(&var.source)?;
    }
    if let Some(cond) = &comp.condition {
        v.visit_expr(cond)?;
    }
    ControlFlow::Continue(())
}

pub fn walk_match_expr<V: Visitor>(v: &mut V, match_expr: &MatchExpr) -> ControlFlow<()> {
    v.visit_expr(&match_expr.scrutinee)?;
    for arm in &match_expr.arms {
        v.visit_pattern(&arm.pattern)?;
        if let Some(guard) = &arm.guard {
            v.visit_expr(guard)?;
        }
        v.visit_expr(&arm.body)?;
    }
    ControlFlow::Continue(())
}

pub fn walk_method_call_expr<V: Visitor>(
    v: &mut V,
    method_call: &MethodCallExpr,
) -> ControlFlow<()> {
    v.visit_expr(&method_call.receiver)?;

    v.visit_path_segment(&method_call.segment)?;

    for arg in &method_call.arguments {
        match arg {
            crate::CallArgument::Positional(e) => v.visit_expr(e)?,
            crate::CallArgument::Named(_, e) => v.visit_expr(e)?,
        }
    }
    ControlFlow::Continue(())
}

pub fn walk_tuple_expr<V: Visitor>(v: &mut V, exprs: &[Expr]) -> ControlFlow<()> {
    for e in exprs {
        v.visit_expr(e)?;
    }
    ControlFlow::Continue(())
}

pub fn walk_loop_expr<V: Visitor>(v: &mut V, loop_expr: &LoopExpr) -> ControlFlow<()> {
    v.visit_block_expr(&loop_expr.body)
}

pub fn walk_while_expr<V: Visitor>(v: &mut V, while_expr: &WhileExpr) -> ControlFlow<()> {
    v.visit_expr(&while_expr.condition)?;
    v.visit_block_expr(&while_expr.body)
}

pub fn walk_assign_eq_expr<V: Visitor>(v: &mut V, assign: &AssignEqExpr) -> ControlFlow<()> {
    v.visit_expr(&assign.target)?;
    v.visit_expr(&assign.value)
}

pub fn walk_assign_op_expr<V: Visitor>(v: &mut V, assign: &AssignOpExpr) -> ControlFlow<()> {
    v.visit_expr(&assign.target)?;
    v.visit_expr(&assign.value)
}

pub fn walk_destructure_assign_expr<V: Visitor>(
    v: &mut V,
    assign: &DestructureAssignExpr,
) -> ControlFlow<()> {
    v.visit_pattern(&assign.pattern)?;
    v.visit_expr(&assign.value)
}

pub fn walk_return_expr<V: Visitor>(v: &mut V, expr: &Option<Box<Expr>>) -> ControlFlow<()> {
    if let Some(e) = expr {
        v.visit_expr(e)?;
    }
    ControlFlow::Continue(())
}

pub fn walk_break_expr<V: Visitor>(v: &mut V, expr: &Option<Box<Expr>>) -> ControlFlow<()> {
    if let Some(e) = expr {
        v.visit_expr(e)?;
    }
    ControlFlow::Continue(())
}

pub fn walk_gen_expr<V: Visitor>(v: &mut V, expr: &Expr) -> ControlFlow<()> {
    v.visit_expr(expr)
}

pub fn walk_await_expr<V: Visitor>(v: &mut V, expr: &Expr) -> ControlFlow<()> {
    v.visit_expr(expr)
}

pub fn walk_break_expr_full<V: Visitor>(
    v: &mut V,
    break_expr: &expr::BreakExpr,
) -> ControlFlow<()> {
    // Default implementation: just visit the value if present
    if let Some(value) = &break_expr.value {
        v.visit_expr(value)?;
    }
    ControlFlow::Continue(())
}

pub fn walk_continue_expr<V: Visitor>(
    _v: &mut V,
    _continue_expr: &expr::ContinueExpr,
) -> ControlFlow<()> {
    // Default implementation: no-op since continue has no sub-expressions
    ControlFlow::Continue(())
}

pub fn walk_macro_invocation<V: Visitor>(
    v: &mut V,
    inv: &MacroInvocation,
) -> ControlFlow<()> {
    v.visit_path(&inv.path)?;
    match &inv.args {
        crate::expr::MacroArgs::Paren(exprs) | crate::expr::MacroArgs::Bracket(exprs) => {
            for e in exprs {
                v.visit_expr(e)?;
            }
        }
        crate::expr::MacroArgs::Brace(stmts) => {
            for s in stmts {
                v.visit_stmt(s)?;
            }
        }
    }
    ControlFlow::Continue(())
}

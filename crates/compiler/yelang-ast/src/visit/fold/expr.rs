use crate::{
    common::{self, *},
    expr::{self, *},
    item::{self, *},
    pattern::{self, *},
    ptr::{self, *},
    query::{self, *},
    stmt::{self, *},
    tokenizer::{self, *},
    types::{self, *},
    visit::fold::folder::Folder,
};

use crate::expr::{CallArgument, DocumentField, KeyVal, Spread};

pub fn fold_expr<F: Folder + ?Sized>(f: &mut F, expr: Expr) -> Expr {
    let kind = match expr.kind {
        ExprKind::Binary(b) => ExprKind::Binary(f.fold_binary_expr(b)),
        ExprKind::Unary(u) => ExprKind::Unary(f.fold_unary_expr(u)),
        ExprKind::If(i) => ExprKind::If(f.fold_if_expr(i)),
        ExprKind::Let(l) => ExprKind::Let(f.fold_let_expr(l)),
        ExprKind::Block(b) => ExprKind::Block(f.fold_block_expr(b)),
        ExprKind::Call(c) => ExprKind::Call(f.fold_call_expr(c)),
        ExprKind::Async(a) => ExprKind::Async(f.fold_async_expr(a)),
        ExprKind::Literal(l) => ExprKind::Literal(l),
        ExprKind::InterpolatedString(parts) => {
            ExprKind::InterpolatedString(fold_interpolated_string(f, parts))
        }
        ExprKind::Path(p) => ExprKind::Path(f.fold_path(p)),
        ExprKind::MemberAccess(m) => ExprKind::MemberAccess(f.fold_member_access(m)),
        ExprKind::ArrayAccess(a) => ExprKind::ArrayAccess(f.fold_array_access(a)),
        ExprKind::Query(q) => ExprKind::Query(Box::new(f.fold_query(*q))),
        ExprKind::Array(arr) => ExprKind::Array(f.fold_array(arr)),
        ExprKind::Object(obj) => ExprKind::Object(f.fold_object(obj)),
        ExprKind::Tuple(exprs) => ExprKind::Tuple(f.fold_tuple_expr(exprs)),
        ExprKind::Range(r) => ExprKind::Range(f.fold_range_expr(r)),
        ExprKind::Return(opt) => ExprKind::Return(f.fold_return_expr(opt)),
        ExprKind::AssignEq(a) => ExprKind::AssignEq(f.fold_assign_eq_expr(a)),
        ExprKind::AssignOp(a) => ExprKind::AssignOp(f.fold_assign_op_expr(a)),
        ExprKind::DestructureAssign(a) => {
            ExprKind::DestructureAssign(f.fold_destructure_assign_expr(a))
        }
        ExprKind::Ternary(t) => ExprKind::Ternary(f.fold_ternary_expr(t)),
        ExprKind::Loop(l) => ExprKind::Loop(Box::new(f.fold_loop_expr(*l))),
        ExprKind::While(w) => ExprKind::While(f.fold_while_expr(w)),
        ExprKind::BindAt(b) => ExprKind::BindAt(f.fold_bind_at(b)),
        ExprKind::DocumentAccess(d) => ExprKind::DocumentAccess(f.fold_document_access(d)),
        ExprKind::ForLoop(fl) => ExprKind::ForLoop(f.fold_for_loop_expr(fl)),
        ExprKind::IsType(i) => ExprKind::IsType(f.fold_is_type_expr(i)),
        ExprKind::TypeCast(t) => ExprKind::TypeCast(f.fold_type_cast(t)),
        ExprKind::TypeAscription(t) => ExprKind::TypeAscription(f.fold_type_ascription(t)),
        ExprKind::Try(t) => ExprKind::Try(f.fold_try_safe_access(t)),
        ExprKind::Lambda(l) => ExprKind::Lambda(f.fold_lambda_expr(l)),
        ExprKind::Struct(s) => ExprKind::Struct(f.fold_struct_expr(s)),
        ExprKind::Comprehension(c) => ExprKind::Comprehension(f.fold_comprehension_expr(c)),
        ExprKind::Match(m) => ExprKind::Match(Box::new(f.fold_match_expr(*m))),
        ExprKind::Grouped(g) => ExprKind::Grouped(f.fold_grouped_expr(g)),
        ExprKind::MethodCall(m) => ExprKind::MethodCall(f.fold_method_call_expr(m)),
        ExprKind::Gen(g) => ExprKind::Gen(f.fold_gen_expr(g)),
        ExprKind::Await(a) => ExprKind::Await(f.fold_await_expr(a)),
        ExprKind::MacroInvocation(m) => ExprKind::MacroInvocation(f.fold_macro_invocation(m)),
        ExprKind::Underscore => ExprKind::Underscore,
        ExprKind::Break(b) => ExprKind::Break(BreakExpr {
            label: b.label,
            value: b
                .value
                .as_ref()
                .map(|v| Box::new(f.fold_expr(v.as_ref().clone()))),
            span: b.span,
        }),
        ExprKind::Continue(c) => ExprKind::Continue(ContinueExpr {
            label: c.label,
            span: c.span,
        }),
        ExprKind::Err => ExprKind::Err,
        ExprKind::Dummy => ExprKind::Dummy,
    };

    Expr {
        kind,
        span: expr.span,
    }
}

// Helper for Vec<Expr>
pub fn fold_exprs<F: Folder + ?Sized>(f: &mut F, exprs: Vec<Expr>) -> Vec<Expr> {
    exprs.into_iter().map(|e| f.fold_expr(e)).collect()
}

pub fn fold_binary_expr<F: Folder + ?Sized>(f: &mut F, node: BinaryExpr) -> BinaryExpr {
    BinaryExpr {
        left: Box::new(f.fold_expr(*node.left)),
        op: node.op,
        right: Box::new(f.fold_expr(*node.right)),
    }
}

pub fn fold_unary_expr<F: Folder + ?Sized>(f: &mut F, node: UnaryExpr) -> UnaryExpr {
    UnaryExpr {
        op: node.op,
        expr: Box::new(f.fold_expr(*node.expr)),
    }
}

pub fn fold_interpolated_string<F: Folder + ?Sized>(
    f: &mut F,
    parts: Vec<StringPart>,
) -> Vec<StringPart> {
    parts
        .into_iter()
        .map(|part| match part {
            StringPart::Literal(s) => StringPart::Literal(s),
            StringPart::Expr(e) => StringPart::Expr(Box::new(f.fold_expr(*e))),
        })
        .collect()
}

pub fn fold_if_expr<F: Folder + ?Sized>(f: &mut F, node: IfExpr) -> IfExpr {
    IfExpr {
        condition: Box::new(f.fold_expr(*node.condition)),
        then_block: f.fold_block_expr(node.then_block),
        else_expr: node.else_expr.map(|e| Box::new(f.fold_expr(*e))),
    }
}

pub fn fold_let_expr<F: Folder + ?Sized>(f: &mut F, node: LetExpr) -> LetExpr {
    LetExpr {
        pattern: f.fold_pattern(node.pattern),
        expr: Box::new(f.fold_expr(*node.expr)),
    }
}

pub fn fold_block_expr<F: Folder + ?Sized>(f: &mut F, node: BlockExpr) -> BlockExpr {
    BlockExpr {
        label: node.label,
        statements: node
            .statements
            .into_iter()
            .map(|s| f.fold_stmt(s))
            .collect(),
    }
}

pub fn fold_call_expr<F: Folder + ?Sized>(f: &mut F, node: CallExpr) -> CallExpr {
    CallExpr {
        callee: Box::new(f.fold_expr(*node.callee)),
        args: node
            .args
            .into_iter()
            .map(|arg| match arg {
                CallArgument::Positional(e) => CallArgument::Positional(f.fold_expr(e)),
                CallArgument::Named(id, e) => CallArgument::Named(id, f.fold_expr(e)),
            })
            .collect(),
    }
}

pub fn fold_async_expr<F: Folder + ?Sized>(f: &mut F, node: AsyncExpr) -> AsyncExpr {
    AsyncExpr {
        block: Box::new(f.fold_block_expr(*node.block)),
    }
}

pub fn fold_member_access<F: Folder + ?Sized>(f: &mut F, node: MemberAccess) -> MemberAccess {
    MemberAccess {
        base: Box::new(f.fold_expr(*node.base)),
        member: node.member,
    }
}

pub fn fold_array_access<F: Folder + ?Sized>(f: &mut F, node: ArrayAccess) -> ArrayAccess {
    ArrayAccess {
        base: Box::new(f.fold_expr(*node.base)),
        index: match node.index {
            ArrayIndex::Single(idx) => ArrayIndex::Single(crate::expr::Index(Box::new(
                f.fold_expr(idx.expr().clone()),
            ))),
            ArrayIndex::Range(r) => ArrayIndex::Range(crate::expr::RangeItem {
                start: r.start.map(|e| Box::new(f.fold_expr(*e))),
                end: r.end.map(|e| Box::new(f.fold_expr(*e))),
                inclusive: r.inclusive,
            }),
            ArrayIndex::Filter(e) => ArrayIndex::Filter(Box::new(f.fold_expr(*e))),
            ArrayIndex::OrderBy(clause) => ArrayIndex::OrderBy(fold_order_by_clause(f, clause)),
            ArrayIndex::GroupBy(selector) => ArrayIndex::GroupBy(crate::expr::GroupBySelector {
                keys: selector
                    .keys
                    .into_iter()
                    .map(|key| crate::expr::GroupBySelectorKey {
                        name: key.name,
                        expr: f.fold_expr(key.expr),
                    })
                    .collect(),
            }),
            ArrayIndex::DistinctBy(expr) => ArrayIndex::DistinctBy(Box::new(f.fold_expr(*expr))),
            ArrayIndex::Stars { stars } => ArrayIndex::Stars { stars },
            ArrayIndex::Enumerate => ArrayIndex::Enumerate,
            ArrayIndex::Distinct => ArrayIndex::Distinct,
        },
    }
}

pub fn fold_array<F: Folder + ?Sized>(f: &mut F, node: Array) -> Array {
    use crate::expr::ArrayKind;
    let kind = match node.kind {
        ArrayKind::List(elements) => ArrayKind::List(fold_exprs(f, elements)),
        ArrayKind::Repeat { value, count } => ArrayKind::Repeat {
            value: Box::new(f.fold_expr(*value)),
            count: Box::new(f.fold_expr(*count)),
        },
    };
    Array { kind }
}

pub fn fold_ternary_expr<F: Folder + ?Sized>(f: &mut F, node: TernaryExpr) -> TernaryExpr {
    TernaryExpr {
        condition: Box::new(f.fold_expr(*node.condition)),
        if_true: Box::new(f.fold_expr(*node.if_true)),
        if_false: Box::new(f.fold_expr(*node.if_false)),
    }
}

pub fn fold_grouped_expr<F: Folder + ?Sized>(f: &mut F, node: GroupedExpr) -> GroupedExpr {
    GroupedExpr {
        expr: Box::new(f.fold_expr(*node.expr)),
    }
}

pub fn fold_range_expr<F: Folder + ?Sized>(f: &mut F, node: RangeExpr) -> RangeExpr {
    RangeExpr {
        start: node.start.map(|e| Box::new(f.fold_expr(*e))),
        end: node.end.map(|e| Box::new(f.fold_expr(*e))),
        op: node.op,
    }
}

pub fn fold_bind_at<F: Folder + ?Sized>(f: &mut F, node: BindAtExpr) -> BindAtExpr {
    BindAtExpr {
        base: Box::new(f.fold_expr(*node.base)),
        at: node.at,
    }
}

pub fn fold_for_loop_expr<F: Folder + ?Sized>(f: &mut F, node: ForLoopExpr) -> ForLoopExpr {
    ForLoopExpr {
        pat: f.fold_pattern(node.pat),
        label: node.label,
        iter: Box::new(f.fold_expr(*node.iter)),
        body: f.fold_block_expr(node.body),
    }
}

pub fn fold_is_type_expr<F: Folder + ?Sized>(f: &mut F, node: IsTypeExpr) -> IsTypeExpr {
    IsTypeExpr {
        expr: Box::new(f.fold_expr(*node.expr)),
        ty: f.fold_type(node.ty),
    }
}

pub fn fold_type_cast<F: Folder + ?Sized>(f: &mut F, node: TypeCast) -> TypeCast {
    TypeCast {
        base: Box::new(f.fold_expr(*node.base)),
        ty: f.fold_type(node.ty),
    }
}

pub fn fold_type_ascription<F: Folder + ?Sized>(f: &mut F, node: TypeAscription) -> TypeAscription {
    TypeAscription {
        expr: Box::new(f.fold_expr(*node.expr)),
        ty: f.fold_type(node.ty),
    }
}

pub fn fold_try_safe_access<F: Folder + ?Sized>(f: &mut F, node: TrySafeAccess) -> TrySafeAccess {
    TrySafeAccess {
        base: Box::new(f.fold_expr(*node.base)),
        op: node.op,
    }
}

pub fn fold_lambda_expr<F: Folder + ?Sized>(f: &mut F, node: LambdaExpr) -> LambdaExpr {
    LambdaExpr {
        header_span: node.header_span,
        fn_sig: item::FnSig {
            params: node
                .fn_sig
                .params
                .into_iter()
                .map(|param| item::Param {
                    pattern: f.fold_pattern(param.pattern),
                    ty: f.fold_type(param.ty),
                    span: param.span,
                })
                .collect(),
            return_type: match node.fn_sig.return_type {
                item::FnRefType::Type(ty) => item::FnRefType::Type(f.fold_type(ty)),
                item::FnRefType::Default(span) => item::FnRefType::Default(span),
            },
            is_async: node.fn_sig.is_async,
            is_variadic: node.fn_sig.is_variadic,
        },
        body: Box::new(f.fold_expr(*node.body)),
    }
}

pub fn fold_struct_expr<F: Folder + ?Sized>(f: &mut F, node: StructExpr) -> StructExpr {
    StructExpr {
        path: f.fold_path(node.path),
        fields: node
            .fields
            .into_iter()
            .map(|field| f.fold_field_assign(field))
            .collect(),
        rest: node.rest.map(|e| Box::new(f.fold_expr(*e))),
    }
}

pub fn fold_field_assign<F: Folder + ?Sized>(f: &mut F, field: FieldAssign) -> FieldAssign {
    FieldAssign {
        name: field.name,
        value: f.fold_expr(field.value),
        is_shorthand: field.is_shorthand,
        span: field.span,
    }
}

pub fn fold_comprehension_expr<F: Folder + ?Sized>(
    f: &mut F,
    node: ComprehensionExpr,
) -> ComprehensionExpr {
    ComprehensionExpr {
        element: Box::new(f.fold_expr(*node.element)),
        variables: node
            .variables
            .into_iter()
            .map(|var| ComprehensionVar {
                pattern: f.fold_pattern(var.pattern),
                source: Box::new(f.fold_expr(*var.source)),
            })
            .collect(),
        condition: node.condition.map(|e| Box::new(f.fold_expr(*e))),
    }
}

pub fn fold_match_expr<F: Folder + ?Sized>(f: &mut F, node: MatchExpr) -> MatchExpr {
    MatchExpr {
        scrutinee: Box::new(f.fold_expr(*node.scrutinee)),
        arms: node
            .arms
            .into_iter()
            .map(|arm| MatchArm {
                pattern: f.fold_pattern(arm.pattern),
                guard: arm.guard.map(|e| Box::new(f.fold_expr(*e))),
                body: Box::new(f.fold_expr(*arm.body)),
                span: arm.span,
            })
            .collect(),
    }
}

// Helpers

pub fn fold_object<F: Folder + ?Sized>(f: &mut F, obj: Object) -> Object {
    Object {
        fields: obj
            .fields
            .into_iter()
            .map(|field| ObjectField {
                key: field.key,
                val: f.fold_expr(field.val),
            })
            .collect(),
        span: obj.span,
    }
}

pub fn fold_assign_eq_expr<F: Folder + ?Sized>(f: &mut F, a: AssignEqExpr) -> AssignEqExpr {
    AssignEqExpr {
        target: Box::new(f.fold_expr(*a.target)),
        value: Box::new(f.fold_expr(*a.value)),
    }
}

pub fn fold_assign_op_expr<F: Folder + ?Sized>(f: &mut F, a: AssignOpExpr) -> AssignOpExpr {
    AssignOpExpr {
        target: Box::new(f.fold_expr(*a.target)),
        value: Box::new(f.fold_expr(*a.value)),
        op: a.op,
    }
}

pub fn fold_destructure_assign_expr<F: Folder + ?Sized>(
    f: &mut F,
    a: DestructureAssignExpr,
) -> DestructureAssignExpr {
    DestructureAssignExpr {
        pattern: f.fold_pattern(a.pattern),
        value: Box::new(f.fold_expr(*a.value)),
    }
}

pub fn fold_loop_expr<F: Folder + ?Sized>(f: &mut F, l: LoopExpr) -> LoopExpr {
    LoopExpr {
        label: l.label,
        body: Box::new(f.fold_block_expr(*l.body)),
    }
}

pub fn fold_while_expr<F: Folder + ?Sized>(f: &mut F, w: WhileExpr) -> WhileExpr {
    WhileExpr {
        label: w.label,
        condition: Box::new(f.fold_expr(*w.condition)),
        body: f.fold_block_expr(w.body),
    }
}

pub fn fold_tuple_expr<F: Folder + ?Sized>(f: &mut F, exprs: Vec<Expr>) -> Vec<Expr> {
    exprs.into_iter().map(|e| f.fold_expr(e)).collect()
}

pub fn fold_return_expr<F: Folder + ?Sized>(
    f: &mut F,
    expr: Option<Box<Expr>>,
) -> Option<Box<Expr>> {
    expr.map(|e| Box::new(f.fold_expr(*e)))
}

pub fn fold_break_expr<F: Folder + ?Sized>(
    f: &mut F,
    expr: Option<Box<Expr>>,
) -> Option<Box<Expr>> {
    expr.map(|e| Box::new(f.fold_expr(*e)))
}

pub fn fold_gen_expr<F: Folder + ?Sized>(f: &mut F, expr: Box<Expr>) -> Box<Expr> {
    Box::new(f.fold_expr(*expr))
}

pub fn fold_await_expr<F: Folder + ?Sized>(f: &mut F, expr: Box<Expr>) -> Box<Expr> {
    Box::new(f.fold_expr(*expr))
}

pub fn fold_document_access<F: Folder + ?Sized>(f: &mut F, d: DocumentAccess) -> DocumentAccess {
    DocumentAccess {
        base: Box::new(f.fold_expr(*d.base)),
        object: f.fold_document(d.object),
    }
}

pub fn fold_document<F: Folder + ?Sized>(f: &mut F, doc: Document) -> Document {
    Document {
        fields: doc
            .fields
            .into_iter()
            .map(|field| match field {
                DocumentField::KeyVal(kv) => DocumentField::KeyVal(KeyVal {
                    key: kv.key,
                    value: f.fold_expr(kv.value),
                }),
                DocumentField::KeyOnly(ko) => DocumentField::KeyOnly(ko),
                DocumentField::Spread(s) => DocumentField::Spread(Spread {
                    expr: f.fold_expr(s.expr),
                }),
            })
            .collect(),
        span: doc.span,
    }
}

pub fn fold_method_call_expr<F: Folder + ?Sized>(f: &mut F, m: MethodCallExpr) -> MethodCallExpr {
    MethodCallExpr {
        receiver: Box::new(f.fold_expr(*m.receiver)),
        segment: f.fold_path_segment(m.segment),
        arguments: m
            .arguments
            .into_iter()
            .map(|arg| match arg {
                CallArgument::Positional(e) => CallArgument::Positional(f.fold_expr(e)),
                CallArgument::Named(id, e) => CallArgument::Named(id, f.fold_expr(e)),
            })
            .collect(),
    }
}

fn fold_order_by_clause<F: Folder + ?Sized>(f: &mut F, clause: OrderByClause) -> OrderByClause {
    OrderByClause {
        orders: clause
            .orders
            .into_iter()
            .map(|part| OrderByPart {
                field: f.fold_expr(part.field),
                direction: part.direction,
            })
            .collect(),
    }
}

pub fn fold_path<F: Folder + ?Sized>(f: &mut F, path: Path) -> Path {
    Path {
        qself: path.qself.map(|q| {
            Box::new(crate::expr::QSelf {
                ty: f.fold_type(q.ty),
                as_trait: q.as_trait.map(|p| Box::new(f.fold_path(*p))),
                span: q.span,
            })
        }),
        segments: path
            .segments
            .into_iter()
            .map(|segment| f.fold_path_segment(segment))
            .collect(),
        is_absolute: path.is_absolute,
        span: path.span,
    }
}

pub fn fold_macro_invocation<F: Folder + ?Sized>(
    f: &mut F,
    m: crate::expr::MacroInvocation,
) -> crate::expr::MacroInvocation {
    crate::expr::MacroInvocation {
        path: f.fold_path(m.path),
        args: m.args,
        span: m.span,
    }
}

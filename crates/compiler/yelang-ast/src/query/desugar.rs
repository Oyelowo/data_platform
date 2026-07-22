//! Desugar query-syntax aggregate calls into `Queryable` method calls.
//!
//! This pass runs on the AST before name resolution. It transforms projections
//! such as `select sum(x) from xs@x` into method-call form so that the rest of
//! the compiler pipeline (resolution, type checking, QIR lowering) only needs
//! to understand `Queryable` methods.
//!
//! The transformation is local to `select` projections and preserves all other
//! expression structure.
//!
//! Examples:
//!
//! ```text
//! select sum(x) from xs@x
//!     => xs.sum()
//!
//! select sum(x * 10) from xs@x where x > 2
//!     => xs.filter(|x| x > 2).map(|x| x * 10).sum()
//!
//! select count(x) from xs@x
//!     => xs.count()
//! ```

use crate::{
    Array, ArrayKind, CallArgument, Expr, ExprKind, FieldAssign, FnRefType, FromNode, Ident,
    LambdaExpr, MethodCallExpr, Object, Param, Path, PathSegment, Pattern, PatternKind, SelectQ,
    StructExpr, Type, TypeKind,
};
use crate::item::FnSig;
use crate::visit::fold::folder::Folder;
use crate::visit::fold::query;
use yelang_interner::Interner;
use yelang_lexer::Span;

/// Aggregate names recognized in query projections.
const AGGREGATES: &[&str] = &["sum", "avg", "count", "product", "min", "max"];

/// Run the aggregate desugaring pass over an entire program.
pub fn desugar_query_aggregates(program: &mut crate::Program, interner: &Interner) {
    let mut folder = QueryAggregateFolder { interner };
    *program = folder.fold_program(program.clone());
}

struct QueryAggregateFolder<'a> {
    interner: &'a Interner,
}

impl Folder for QueryAggregateFolder<'_> {
    fn fold_expr(&mut self, node: Expr) -> Expr {
        // Detect `select aggregate(x) from source@x` and replace the whole query
        // expression with the equivalent `Queryable` method-call chain. This
        // preserves the semantics (a single scalar result) instead of leaving a
        // query that projects the aggregate once per row.
        let is_aggregate_query = if let ExprKind::Query(q) = &node.kind {
            matches!(&q.kind, crate::query::QueryKind::Select(sq)
                if sq.from.len() == 1 && as_aggregate_call(self.interner, &sq.projection).is_some())
        } else {
            false
        };

        let folded = crate::visit::fold::expr::fold_expr(self, node);

        if is_aggregate_query {
            if let ExprKind::Query(q) = folded.kind {
                if let crate::query::QueryKind::Select(sq) = q.kind {
                    return sq.projection;
                }
                // Not a select after folding; reconstruct the query expression.
                return Expr {
                    kind: ExprKind::Query(q),
                    span: folded.span,
                };
            }
        }

        folded
    }

    fn fold_select_stmt(&mut self, stmt: SelectQ) -> SelectQ {
        // Fold all non-projection parts first so that any nested queries inside
        // where/group/order clauses are also desugared.
        let from: Vec<FromNode> = stmt.from.into_iter().map(|n| self.fold_from_node(n)).collect();
        let links = stmt
            .links
            .into_iter()
            .map(|l| self.fold_select_linkpath(l))
            .collect();
        let post_links_for = stmt
            .post_links_for
            .into_iter()
            .map(|b| crate::query::ForRootModifiers {
                target: self.fold_ident(b.target),
                modifiers: query::fold_modifiers(self, b.modifiers),
            })
            .collect();
        let where_clause = stmt.where_clause.map(|w| self.fold_expr(w));
        let group_by = stmt.group_by.map(|g| crate::query::GroupByClause {
            keys: g
                .keys
                .into_iter()
                .map(|k| crate::query::GroupByKey {
                    name: k.name,
                    expr: self.fold_expr(k.expr),
                })
                .collect(),
            into: g.into,
        });
        let order_by = stmt.order_by.map(|o| {
            o.into_iter()
                .map(|p| self.fold_select_order_by_part(p))
                .collect()
        });
        let range = stmt.range.map(|r| self.fold_select_range(r));

        // Transform aggregate calls in the projection. This must happen after
        // the where-clause has been folded so we can push it into a filter.
        let projection = if from.len() == 1 {
            if let (Some(source_var), Some(binder)) = (from[0].var, from[0].bind) {
                transform_projection(
                    self.interner,
                    &stmt.projection,
                    source_var,
                    binder,
                    where_clause.as_ref(),
                )
            } else {
                stmt.projection
            }
        } else {
            stmt.projection
        };

        SelectQ {
            projection: self.fold_expr(projection),
            from,
            links_match_kind: stmt.links_match_kind,
            links,
            post_links_for,
            where_clause,
            group_by,
            order_by,
            range,
        }
    }
}

/// Transform aggregate calls in a projection expression into `Queryable` method
/// calls on the source collection.
fn transform_projection(
    interner: &Interner,
    expr: &Expr,
    source_var: Ident,
    binder: Ident,
    where_clause: Option<&Expr>,
) -> Expr {
    if let Some((name, arg)) = as_aggregate_call(interner, expr) {
        return build_aggregate_chain(interner, source_var, binder, where_clause, name, arg);
    }

    match &expr.kind {
        ExprKind::Struct(s) => {
            let mut fields = Vec::new();
            for field in &s.fields {
                fields.push(FieldAssign {
                    name: field.name,
                    value: transform_projection(
                        interner,
                        &field.value,
                        source_var,
                        binder,
                        where_clause,
                    ),
                    is_shorthand: field.is_shorthand,
                    span: field.span,
                });
            }
            Expr {
                kind: ExprKind::Struct(StructExpr {
                    path: s.path.clone(),
                    fields,
                    rest: s.rest.clone(),
                }),
                span: expr.span,
            }
        }
        ExprKind::Object(o) => {
            let mut fields = Vec::new();
            for field in &o.fields {
                fields.push(crate::ObjectField::new(
                    field.key,
                    transform_projection(interner, field.value(), source_var, binder, where_clause)
                        .clone(),
                ));
            }
            Expr {
                kind: ExprKind::Object(Object { fields, span: o.span }),
                span: expr.span,
            }
        }
        ExprKind::Tuple(elements) => {
            let mut new_elements = Vec::new();
            for elem in elements {
                new_elements.push(transform_projection(
                    interner, elem, source_var, binder, where_clause,
                ));
            }
            Expr {
                kind: ExprKind::Tuple(new_elements),
                span: expr.span,
            }
        }
        ExprKind::Array(a) => {
            let elements = match &a.kind {
                ArrayKind::List(elems) => elems
                    .iter()
                    .map(|e| transform_projection(interner, e, source_var, binder, where_clause))
                    .collect(),
                ArrayKind::Repeat { value, count } => {
                    vec![
                        transform_projection(interner, value, source_var, binder, where_clause),
                        transform_projection(interner, count, source_var, binder, where_clause),
                    ]
                }
            };
            Expr {
                kind: ExprKind::Array(Array {
                    kind: ArrayKind::List(elements),
                }),
                span: expr.span,
            }
        }
        _ => expr.clone(),
    }
}

/// If `expr` is a call to a recognized aggregate function, return its name and
/// single argument.
fn as_aggregate_call<'a>(interner: &Interner, expr: &'a Expr) -> Option<(&'static str, &'a Expr)> {
    let ExprKind::Call(call) = &expr.kind else {
        return None;
    };
    let ExprKind::Path(path) = &call.callee.kind else {
        return None;
    };
    if path.segments.len() != 1 {
        return None;
    }
    let name = path.segments[0].ident.as_str(interner);
    let agg_name = AGGREGATES.iter().copied().find(|&n| n == name)?;
    if call.args.len() != 1 {
        return None;
    }
    let CallArgument::Positional(arg) = &call.args[0] else {
        return None;
    };
    Some((agg_name, arg))
}

/// Build a method-call chain that computes the aggregate over the source.
fn build_aggregate_chain(
    interner: &Interner,
    source_var: Ident,
    binder: Ident,
    where_clause: Option<&Expr>,
    agg_name: &str,
    arg: &Expr,
) -> Expr {
    let span = arg.span;

    // Base receiver: the source collection variable.
    let mut receiver = path_expr(source_var, span);

    // Push a filter if there is a where clause.
    if let Some(cond) = where_clause {
        let pred = lambda_expr(binder, cond.clone(), span);
        receiver = method_call_expr(interner, receiver, "filter", vec![pred], span);
    }

    // Push a map if the aggregate argument is not exactly the element binder.
    if !is_path_to(arg, binder) {
        let mapper = lambda_expr(binder, arg.clone(), span);
        receiver = method_call_expr(interner, receiver, "map", vec![mapper], span);
    }

    method_call_expr(interner, receiver, agg_name, vec![], span)
}

fn is_path_to(expr: &Expr, ident: Ident) -> bool {
    let ExprKind::Path(path) = &expr.kind else {
        return false;
    };
    path.segments.len() == 1 && path.segments[0].ident.symbol == ident.symbol
}

fn path_expr(ident: Ident, span: Span) -> Expr {
    Expr {
        kind: ExprKind::Path(Path {
            qself: None,
            segments: vec![PathSegment {
                ident,
                args: None,
            }],
            is_absolute: false,
            span,
        }),
        span,
    }
}

fn method_call_expr(
    interner: &Interner,
    receiver: Expr,
    method: &str,
    args: Vec<Expr>,
    span: Span,
) -> Expr {
    Expr {
        kind: ExprKind::MethodCall(MethodCallExpr {
            receiver: Box::new(receiver),
            segment: PathSegment {
                ident: Ident::new(interner.intern(method), span),
                args: None,
            },
            arguments: args.into_iter().map(CallArgument::Positional).collect(),
        }),
        span,
    }
}

fn lambda_expr(param: Ident, body: Expr, span: Span) -> Expr {
    let param_pattern = Pattern {
        pattern: PatternKind::Binding {
            name: param,
            mutability: crate::Mutability::Immutable,
            subpattern: None,
        },
        span,
    };
    let param_ty = Type {
        kind: TypeKind::Infer,
        span,
    };
    Expr {
        kind: ExprKind::Lambda(LambdaExpr {
            header_span: span,
            fn_sig: FnSig {
                params: vec![Param {
                    pattern: param_pattern,
                    ty: param_ty,
                    span,
                }],
                return_type: FnRefType::Default(span),
                is_async: false,
                is_variadic: false,
                abi: None,
            },
            body: Box::new(body),
        }),
        span,
    }
}

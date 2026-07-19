//! In-memory interpreter for physical QIR plans.

use yelang_interner::Symbol;

use crate::errors::PlanError;
use crate::exec::interface::{QueryExecutor, Value};
use crate::exec::kernels::KernelRegistry;
use crate::exec::value::value_eq;
use crate::expr::{Direction, OrderKey, QExpr, QExprId, QLit};
use crate::pir::operator::{AggMode, PirOp};
use crate::pir::plan::PhysicalPlan;
use crate::pir::props::PhysicalProps;

/// In-memory query executor.
#[derive(Debug, Default)]
pub struct MemoryExecutor {
    kernels: KernelRegistry,
}

impl MemoryExecutor {
    /// Create a new in-memory executor.
    pub fn new() -> Self {
        Self {
            kernels: KernelRegistry::new(),
        }
    }
}

impl QueryExecutor for MemoryExecutor {
    type Error = PlanError;

    fn execute(&self, plan: &PhysicalPlan) -> Result<Value, Self::Error> {
        let Some(root) = plan.root else {
            return Ok(Value::Array(vec![]));
        };
        let ctx = ExecCtx {
            plan,
            kernels: &self.kernels,
            row: Value::Null,
        };
        ctx.execute(root, &PhysicalProps::any())
    }
}

struct ExecCtx<'a> {
    plan: &'a PhysicalPlan,
    kernels: &'a KernelRegistry,
    row: Value,
}

impl<'a> ExecCtx<'a> {
    fn execute(&self, id: crate::ids::PirId, _required: &PhysicalProps) -> Result<Value, PlanError> {
        match self.plan.operator(id) {
            PirOp::TableScan { source, .. } => self.scan(source),
            PirOp::Values { rows } => {
                let values: Result<Vec<_>, _> = rows.iter().map(|&e| self.eval(e)).collect();
                Ok(Value::Array(values?))
            }
            PirOp::Filter { input, predicate } => {
                let rows = self.execute(*input, _required)?.into_array()?;
                let mut out = Vec::new();
                for row in rows {
                    let ctx = self.with_row(row.clone());
                    let pred = ctx.apply_closure(*predicate)?;
                    if pred.to_bool() {
                        out.push(row);
                    }
                }
                Ok(Value::Array(out))
            }
            PirOp::Project { input, projection } => {
                let rows = self.execute(*input, _required)?.into_array()?;
                let mut out = Vec::new();
                for row in rows {
                    let ctx = self.with_row(row);
                    out.push(ctx.apply_closure(*projection)?);
                }
                Ok(Value::Array(out))
            }
            PirOp::FlatMap { input, projection } => {
                let rows = self.execute(*input, _required)?.into_array()?;
                let mut out = Vec::new();
                for row in rows {
                    let ctx = self.with_row(row);
                    let mapped = ctx.apply_closure(*projection)?;
                    out.extend(mapped.into_array()?);
                }
                Ok(Value::Array(out))
            }
            PirOp::Sort { input, keys } => {
                let mut rows = self.execute(*input, _required)?.into_array()?;
                self.sort_rows(&mut rows, keys);
                Ok(Value::Array(rows))
            }
            PirOp::TopK { input, keys, k } => {
                let mut rows = self.execute(*input, _required)?.into_array()?;
                self.sort_rows(&mut rows, keys);
                rows.truncate(*k);
                Ok(Value::Array(rows))
            }
            PirOp::Slice { input, offset, limit } => {
                let rows = self.execute(*input, _required)?.into_array()?;
                let end = limit.map(|l| offset + l).unwrap_or(rows.len());
                Ok(Value::Array(rows.into_iter().skip(*offset).take(end - offset).collect()))
            }
            PirOp::Distinct { input, .. } => {
                let rows = self.execute(*input, _required)?.into_array()?;
                let mut out = Vec::new();
                for row in rows {
                    if !out.iter().any(|r| value_eq(r, &row)) {
                        out.push(row);
                    }
                }
                Ok(Value::Array(out))
            }
            PirOp::GroupBy { input, key } => {
                let rows = self.execute(*input, _required)?.into_array()?;
                let mut groups: Vec<(Value, Vec<Value>)> = Vec::new();
                for row in rows {
                    let ctx = self.with_row(row.clone());
                    let k = ctx.apply_closure(*key)?;
                    if let Some((_, vals)) = groups.iter_mut().find(|(gk, _)| value_eq(gk, &k)) {
                        vals.push(row);
                    } else {
                        groups.push((k, vec![row]));
                    }
                }
                let out: Vec<Value> = groups
                    .into_iter()
                    .map(|(k, vals)| {
                        Value::Record(vec![
                            (Symbol::from(1), k),
                            (Symbol::from(2), Value::Array(vals)),
                        ])
                    })
                    .collect();
                Ok(Value::Array(out))
            }
            PirOp::HashAggregate { input, group_keys, aggregates, mode } => {
                let rows = self.execute(*input, _required)?.into_array()?;
                self.hash_aggregate(rows, group_keys, aggregates, mode)
            }
            PirOp::NestedLoopJoin { outer, inner, predicate, kind } => {
                let outer_rows = self.execute(*outer, _required)?.into_array()?;
                let inner_rows = self.execute(*inner, _required)?.into_array()?;
                let mut out = Vec::new();
                for o in &outer_rows {
                    let mut matched = false;
                    for i in &inner_rows {
                        let pair = Value::Record(vec![
                            (Symbol::from(1), o.clone()),
                            (Symbol::from(2), i.clone()),
                        ]);
                        let include = if let Some(pred) = predicate {
                            let ctx = self.with_row(pair.clone());
                            ctx.apply_closure(*pred)?.to_bool()
                        } else {
                            true
                        };
                        if include {
                            matched = true;
                            if *kind != crate::logical::operator::JoinKind::Anti {
                                out.push(pair);
                            }
                        }
                    }
                    if !matched && (*kind == crate::logical::operator::JoinKind::Left || *kind == crate::logical::operator::JoinKind::Full) {
                        out.push(o.clone());
                    }
                }
                Ok(Value::Array(out))
            }
            PirOp::UnionAll { inputs } | PirOp::Union { inputs } => {
                let mut out = Vec::new();
                for id in inputs {
                    out.extend(self.execute(*id, _required)?.into_array()?);
                }
                if matches!(self.plan.operator(id), PirOp::Union { .. }) {
                    let mut deduped = Vec::new();
                    for r in out {
                        if !deduped.iter().any(|x| value_eq(x, &r)) {
                            deduped.push(r);
                        }
                    }
                    out = deduped;
                }
                Ok(Value::Array(out))
            }
            PirOp::Construct { kind, fields } => {
                let field_values: Result<Vec<_>, _> = fields
                    .iter()
                    .map(|(name, id)| Ok((*name, self.execute(*id, _required)?)))
                    .collect();
                match kind {
                    crate::logical::operator::ConstructKind::Record => Ok(Value::Record(field_values?)),
                    crate::logical::operator::ConstructKind::Tuple => Ok(Value::Array(field_values?.into_iter().map(|(_, v)| v).collect())),
                    crate::logical::operator::ConstructKind::Array => Ok(Value::Array(field_values?.into_iter().map(|(_, v)| v).collect())),
                    crate::logical::operator::ConstructKind::Facet => Ok(Value::Record(field_values?)),
                }
            }
            PirOp::AttachField { input, field, value_plan } => {
                let mut record = self.execute(*input, _required)?.into_record()?;
                let value = self.execute(*value_plan, _required)?;
                record.push((*field, value));
                Ok(Value::Record(record))
            }
            PirOp::Expr(expr) => self.eval(*expr),
            _ => Ok(Value::Array(vec![])),
        }
    }

    fn scan(&self, source: &crate::logical::operator::ScanSource) -> Result<Value, PlanError> {
        match source {
            crate::logical::operator::ScanSource::Expr(expr) => self.eval(*expr),
            crate::logical::operator::ScanSource::Named(_) => Ok(Value::Array(vec![])),
        }
    }

    fn eval(&self, expr: QExprId) -> Result<Value, PlanError> {
        match self.plan.expr(expr) {
            QExpr::Lit(QLit::Int(n), _) => Ok(Value::Int(*n)),
            QExpr::Lit(QLit::Bool(b), _) => Ok(Value::Bool(*b)),
            QExpr::Lit(QLit::Float(f), _) => Ok(Value::Float(*f)),
            QExpr::Lit(QLit::Str(s), _) => Ok(Value::Str(*s)),
            QExpr::Lit(QLit::Unit, _) => Ok(Value::Null),
            QExpr::Column(_, _) => Ok(self.row.clone()),
            QExpr::Field(base, field, _) => {
                let base = self.eval(*base)?;
                Ok(base.field(*field).cloned().unwrap_or(Value::Null))
            }
            QExpr::Binary(op, l, r, _) => {
                let left = self.eval(*l)?;
                let right = self.eval(*r)?;
                Ok(self.kernels.eval_binary(*op, left, right))
            }
            QExpr::Unary(op, e, _) => {
                let v = self.eval(*e)?;
                Ok(self.kernels.eval_unary(*op, v))
            }
            QExpr::Record(fields, _) => {
                let vals: Result<Vec<_>, _> = fields
                    .iter()
                    .map(|(name, e)| Ok((*name, self.eval(*e)?)))
                    .collect();
                Ok(Value::Record(vals?))
            }
            QExpr::Tuple(elems, _) => {
                let vals: Result<Vec<_>, _> = elems.iter().map(|e| self.eval(*e)).collect();
                Ok(Value::Array(vals?))
            }
            QExpr::Array(elems, _) => {
                let vals: Result<Vec<_>, _> = elems.iter().map(|e| self.eval(*e)).collect();
                Ok(Value::Array(vals?))
            }
            QExpr::Closure { .. } => Ok(Value::Null), // closures are applied, not returned
            QExpr::If(c, t, e, _) => {
                if self.eval(*c)?.to_bool() {
                    self.eval(*t)
                } else {
                    self.eval(*e)
                }
            }
            _ => Ok(Value::Null),
        }
    }

    fn apply_closure(&self, expr: QExprId) -> Result<Value, PlanError> {
        match self.plan.expr(expr) {
            QExpr::Closure { params, body, .. } => {
                if params.len() == 1 {
                    let ctx = self.with_row(self.row.clone());
                    ctx.eval(*body)
                } else {
                    self.eval(*body)
                }
            }
            _ => self.eval(expr),
        }
    }

    fn with_row(&self, row: Value) -> ExecCtx<'a> {
        ExecCtx { plan: self.plan, kernels: self.kernels, row }
    }

    fn sort_rows(&self, rows: &mut Vec<Value>, keys: &[OrderKey]) {
        rows.sort_by(|a, b| {
            for key in keys {
                let av = eval_key(self.plan, self.kernels, a, key);
                let bv = eval_key(self.plan, self.kernels, b, key);
                let ord = compare_values(&av, &bv);
                let ord = match key.dir {
                    Direction::Asc => ord,
                    Direction::Desc => ord.reverse(),
                };
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
            std::cmp::Ordering::Equal
        });
    }

    fn hash_aggregate(
        &self,
        rows: Vec<Value>,
        group_keys: &[QExprId],
        aggregates: &[crate::pir::operator::PhysicalAggregateOp],
        _mode: &AggMode,
    ) -> Result<Value, PlanError> {
        if group_keys.is_empty() {
            // Scalar aggregate: one result row.
            let mut agg_values = Vec::new();
            for agg in aggregates {
                let vals: Result<Vec<Value>, _> = rows.iter().map(|r| {
                    let ctx = self.with_row(r.clone());
                    ctx.apply_closure(agg.input_expr)
                }).collect();
                agg_values.push((Symbol::from(0), self.kernels.eval_aggregate(agg.class, vals?)));
            }
            Ok(Value::Record(agg_values))
        } else {
            // Grouped aggregate.
            let mut groups: Vec<(Vec<Value>, Vec<Value>)> = Vec::new();
            for row in rows {
                let ctx = self.with_row(row.clone());
                let key: Result<Vec<_>, _> = group_keys.iter().map(|k| ctx.apply_closure(*k)).collect();
                let key = key?;
                if let Some((_, rs)) = groups.iter_mut().find(|(gk, _)| {
                    gk.len() == key.len() && gk.iter().zip(&key).all(|(a, b)| value_eq(a, b))
                }) {
                    rs.push(row);
                } else {
                    groups.push((key, vec![row]));
                }
            }
            let out: Result<Vec<_>, _> = groups
                .into_iter()
                .map(|(k, rs)| {
                    let mut fields = Vec::new();
                    for (i, _gk) in group_keys.iter().enumerate() {
                        fields.push((Symbol::from(i as u32 + 100), k[i].clone()));
                    }
                    for agg in aggregates {
                        let vals: Result<Vec<Value>, _> = rs.iter().map(|r| {
                            let ctx = self.with_row(r.clone());
                            ctx.apply_closure(agg.input_expr)
                        }).collect();
                        fields.push((Symbol::from(0), self.kernels.eval_aggregate(agg.class, vals?)));
                    }
                    Ok(Value::Record(fields))
                })
                .collect();
            Ok(Value::Array(out?))
        }
    }
}

impl Value {
    fn into_array(self) -> Result<Vec<Value>, PlanError> {
        match self {
            Value::Array(a) => Ok(a),
            _ => Err(PlanError::Execution("expected array".to_string())),
        }
    }

    fn into_record(self) -> Result<Vec<(Symbol, Value)>, PlanError> {
        match self {
            Value::Record(r) => Ok(r),
            _ => Err(PlanError::Execution("expected record".to_string())),
        }
    }
}

fn eval_key(plan: &PhysicalPlan, kernels: &KernelRegistry, row: &Value, key: &OrderKey) -> Value {
    let ctx = ExecCtx { plan, kernels, row: row.clone() };
    ctx.apply_closure(key.expr).unwrap_or(Value::Null)
}

fn compare_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal),
        (Value::Str(x), Value::Str(y)) => x.as_usize().cmp(&y.as_usize()),
        _ => std::cmp::Ordering::Equal,
    }
}



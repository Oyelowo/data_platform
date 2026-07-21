//! End-to-end compiler driver.
//!
//! A `Driver` takes Yelang source strings, runs the full frontend and query
//! pipeline, and returns either a compiled crate or the result of executing the
//! first query found in `main`.

use yelang_ast::Program;
use yelang_hir::hir::expr::Expr;
use yelang_hir::hir::item::ItemKind;
use yelang_hir::ids::{BodyId, ExprId, QueryId};
use yelang_hir::lowering::context::lower_crate;
use yelang_interner::Interner;
use yelang_lexer::TokenKind;
use yelang_qir::backend::MemoryBackend;
use yelang_qir::exec::{MemoryExecutor, QueryExecutor, Value};
use yelang_qir::lir::plan::LogicalPlan;
use yelang_qir::{lower_query, plan_logical as plan_logical_fn};
use yelang_resolve::resolve_crate;
use yelang_tycheck::tcx::TyCtxt;
use yelang_tycheck::type_check_crate;

use crate::error::{DriverError, QueryLocation, Result};
use crate::stdlib::load_core_stdlib;

/// A fully compiled Yelang crate ready for inspection or execution.
pub struct CompiledCrate {
    /// The type context, which owns the HIR crate and interner.
    pub tcx: TyCtxt,
    /// The logical query plan for the first query found in `main`, if any.
    pub plan: LogicalPlan,
}

impl std::fmt::Debug for CompiledCrate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompiledCrate")
            .field("plan", &self.plan)
            .finish_non_exhaustive()
    }
}

impl CompiledCrate {
    /// Return the `main` function's `DefId`.
    pub fn main_def(&self) -> Option<yelang_arena::DefId> {
        find_main(&self.tcx)
    }

    /// Plan this compiled crate's logical plan for the in-memory backend and
    /// execute it. If the crate was compiled without a query expression, the
    /// `main` body is evaluated directly.
    pub fn run(&self) -> Result<Value> {
        if self.plan.root.is_some() {
            let physical = plan_logical_fn(&self.plan, &MemoryBackend::new())?;
            return MemoryExecutor::new()
                .execute(&physical)
                .map_err(|e| DriverError::Execution(format!("{:?}", e)));
        }
        self.eval_main()
    }

    /// Evaluate the `main` function body directly when it contains no query.
    ///
    /// Looks for a tail expression or a `let _ = <expr>;` binding and lowers it
    /// to a single-node physical plan.
    fn eval_main(&self) -> Result<Value> {
        let main_def = self.main_def().ok_or(DriverError::MissingMain)?;
        let body_id = main_body(&self.tcx, main_def).ok_or(DriverError::MainHasNoBody)?;
        let results = self.tcx
            .typeck_results
            .get(main_def)
            .ok_or_else(|| DriverError::TypeCheck(vec![]))?;

        let mut plan = LogicalPlan::empty();
        let mut ctx = yelang_qir::lir::lower::LoweringCtxt::new(&self.tcx, body_id, results);
        ctx.populate_stdlib_tables()
            .map_err(|e| DriverError::QirLowering(e))?;
        yelang_qir::lir::lower::populate_local_values(&mut plan, &mut ctx, body_id)
            .map_err(|e| DriverError::QirLowering(e))?;

        let body = self.tcx.crate_hir().body(body_id).ok_or(DriverError::MainHasNoBody)?;
        let body_expr = self.tcx.crate_hir().expr(body.value).ok_or(DriverError::MissingQuery)?;
        let eval_expr_id = match body_expr {
            Expr::Block { block } => {
                // Prefer the block's tail expression.
                if let Some(tail) = block.expr {
                    Some(tail)
                } else {
                    // Otherwise use the last `let _ = <expr>;` binding.
                    block.stmts.iter().rev().find_map(|stmt_id| {
                        let stmt = self.tcx.crate_hir().stmt(*stmt_id)?;
                        match stmt {
                            yelang_hir::hir::core::Stmt::Let { pat, init, .. }
                                if is_discard_pattern(&self.tcx, *pat) =>
                            {
                                *init
                            }
                            _ => None,
                        }
                    })
                }
            }
            _ => Some(body.value),
        };

        let eval_expr_id = eval_expr_id.ok_or(DriverError::MissingQuery)?;
        let qexpr = yelang_qir::lir::lower::expr::lower_hir_expr(&mut plan, &mut ctx, eval_expr_id)
            .map_err(|e| DriverError::QirLowering(e))?;

        let root = match plan.expr(qexpr) {
            yelang_qir::expr::QExpr::Subplan(lir, _) => *lir,
            _ => {
                let ty = plan.expr(qexpr).ty();
                plan.expr_op(qexpr, ty)
            }
        };
        plan.set_root(root);

        let physical = plan_logical_fn(&plan, &MemoryBackend::new())?;
        MemoryExecutor::new()
            .execute(&physical)
            .map_err(|e| DriverError::Execution(format!("{:?}", e)))
    }
}

fn is_discard_pattern(tcx: &TyCtxt, pat_id: yelang_hir::ids::PatId) -> bool {
    tcx.crate_hir()
        .pat(pat_id)
        .map(|p| matches!(p, yelang_hir::hir::pat::Pat::Wild))
        .unwrap_or(false)
}

/// Entry point for compiling and running small snippets of Yelang source.
#[derive(Debug, Default, Clone)]
pub struct Driver {
    /// Optional override for the stdlib source. If `None`, the core prelude is
    /// loaded from `../stdlib/core/src` relative to this crate.
    stdlib_source: Option<String>,
}

impl Driver {
    /// Create a new driver using the on-disk core stdlib.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a driver with an explicit stdlib source. This is useful for tests
    /// that want a minimal or custom prelude.
    pub fn with_stdlib(stdlib: impl Into<String>) -> Self {
        Self {
            stdlib_source: Some(stdlib.into()),
        }
    }

    /// Compile a snippet of user source together with the prelude.
    ///
    /// The snippet must define a `main` function. The driver returns the
    /// compiled crate and the logical plan for the first query found inside
    /// `main`.
    pub fn compile(&self, user_src: &str) -> Result<CompiledCrate> {
        let full = self.assemble_source(user_src)?;
        let (mut program, interner) = parse_program(&full)?;
        yelang_ast::desugar_query_aggregates(&mut program, &interner);
        let resolved = resolve_crate(&program, &interner);
        if !resolved.errors.is_empty() {
            return Err(DriverError::Resolution(resolved.errors));
        }

        let hir_crate = lower_crate(&program, &resolved, &interner);
        let mut tcx = TyCtxt::with_string_interner(hir_crate, interner.clone());
        let diagnostics = type_check_crate(&mut tcx);
        if !diagnostics.is_empty() {
            return Err(DriverError::TypeCheck(diagnostics));
        }

        let main_def = find_main(&tcx).ok_or(DriverError::MissingMain)?;
        let body_id = main_body(&tcx, main_def).ok_or(DriverError::MainHasNoBody)?;
        let query_loc = find_query(&tcx, body_id).ok_or(DriverError::MissingQuery)?;
        let results = tcx
            .typeck_results
            .get(main_def)
            .ok_or_else(|| DriverError::TypeCheck(vec![]))?;

        let plan = lower_query(&tcx, query_loc.body_id, query_loc.query_id, results)?;

        Ok(CompiledCrate { tcx, plan })
    }

    /// Compile and immediately execute the first query in `main`, or, if `main`
    /// contains no query expression, evaluate the function body directly.
    ///
    /// This is the convenience API for tests and small REPL-like use cases.
    pub fn run(&self, user_src: &str) -> Result<Value> {
        let compiled = self.compile_or_eval_main(user_src)?;
        compiled.run()
    }

    /// Like `compile`, but does not require a query expression in `main`.
    /// The returned `CompiledCrate` may have an empty plan; `CompiledCrate::run`
    /// will fall back to evaluating the `main` body expression directly.
    pub fn compile_or_eval_main(&self, user_src: &str) -> Result<CompiledCrate> {
        let full = self.assemble_source(user_src)?;
        let (mut program, interner) = parse_program(&full)?;
        yelang_ast::desugar_query_aggregates(&mut program, &interner);
        let resolved = resolve_crate(&program, &interner);
        if !resolved.errors.is_empty() {
            return Err(DriverError::Resolution(resolved.errors));
        }

        let hir_crate = lower_crate(&program, &resolved, &interner);
        let mut tcx = TyCtxt::with_string_interner(hir_crate, interner.clone());
        let diagnostics = type_check_crate(&mut tcx);
        if !diagnostics.is_empty() {
            return Err(DriverError::TypeCheck(diagnostics));
        }

        let main_def = find_main(&tcx).ok_or(DriverError::MissingMain)?;
        let body_id = main_body(&tcx, main_def).ok_or(DriverError::MainHasNoBody)?;
        let results = tcx
            .typeck_results
            .get(main_def)
            .ok_or_else(|| DriverError::TypeCheck(vec![]))?;

        let mut plan = LogicalPlan::empty();
        if let Some(query_loc) = find_query(&tcx, body_id) {
            plan = lower_query(&tcx, query_loc.body_id, query_loc.query_id, results)?;
        }

        Ok(CompiledCrate { tcx, plan })
    }

    /// Assemble the final source string from the prelude and user code.
    fn assemble_source(&self, user_src: &str) -> Result<String> {
        let stdlib = match &self.stdlib_source {
            Some(s) => s.clone(),
            None => load_core_stdlib()?,
        };
        Ok(format!("{}\n{}", stdlib, user_src))
    }
}

/// Parse Yelang source into an AST program and interner.
fn parse_program(src: &str) -> Result<(Program, Interner)> {
    let interner = Interner::new();
    let tokens = TokenKind::tokenize(src, &interner)
        .map_err(|e| DriverError::Parse(format!("{:?}", e)))?;
    let mut stream = tokens;
    let program = stream
        .parse::<Program>()
        .map_err(|e| DriverError::Parse(format!("{:?}", e)))?;
    Ok((program, interner))
}

/// Find the `main` function in the HIR crate.
fn find_main(tcx: &TyCtxt) -> Option<yelang_arena::DefId> {
    tcx.crate_hir()
        .items
        .iter_enumerated()
        .find_map(|(def_id, item)| {
            let item = item.as_ref()?;
            match &item.kind {
                ItemKind::Fn { .. } => {
                    if tcx.resolve_symbol(item.ident.symbol) == Some("main") {
                        Some(def_id)
                    } else {
                        None
                    }
                }
                _ => None,
            }
        })
}

/// Return the body id of a function item.
fn main_body(tcx: &TyCtxt, def_id: yelang_arena::DefId) -> Option<BodyId> {
    tcx.crate_hir()
        .items
        .get(def_id)
        .and_then(|i| i.as_ref())
        .and_then(|i| match &i.kind {
            ItemKind::Fn { body, .. } => Some(*body),
            _ => None,
        })
}

/// Find the first query expression inside a function body.
fn find_query(tcx: &TyCtxt, body_id: BodyId) -> Option<QueryLocation> {
    let body = tcx.crate_hir().body(body_id)?;
    find_query_expr(tcx, body.value).map(|query_id| QueryLocation { body_id, query_id })
}

fn find_query_expr(tcx: &TyCtxt, expr_id: ExprId) -> Option<QueryId> {
    let expr = tcx.crate_hir().expr(expr_id)?;
    match expr {
        Expr::Query(query_id) => Some(*query_id),
        Expr::Block { block } => {
            for stmt_id in &block.stmts {
                let stmt = tcx.crate_hir().stmt(*stmt_id)?;
                let stmt_expr = match stmt {
                    yelang_hir::hir::core::Stmt::Expr { expr } => Some(*expr),
                    yelang_hir::hir::core::Stmt::Let { init, .. } => *init,
                    _ => None,
                };
                if let Some(e) = stmt_expr {
                    if let Some(q) = find_query_expr(tcx, e) {
                        return Some(q);
                    }
                }
            }
            block.expr.and_then(|e| find_query_expr(tcx, e))
        }
        Expr::Let { expr, .. } => find_query_expr(tcx, *expr),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_compiles_simple_select() {
        let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select x from xs@x;
}
"#;
        let compiled = Driver::new().compile(src).expect("compile");
        assert!(compiled.plan.root.is_some());
    }

    #[test]
    fn driver_runs_filter_and_map() {
        let src = r#"
fn main() {
    let xs = [1, 2, 3, 4, 5];
    let _ = select x + 10 from xs@x where x > 2;
}
"#;
        let value = Driver::new().run(src).expect("run");
        let ints: Vec<i128> = value
            .try_into_array()
            .unwrap()
            .into_iter()
            .map(|v| match v {
                Value::Int(n) => n,
                _ => panic!("expected int"),
            })
            .collect();
        assert_eq!(ints, vec![13, 14, 15]);
    }
}

//! Yelang compiler driver — orchestrates the full pipeline.
//!
//! ```text
//! Source (.ye)
//!   → Lexer → Parser (AST)
//!     → Resolve → HIR Lowering
//!       → THIR + TyCheck
//!         → Plan Extraction (yelang-qir)
//!           → Decorrelation + Optimization
//!             → Physical Planning
//!               → Executor (storage backends)
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use yelang_driver::{compile_source, InMemoryExecutor};
//!
//! let result = compile_source(
//!     r#"
//!     fn main() {
//!         let result = select users@u[where u.age > 18][*].name
//!                      from users@u:User;
//!     }
//!     "#,
//!     &InMemoryExecutor,
//! ).expect("compilation failed");
//!
//! for (query_id, phys_id) in &result.plans {
//!     println!("Query {:?} → physical plan {:?}", query_id, phys_id);
//! }
//! ```

use yelang_ast::Program;
use yelang_hir::ids::QueryId;
use yelang_interner::Interner;
use yelang_lexer::FileId;
use yelang_qir::physical::{Executor, InMemoryExecutor, PhysArena, PhysId};
use yelang_qir::plan::PlanArena;
use yelang_qir::{extract_query, Optimizer};
use yelang_tycheck::diagnostics::Diagnostic;
use yelang_tycheck::tcx::TyCtxt;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur during compilation.
#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    #[error("lexing failed: {0}")]
    Lex(String),

    #[error("parsing failed: {0}")]
    Parse(String),

    #[error("name resolution produced {} error(s)", .0.len())]
    Resolve(Vec<yelang_resolve::error::ResolutionError>),

    #[error("type checking produced {} error(s)", .0.len())]
    TypeCheck(Vec<Diagnostic>),
}

// ---------------------------------------------------------------------------
// Compilation result
// ---------------------------------------------------------------------------

/// The result of a successful compilation.
pub struct CompilationResult {
    /// The string interner (needed to resolve symbols in plans).
    pub interner: Interner,

    /// The HIR crate (kept alive for expression references in plans).
    pub hir: yelang_hir::Crate,

    /// The logical plan arena (all extracted + optimized plans).
    pub plan_arena: PlanArena,

    /// The physical plan arena (all lowered physical plans).
    pub phys_arena: PhysArena,

    /// Mapping from HIR query id to (optimized logical plan root, physical plan root).
    pub plans: Vec<(QueryId, yelang_qir::PlanId, PhysId)>,

    /// Type-checking diagnostics (warnings, etc.).
    pub diagnostics: Vec<Diagnostic>,
}

// ---------------------------------------------------------------------------
// Full pipeline
// ---------------------------------------------------------------------------

/// Compile source text through the full pipeline.
///
/// Returns a [`CompilationResult`] containing the logical and physical
/// plans for every query expression found in the source.
pub fn compile_source(
    src: &str,
    executor: &dyn Executor,
) -> Result<CompilationResult, CompileError> {
    // 1. Lex + Parse.
    let interner = Interner::new();
    let file_id = FileId::new(1);
    let program = yelang_ast::parse_program_strict_with_file_id(src, &mut interner.clone(), file_id)
        .map_err(CompileError::Parse)?;

    compile_program(&program, &interner, executor)
}

/// Compile a parsed program through the full pipeline.
pub fn compile_program(
    program: &Program,
    interner: &Interner,
    executor: &dyn Executor,
) -> Result<CompilationResult, CompileError> {
    // 2. Resolve.
    let resolved = yelang_resolve::resolve_crate(program, interner);
    if !resolved.errors.is_empty() {
        return Err(CompileError::Resolve(resolved.errors));
    }

    // 3. Lower to HIR.
    let hir_crate = yelang_hir::lower_crate(program, &resolved, interner);

    // 4. Type check.
    let mut tcx = TyCtxt::with_string_interner(hir_crate, interner.clone());
    let diagnostics = yelang_tycheck::type_check_crate(&mut tcx);

    // Check for type errors.
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == yelang_tycheck::diagnostics::Severity::Error)
        .cloned()
        .collect();
    if !errors.is_empty() {
        return Err(CompileError::TypeCheck(errors));
    }

    // 5. Get the HIR back from TyCtxt.
    let hir = tcx.crate_hir().clone();

    // 6. Extract, optimize, and physically plan every query.
    let mut plan_arena = PlanArena::new();
    let mut phys_arena = PhysArena::new();
    let optimizer = Optimizer::new();
    let mut plans = Vec::new();

    // Iterate over all queries in the HIR.
    for (query_id, query_slot) in hir.queries.iter() {
        if query_slot.is_none() {
            continue;
        }

        // 6a. Extract logical plan from HIR query.
        let Some(logical_root) = extract_query(query_id, &hir, interner, &hir.lang_items, &mut plan_arena) else {
            continue; // Mutations not yet handled.
        };

        // 6b. Optimize (decorrelation + fixpoint rules).
        let optimized_root = optimizer.optimize(logical_root, &mut plan_arena, &hir);

        // 6c. Lower to physical plan.
        let phys_root =
            yelang_qir::physical::planner::plan_physical(
                optimized_root,
                &plan_arena,
                executor,
                &mut phys_arena,
            );

        plans.push((query_id, optimized_root, phys_root));
    }

    Ok(CompilationResult {
        interner: interner.clone(),
        hir,
        plan_arena,
        phys_arena,
        plans,
        diagnostics,
    })
}

// ---------------------------------------------------------------------------
// Convenience: compile with in-memory executor
// ---------------------------------------------------------------------------

/// Compile source text using the in-memory executor (no distribution).
pub fn compile_in_memory(src: &str) -> Result<CompilationResult, CompileError> {
    compile_source(src, &InMemoryExecutor)
}

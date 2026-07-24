//! QIR → bytecode compiler.
//!
//! Compiles a QIR physical plan into bytecode instructions that the VM
//! can execute. Each physical operator maps to query bytecode operations
//! (QueryScan, QueryFilter, QueryJoin, QueryAggregate, etc.).

use yelang_interner::Symbol;
use yelang_qir::physical::{JoinAlgorithm as PhysJoinAlgorithm, PhysArena, PhysId, PhysOp};
use yelang_qir::plan::{Direction, JoinKind as PhysJoinKind, PlanArena, TraversePath};

use crate::instruction::{CompiledFunction, Instruction};
use crate::join::{JoinAlgorithm, JoinKind, JoinSpec};
use crate::traverse::{TraverseDirection, TraverseSpec};

/// Compile a QIR physical plan into a bytecode function.
///
/// The resulting function, when executed by the VM, runs the query
/// and leaves the result on the stack.
///
/// `plan_arena` supplies the THIR expressions needed to resolve a join's key
/// columns (the `on` expressions are field accesses back into the typed HIR).
pub fn compile_query(
    plan: &PhysArena,
    root: PhysId,
    plan_arena: &PlanArena,
) -> CompiledFunction {
    let mut compiler = QueryCompiler::new(plan_arena);
    compiler.compile_node(plan, root);
    compiler.emit(Instruction::Halt);
    compiler.finish()
}

struct QueryCompiler<'a> {
    instructions: Vec<Instruction>,
    plan_arena: &'a PlanArena,
}

impl<'a> QueryCompiler<'a> {
    fn new(plan_arena: &'a PlanArena) -> Self {
        Self {
            instructions: Vec::new(),
            plan_arena,
        }
    }

    fn compile_node(&mut self, plan: &PhysArena, node_id: PhysId) {
        let Some(op) = plan.get(node_id) else {
            return;
        };

        match op {
            PhysOp::Scan { source, filter, .. } => {
                // Emit a query scan.
                let table_id = match source {
                    yelang_qir::plan::SourceRef::Table { def, .. } => def.raw() as u64,
                    _ => 0,
                };
                self.emit(Instruction::QueryScan(table_id));

                // If there's a pushed-down filter, emit it.
                if filter.is_some() {
                    // TODO: compile the filter expression.
                    // For now, emit a no-op filter.
                    self.emit(Instruction::PushConst(crate::value::Value::Bool(true)));
                    self.emit(Instruction::QueryFilter);
                }
            }

            PhysOp::Filter { input, pred: _ } => {
                // Compile the input first.
                self.compile_node(plan, *input);
                // TODO: compile the filter predicate expression.
                // For now, emit a pass-through filter.
                self.emit(Instruction::PushConst(crate::value::Value::Bool(true)));
                self.emit(Instruction::QueryFilter);
            }

            PhysOp::Project { input, exprs } => {
                self.compile_node(plan, *input);
                let fields: Vec<Symbol> = exprs.iter().map(|(name, _)| *name).collect();
                self.emit(Instruction::QueryProject(fields));
            }

            PhysOp::Map { input, func: _, flatten_depth: _ } => {
                self.compile_node(plan, *input);
                // TODO: compile the map function.
                // For now, pass through.
            }

            PhysOp::Join {
                left,
                right,
                kind,
                algorithm,
                on,
                filter: _,
            } => {
                // Compile both sides; the right result ends up on top of the
                // stack, matching the VM's QueryJoin pop order.
                self.compile_node(plan, *left);
                self.compile_node(plan, *right);
                // Compile the join predicate: resolve each equi-join `on`
                // pair to its key columns and pick the physical algorithm.
                let spec = build_join_spec(*kind, *algorithm, on, self.plan_arena);
                self.emit(Instruction::QueryJoin(spec));
            }

            PhysOp::Aggregate {
                input,
                keys,
                aggs: _,
                into: _,
                algorithm: _,
            } => {
                self.compile_node(plan, *input);
                let key_names: Vec<Symbol> = keys.iter().map(|(name, _)| *name).collect();
                self.emit(Instruction::QueryAggregate(key_names));
            }

            PhysOp::Sort {
                input,
                specs,
                algorithm: _,
            } => {
                self.compile_node(plan, *input);
                // TODO: compile sort specs.
                let sort_keys: Vec<(Symbol, bool)> = Vec::new();
                let _ = specs;
                self.emit(Instruction::QuerySort(sort_keys));
            }

            PhysOp::Limit {
                input,
                skip,
                fetch,
            } => {
                self.compile_node(plan, *input);
                // Push skip and fetch values.
                let skip_val = skip
                    .map(|_| crate::value::Value::Uint(0))
                    .unwrap_or(crate::value::Value::Uint(0));
                let fetch_val = fetch
                    .map(|_| crate::value::Value::Uint(0))
                    .unwrap_or(crate::value::Value::Uint(u128::MAX));
                self.emit(Instruction::PushConst(skip_val));
                self.emit(Instruction::PushConst(fetch_val));
                self.emit(Instruction::QueryLimit);
            }

            PhysOp::TopN {
                input,
                specs: _,
                skip,
                fetch: _,
            } => {
                self.compile_node(plan, *input);
                // TopN = Sort + Limit fused.
                let sort_keys: Vec<(Symbol, bool)> = Vec::new();
                self.emit(Instruction::QuerySort(sort_keys));
                let skip_val = skip
                    .map(|_| crate::value::Value::Uint(0))
                    .unwrap_or(crate::value::Value::Uint(0));
                self.emit(Instruction::PushConst(skip_val));
                // TODO: resolve fetch ExprRef to an actual limit value.
                self.emit(Instruction::PushConst(crate::value::Value::Uint(0)));
                self.emit(Instruction::QueryLimit);
            }

            PhysOp::Distinct { input, on: _ } => {
                self.compile_node(plan, *input);
                // TODO: implement distinct.
            }

            PhysOp::Window { input, funcs: _ } => {
                self.compile_node(plan, *input);
                // TODO: implement window functions.
            }

            PhysOp::Traverse { input, paths, strategy: _ } => {
                self.compile_node(plan, *input);
                // Build a traversal spec from the first path's first segment.
                // With no traversable path, the input passes through unchanged.
                if let Some(spec) = build_traverse_spec(paths) {
                    self.emit(Instruction::QueryTraverse(spec));
                }
            }

            PhysOp::Exchange { input, kind: _ } => {
                // Exchange is a no-op in single-node execution.
                // In distributed execution, this becomes a network shuffle.
                self.compile_node(plan, *input);
            }

            PhysOp::Union { inputs } => {
                // Compile all inputs and concatenate results.
                for input in inputs {
                    self.compile_node(plan, *input);
                }
                // TODO: emit union/concatenation.
            }

            PhysOp::Repeat { input, func: _, max_iters: _ } => {
                self.compile_node(plan, *input);
                // TODO: implement repeat/fixpoint iteration.
            }

            PhysOp::Extension { node: _ } => {
                // User-defined operator: cannot compile to bytecode.
                // Must be executed via the Extension's own execution logic.
            }

            PhysOp::Constant { value: _ } => {
                // TODO: push the constant value.
                self.emit(Instruction::PushConst(crate::value::Value::Null));
            }

            PhysOp::Empty { produce_one_row } => {
                if *produce_one_row {
                    self.emit(Instruction::PushConst(crate::value::Value::Unit));
                } else {
                    self.emit(Instruction::PushConst(crate::value::Value::QueryResult(
                        vec![],
                    )));
                }
            }
        }
    }

    fn emit(&mut self, instr: Instruction) {
        self.instructions.push(instr);
    }

    fn finish(self) -> CompiledFunction {
        CompiledFunction {
            name: None,
            instructions: self.instructions,
            num_locals: 0,
            num_args: 0,
        }
    }
}

/// Build a VM [`TraverseSpec`] from the first segment of the first traversal
/// path, if any.
///
/// The edge table conventionally carries `_from`/`_to` key columns and nodes
/// are keyed by `id`; the nested result is stored under the target's collection
/// label. Returns `None` when there is no path/segment to traverse.
fn build_traverse_spec(paths: &[TraversePath]) -> Option<TraverseSpec> {
    let segment = paths.first()?.segments.first()?;

    // Conventional column/key names for edge and node tables.
    let interner = yelang_interner::Interner::new();
    let from = interner.intern("_from");
    let to = interner.intern("_to");
    let id = interner.intern("id");

    let direction = match segment.direction {
        Direction::Forward => TraverseDirection::Out,
        Direction::Backward => TraverseDirection::In,
        Direction::Both => TraverseDirection::Both,
    };

    Some(TraverseSpec {
        edge_table: segment.edge.def.raw() as u64,
        source_column: from,
        target_column: to,
        target_table: segment.target.def.raw() as u64,
        direction,
        source_key: id,
        target_key: id,
        output: segment.target.label,
    })
}

/// Build a VM [`JoinSpec`] from a physical join operator.
///
/// Each `on` pair `(left_expr, right_expr)` is an equi-join predicate; when
/// both sides resolve to field accesses their column names become the join
/// keys. If any pair fails to resolve (a non-equi predicate such as `<`), the
/// whole join falls back to a keyless nested loop so the residual predicate is
/// never silently dropped into a wrong hash key.
fn build_join_spec(
    kind: PhysJoinKind,
    algorithm: PhysJoinAlgorithm,
    on: &[(yelang_qir::plan::JoinKey, yelang_qir::plan::JoinKey)],
    plan_arena: &PlanArena,
) -> JoinSpec {
    let mut left_keys = Vec::with_capacity(on.len());
    let mut right_keys = Vec::with_capacity(on.len());
    let mut all_equi = true;

    for (left_key, right_key) in on {
        let lk = match left_key {
            yelang_qir::plan::JoinKey::Column(sym) => Some(*sym),
            yelang_qir::plan::JoinKey::Expr(expr) => plan_arena.field_name(*expr),
        };
        let rk = match right_key {
            yelang_qir::plan::JoinKey::Column(sym) => Some(*sym),
            yelang_qir::plan::JoinKey::Expr(expr) => plan_arena.field_name(*expr),
        };
        match (lk, rk) {
            (Some(lk), Some(rk)) => {
                left_keys.push(lk);
                right_keys.push(rk);
            }
            _ => {
                all_equi = false;
            }
        }
    }

    // A single non-equi predicate demotes the whole join to a keyless scan.
    if !all_equi {
        left_keys.clear();
        right_keys.clear();
    }

    let vm_kind = match kind {
        PhysJoinKind::Inner => JoinKind::Inner,
        PhysJoinKind::Left => JoinKind::Left,
        PhysJoinKind::Right => JoinKind::Right,
        PhysJoinKind::Full => JoinKind::Full,
        PhysJoinKind::Semi => JoinKind::Semi,
        PhysJoinKind::Anti => JoinKind::Anti,
        PhysJoinKind::Cross => JoinKind::Cross,
    };

    let vm_algorithm = match algorithm {
        PhysJoinAlgorithm::HashBuildProbe
        | PhysJoinAlgorithm::CoLocatedHash
        | PhysJoinAlgorithm::ShuffleHash
        | PhysJoinAlgorithm::BroadcastHash => JoinAlgorithm::Hash,
        PhysJoinAlgorithm::SortMerge | PhysJoinAlgorithm::NestedLoop => JoinAlgorithm::NestedLoop,
    };

    // A hash join needs at least one equi key; otherwise fall back to a
    // nested loop (cross joins always take the nested-loop path).
    let vm_algorithm = if left_keys.is_empty() {
        JoinAlgorithm::NestedLoop
    } else {
        vm_algorithm
    };

    JoinSpec {
        kind: vm_kind,
        algorithm: vm_algorithm,
        left_keys,
        right_keys,
    }
}

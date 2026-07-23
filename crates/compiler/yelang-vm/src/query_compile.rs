//! QIR → bytecode compiler.
//!
//! Compiles a QIR physical plan into bytecode instructions that the VM
//! can execute. Each physical operator maps to query bytecode operations
//! (QueryScan, QueryFilter, QueryJoin, QueryAggregate, etc.).

use yelang_interner::Symbol;
use yelang_qir::physical::{PhysArena, PhysId, PhysOp};
use yelang_qir::plan::{Direction, TraversePath};

use crate::instruction::{CompiledFunction, Instruction};
use crate::traverse::{TraverseDirection, TraverseSpec};

/// Compile a QIR physical plan into a bytecode function.
///
/// The resulting function, when executed by the VM, runs the query
/// and leaves the result on the stack.
pub fn compile_query(plan: &PhysArena, root: PhysId) -> CompiledFunction {
    let mut compiler = QueryCompiler::new();
    compiler.compile_node(plan, root);
    compiler.emit(Instruction::Halt);
    compiler.finish()
}

struct QueryCompiler {
    instructions: Vec<Instruction>,
}

impl QueryCompiler {
    fn new() -> Self {
        Self {
            instructions: Vec::new(),
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
                kind: _,
                algorithm: _,
                on: _,
                filter: _,
            } => {
                // Compile both sides.
                self.compile_node(plan, *left);
                self.compile_node(plan, *right);
                // TODO: compile the join predicate.
                self.emit(Instruction::PushConst(crate::value::Value::Bool(true)));
                self.emit(Instruction::QueryJoin);
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

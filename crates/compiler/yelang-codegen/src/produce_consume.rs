//! Neumann's produce/consume model for query pipeline code generation.
//!
//! From "Efficiently Compiling Efficient Query Plans for Modern Hardware"
//! (Neumann, VLDB 2011):
//!
//! Each physical operator has two compile-time methods:
//! - `produce()`: "generate your output tuples" — emits the outer loop
//! - `consume(attrs, source)`: "I just received a tuple" — emits inner logic
//!
//! These are NOT runtime functions. They are code-generation directives
//! that emit IR. The generated code has no operator objects — just tight
//! loops with fused predicate evaluation, hash computation, and output.
//!
//! ```text
//! Scan.produce:
//!   for each tuple in relation:
//!     parent.consume(attributes)
//!
//! Filter.consume(attrs, source):
//!   if predicate(attrs):
//!     parent.consume(attrs)
//!
//! HashJoin.produce:
//!   left.produce()    // build phase
//!   right.produce()   // probe phase
//! HashJoin.consume(attrs, source):
//!   if source == left:
//!     insert into hash table
//!   else:
//!     for each match in hash table:
//!       parent.consume(attrs + matched_attrs)
//! ```

use crate::emit::IrEmitter;
use yelang_qir::physical::{PhysArena, PhysId, PhysOp};
use crate::pipeline::{identify_pipelines, Pipeline};

/// The produce/consume code generator.
///
/// Walks the physical plan tree and emits fused pipeline code
/// through the `IrEmitter` interface.
pub struct ProduceConsume<'a, E: IrEmitter> {
    /// The physical plan arena.
    plan: &'a PhysArena,
    /// The IR emitter.
    emitter: &'a mut E,
}

impl<'a, E: IrEmitter> ProduceConsume<'a, E> {
    pub fn new(plan: &'a PhysArena, emitter: &'a mut E) -> Self {
        Self { plan, emitter }
    }

    /// Generate code for the entire physical plan.
    ///
    /// 1. Identify pipelines (split at pipeline breakers)
    /// 2. For each pipeline, emit fused code via produce/consume
    pub fn generate(&mut self, root: PhysId) {
        let pipelines = identify_pipelines(self.plan, root);

        for pipeline in &pipelines {
            self.generate_pipeline(pipeline);
        }
    }

    /// Generate code for a single pipeline.
    fn generate_pipeline(&mut self, pipeline: &Pipeline) {
        if pipeline.operators.is_empty() {
            return;
        }

        // The first operator in the pipeline drives the loop (produce).
        // Subsequent operators consume tuples from the previous operator.
        let first_op_ref = pipeline.operators[0].op;
        if let Some(op) = self.plan.get(PhysId::new(first_op_ref.0 as u32)) {
            self.produce(op);
        }
    }

    /// Emit the `produce` code for an operator.
    ///
    /// The produce method generates the outer loop that drives tuple
    /// flow through the pipeline.
    fn produce(&mut self, op: &PhysOp) {
        match op {
            PhysOp::Scan { source, filter, .. } => {
                // Emit: for each tuple in relation { ... }
                // The loop body calls the parent's consume method.
                //
                // TODO: emit actual scan loop via IrEmitter.
                // This requires knowing the relation's schema and
                // the storage engine's scan interface.
                let _ = (source, filter);
            }

            PhysOp::Sort { input, .. } => {
                // Sort is a full pipeline breaker.
                // Produce: sort all input tuples, then emit them in order.
                //
                // TODO: emit sort logic (external merge sort, top-N heap).
                if let Some(input_op) = self.plan.get(*input) {
                    self.produce(input_op);
                }
            }

            PhysOp::Aggregate { input, .. } => {
                // Aggregate is a full pipeline breaker.
                // Produce: aggregate all input tuples, then emit groups.
                //
                // TODO: emit hash aggregation logic.
                if let Some(input_op) = self.plan.get(*input) {
                    self.produce(input_op);
                }
            }

            PhysOp::Join { left, right, .. } => {
                // Join is a partial breaker.
                // Produce: build hash table from left, probe with right.
                //
                // TODO: emit hash join build + probe logic.
                if let Some(left_op) = self.plan.get(*left) {
                    self.produce(left_op);
                }
                if let Some(right_op) = self.plan.get(*right) {
                    self.produce(right_op);
                }
            }

            // Non-breaker operators: produce delegates to input.
            PhysOp::Filter { input, .. }
            | PhysOp::Project { input, .. }
            | PhysOp::Map { input, .. }
            | PhysOp::Limit { input, .. }
            | PhysOp::Distinct { input, .. }
            | PhysOp::Window { input, .. }
            | PhysOp::Exchange { input, .. }
            | PhysOp::Traverse { input, .. }
            | PhysOp::TopN { input, .. } => {
                if let Some(input_op) = self.plan.get(*input) {
                    self.produce(input_op);
                }
            }

            PhysOp::Union { inputs } => {
                for input in inputs {
                    if let Some(input_op) = self.plan.get(*input) {
                        self.produce(input_op);
                    }
                }
            }

            _ => {}
        }
    }

    /// Emit the `consume` code for an operator.
    ///
    /// The consume method processes a tuple received from the child
    /// operator and forwards it to the parent.
    #[allow(dead_code)]
    fn consume(&mut self, op: &PhysOp) {
        match op {
            PhysOp::Filter { pred, .. } => {
                // Emit: if predicate(tuple) { parent.consume(tuple) }
                let _ = pred;
                // TODO: emit predicate evaluation + conditional branch.
            }

            PhysOp::Project { exprs, .. } => {
                // Emit: compute projection expressions, forward result.
                let _ = exprs;
                // TODO: emit expression evaluation.
            }

            PhysOp::Map { func, .. } => {
                // Emit: apply map function, forward result.
                let _ = func;
                // TODO: emit function application.
            }

            _ => {
                // Other operators: forward tuple unchanged.
            }
        }
    }
}

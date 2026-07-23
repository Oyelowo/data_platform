//! Pipeline identification and management.
//!
//! A pipeline is a sequence of operators that can be fused into a single
//! tight loop. Pipeline breakers (hash join build, aggregation, sort)
//! materialize intermediate state and split the pipeline.
//!
//! From Neumann (VLDB 2011):
//! > A pipeline breaker is an operator that takes an incoming tuple out
//! > of CPU registers. A full pipeline breaker materializes all incoming
//! > tuples from that input before continuing.

use yelang_qir::physical::PhysOp;

/// A pipeline: a sequence of fused operators.
#[derive(Debug)]
pub struct Pipeline {
    /// The operators in this pipeline, in execution order.
    pub operators: Vec<PipelineOp>,
    /// Whether this pipeline has been compiled.
    pub compiled: bool,
}

/// An operator within a pipeline.
#[derive(Debug)]
pub struct PipelineOp {
    /// The physical operator.
    pub op: PhysOpRef,
    /// Whether this operator is a pipeline breaker.
    pub is_breaker: bool,
}

/// A reference to a physical operator (by index in the physical plan).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysOpRef(pub u64);

/// Pipeline breaker classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineBreaker {
    /// Full breaker: materializes ALL tuples before continuing.
    /// Examples: Sort, HashAggregate (build phase).
    Full,
    /// Partial breaker: materializes some tuples.
    /// Examples: HashJoin (build side), Sort with LIMIT.
    Partial,
    /// Not a breaker: tuples flow through in registers.
    /// Examples: Filter, Map, Project.
    None,
}

impl PipelineBreaker {
    /// Classify a physical operator as a pipeline breaker.
    pub fn classify(op: &PhysOp) -> Self {
        match op {
            // Full breakers: must see all input before producing output.
            PhysOp::Sort { .. } => PipelineBreaker::Full,
            PhysOp::Aggregate { .. } => PipelineBreaker::Full,
            PhysOp::TopN { .. } => PipelineBreaker::Partial,

            // Partial breakers: build side materializes, probe side flows.
            PhysOp::Join { .. } => PipelineBreaker::Partial,

            // Not breakers: tuples flow through.
            PhysOp::Scan { .. }
            | PhysOp::Filter { .. }
            | PhysOp::Project { .. }
            | PhysOp::Map { .. }
            | PhysOp::Limit { .. }
            | PhysOp::Distinct { .. }
            | PhysOp::Window { .. }
            | PhysOp::Exchange { .. }
            | PhysOp::Traverse { .. }
            | PhysOp::Repeat { .. }
            | PhysOp::Union { .. }
            | PhysOp::Extension { .. }
            | PhysOp::Constant { .. }
            | PhysOp::Empty { .. } => PipelineBreaker::None,
        }
    }
}

/// Identify pipelines in a physical plan.
///
/// Walks the plan tree and splits at pipeline breakers.
/// Returns a list of pipelines in execution order.
pub fn identify_pipelines(plan: &yelang_qir::physical::PhysArena, root: yelang_qir::physical::PhysId) -> Vec<Pipeline> {
    let mut pipelines = Vec::new();
    let mut current_ops = Vec::new();

    // Walk the plan tree depth-first.
    let mut stack = vec![root];
    while let Some(op_id) = stack.pop() {
        let Some(op) = plan.get(op_id) else {
            continue;
        };

        let breaker = PipelineBreaker::classify(op);
        current_ops.push(PipelineOp {
            op: PhysOpRef(op_id.raw() as u64),
            is_breaker: breaker != PipelineBreaker::None,
        });

        if breaker != PipelineBreaker::None {
            // End the current pipeline and start a new one.
            pipelines.push(Pipeline {
                operators: std::mem::take(&mut current_ops),
                compiled: false,
            });
        }

        // Push children onto the stack.
        match op {
            PhysOp::Filter { input, .. }
            | PhysOp::Project { input, .. }
            | PhysOp::Map { input, .. }
            | PhysOp::Sort { input, .. }
            | PhysOp::Limit { input, .. }
            | PhysOp::Distinct { input, .. }
            | PhysOp::Window { input, .. }
            | PhysOp::Exchange { input, .. }
            | PhysOp::Traverse { input, .. }
            | PhysOp::Repeat { input, .. }
            | PhysOp::Aggregate { input, .. }
            | PhysOp::TopN { input, .. } => {
                stack.push(*input);
            }
            PhysOp::Join { left, right, .. } => {
                stack.push(*left);
                stack.push(*right);
            }
            PhysOp::Union { inputs } => {
                for input in inputs.iter().rev() {
                    stack.push(*input);
                }
            }
            _ => {}
        }
    }

    // Don't forget the last pipeline.
    if !current_ops.is_empty() {
        pipelines.push(Pipeline {
            operators: current_ops,
            compiled: false,
        });
    }

    pipelines
}

//! CFG simplification and constant folding.

use crate::body::{Body, Statement};
use crate::terminator::TerminatorKind;

use super::MirPass;

/// Simplify the control flow graph.
///
/// - Merge blocks that have a single predecessor and single successor
/// - Remove empty blocks that just goto the next block
pub struct SimplifyCfg;

impl MirPass for SimplifyCfg {
    fn name(&self) -> &str {
        "simplify_cfg"
    }

    fn run(&self, body: &mut Body) {
        // Count predecessors for each block.
        let block_count = body.basic_blocks.len();
        let mut pred_count = vec![0u32; block_count];

        for block in body.basic_blocks.iter() {
            for succ in block.terminator.kind.successors() {
                let idx = succ.raw() as usize;
                if idx < pred_count.len() {
                    pred_count[idx] += 1;
                }
            }
        }

        // Merge blocks: if block A's terminator is `goto B` and B has
        // exactly one predecessor (A), merge B's statements into A.
        let mut merged = true;
        while merged {
            merged = false;
            let block_ids: Vec<_> = body
                .basic_blocks
                .iter_enumerated()
                .map(|(id, _)| id)
                .collect();

            for block_id in block_ids {
                let target = match body.basic_blocks[block_id].terminator.kind {
                    TerminatorKind::Goto { target } => target,
                    _ => continue,
                };

                // Check if target has exactly one predecessor.
                let target_idx = target.raw() as usize;
                if target_idx >= pred_count.len() || pred_count[target_idx] != 1 {
                    continue;
                }

                // Don't merge a block with itself.
                if block_id == target {
                    continue;
                }

                // Merge target's statements into this block.
                let target_stmts = body.basic_blocks[target].statements.clone();
                let target_term = body.basic_blocks[target].terminator.clone();

                body.basic_blocks[block_id].statements.extend(target_stmts);
                body.basic_blocks[block_id].terminator = target_term;

                // Mark target as empty (can't remove from IndexVec).
                body.basic_blocks[target].statements.clear();
                body.basic_blocks[target].terminator.kind = TerminatorKind::Unreachable;

                // Update predecessor counts.
                for succ in body.basic_blocks[block_id].terminator.kind.successors() {
                    let succ_idx = succ.raw() as usize;
                    if succ_idx < pred_count.len() {
                        pred_count[succ_idx] += 1;
                    }
                }
                // The old target's predecessors are no longer relevant.
                if target_idx < pred_count.len() {
                    pred_count[target_idx] = 0;
                }

                merged = true;
            }
        }
    }
}

/// Constant folding pass.
///
/// Folds constant binary/unary operations at compile time.
/// This is a placeholder — full constant folding requires type information
/// and constant evaluation infrastructure.
pub struct ConstFold;

impl MirPass for ConstFold {
    fn name(&self) -> &str {
        "const_fold"
    }

    fn run(&self, body: &mut Body) {
        // Phase 1: Find constant locals (assigned a constant, never reassigned).
        // Phase 2: Replace uses of constant locals with the constant value.
        // Phase 3: Fold binary/unary operations on constants.
        //
        // TODO: Implement when constant evaluation infrastructure is available.
        // For now, this is a no-op placeholder.
        let _ = body;
    }
}

//! Dead code elimination.
//!
//! Removes:
//! - Unreachable basic blocks (not reachable from entry)
//! - Unused local variables (never read)
//! - Dead assignments (assigned but never read)

use yelang_arena::FxHashSet;

use crate::body::{BasicBlock, Body, Local, Statement};
use crate::ops::Operand;
use crate::place::Place;
use crate::terminator::TerminatorKind;

use super::MirPass;

/// Dead code elimination pass.
pub struct DeadCodeElimination;

impl MirPass for DeadCodeElimination {
    fn name(&self) -> &str {
        "dce"
    }

    fn run(&self, body: &mut Body) {
        // Phase 1: find reachable blocks.
        let reachable = find_reachable_blocks(body);

        // Phase 2: remove unreachable blocks.
        // We can't remove blocks from IndexVec (indices are stable),
        // so we replace unreachable blocks with empty blocks that
        // have an Unreachable terminator.
        let all_blocks: Vec<BasicBlock> = body
            .basic_blocks
            .iter_enumerated()
            .map(|(id, _)| id)
            .collect();
        for block_id in all_blocks {
            if !reachable.contains(&block_id) {
                body.basic_blocks[block_id].statements.clear();
                body.basic_blocks[block_id].terminator.kind = TerminatorKind::Unreachable;
            }
        }

        // Phase 3: find used locals.
        let used_locals = find_used_locals(body);

        // Phase 4: remove dead assignments to unused locals.
        let block_count = body.basic_blocks.len();
        for i in 0..block_count {
            let block_id = crate::body::BasicBlock::new(i as u32);
            if let Some(block) = body.basic_blocks.get_mut(block_id) {
                block.statements.retain(|stmt| match stmt {
                    Statement::Assign(place, _) => {
                        // Keep the assignment if the local is used.
                        used_locals.contains(&place.local)
                    }
                    Statement::Nop => false, // Remove nops.
                });
            }
        }
    }
}

/// Find all basic blocks reachable from the entry block.
fn find_reachable_blocks(body: &Body) -> FxHashSet<BasicBlock> {
    let mut reachable = FxHashSet::default();
    let mut worklist = vec![body.entry_block()];

    while let Some(block_id) = worklist.pop() {
        if reachable.contains(&block_id) {
            continue;
        }
        reachable.insert(block_id);

        // Add successors to worklist.
        if let Some(block) = body.basic_blocks.get(block_id) {
            for succ in block.terminator.kind.successors() {
                if !reachable.contains(&succ) {
                    worklist.push(succ);
                }
            }
        }
    }

    reachable
}

/// Find all locals that are read (used as operands or in places).
fn find_used_locals(body: &Body) -> FxHashSet<Local> {
    let mut used = FxHashSet::default();

    // Arguments and return pointer are always used.
    used.insert(body.return_local());
    for i in 1..=body.arg_count {
        used.insert(Local::new(i as u32));
    }

    for block in body.basic_blocks.iter() {
        // Scan statements for local usage.
        for stmt in &block.statements {
            match stmt {
                Statement::Assign(place, rvalue) => {
                    collect_locals_in_place(place, &mut used);
                    collect_locals_in_rvalue(rvalue, &mut used);
                }
                Statement::Nop => {}
            }
        }

        // Scan terminator for local usage.
        collect_locals_in_terminator(&block.terminator.kind, &mut used);
    }

    used
}

fn collect_locals_in_place(place: &Place, used: &mut FxHashSet<Local>) {
    used.insert(place.local);
    for proj in &place.projection {
        if let crate::place::Projection::Index(index_local) = proj {
            used.insert(*index_local);
        }
    }
}

fn collect_locals_in_rvalue(rvalue: &crate::body::Rvalue, used: &mut FxHashSet<Local>) {
    use crate::body::Rvalue;
    match rvalue {
        Rvalue::Use(operand) => collect_locals_in_operand(operand, used),
        Rvalue::BinaryOp(_, a, b) => {
            collect_locals_in_operand(a, used);
            collect_locals_in_operand(b, used);
        }
        Rvalue::UnaryOp(_, operand) => collect_locals_in_operand(operand, used),
        Rvalue::Ref(place) | Rvalue::AddressOf(place) | Rvalue::Len(place) => {
            collect_locals_in_place(place, used);
        }
        Rvalue::Aggregate(_, operands) => {
            for operand in operands {
                collect_locals_in_operand(operand, used);
            }
        }
        Rvalue::Cast(operand, _) => collect_locals_in_operand(operand, used),
        Rvalue::Repeat(operand, _) => collect_locals_in_operand(operand, used),
    }
}

fn collect_locals_in_operand(operand: &Operand, used: &mut FxHashSet<Local>) {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => collect_locals_in_place(place, used),
        Operand::Constant(_) => {}
    }
}

fn collect_locals_in_terminator(kind: &TerminatorKind, used: &mut FxHashSet<Local>) {
    match kind {
        TerminatorKind::SwitchInt { discr, .. } => collect_locals_in_operand(discr, used),
        TerminatorKind::Call {
            func,
            args,
            destination,
            ..
        } => {
            collect_locals_in_operand(func, used);
            for arg in args {
                collect_locals_in_operand(arg, used);
            }
            collect_locals_in_place(destination, used);
        }
        TerminatorKind::Drop { place, .. } => collect_locals_in_place(place, used),
        TerminatorKind::Assert { cond, .. } => collect_locals_in_operand(cond, used),
        _ => {}
    }
}

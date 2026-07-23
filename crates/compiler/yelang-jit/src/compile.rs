//! Bytecode → Cranelift IR translation.
//!
//! The Yelang VM is a stack machine over the rich [`Value`] enum. Compiling
//! *all* of that to native code would require boxing every value, which would
//! defeat the point. Instead, the JIT targets the **hot numeric kernel**
//! subset that dominates aggregate / arithmetic query pipelines:
//!
//! * integer / boolean constants and arithmetic (`+ - * / %`, neg, bitwise),
//! * signed comparisons,
//! * local variables,
//! * structured and unstructured control flow (`jump`, `jumpif`, loops),
//! * `return` / `halt`.
//!
//! These instructions are lowered to native `i64` machine code that operates
//! on a memory-backed operand stack (a Cranelift stack slot), so arbitrary
//! jumps — including backward jumps for loops — translate cleanly without
//! needing SSA block parameters for the operand stack.
//!
//! Anything outside this subset (floats, strings, arrays, structs, calls,
//! query operators, …) makes a function *not JIT-able*; [`is_jittable`]
//! reports this and the [`crate::backend::JitBackend`] falls back to the
//! interpreter for those functions.

use std::collections::{BTreeSet, HashMap, VecDeque};

use cranelift::frontend::FunctionBuilder;
use cranelift_codegen::ir::{
    condcodes::IntCC, types, Block, InstBuilder, MemFlagsData, StackSlotData, StackSlotKind,
    Value as ClifValue,
};
use yelang_vm::{CompiledFunction, Instruction, Value};

/// Errors that can occur while JIT-compiling a function.
#[derive(Debug, thiserror::Error)]
pub enum JitError {
    /// The bytecode uses instructions outside the JIT-able subset.
    #[error("bytecode not supported by the JIT: {0}")]
    Unsupported(String),
    /// The operand stack height at a program counter is path-dependent,
    /// which the memory-stack lowering cannot represent.
    #[error("inconsistent operand stack height at pc {0}")]
    InconsistentStackHeight(usize),
    /// Cranelift codegen / ISA initialisation failed.
    #[error("cranelift codegen error: {0}")]
    Codegen(String),
    /// The cranelift-module layer failed (declare / define / finalize).
    #[error("cranelift module error: {0}")]
    Module(String),
    /// The interpreter fallback failed.
    #[error("interpreter error: {0}")]
    Interpret(String),
}

/// Whether every instruction in `func` belongs to the JIT-able subset.
///
/// When this returns `false`, the function must be run on the interpreter.
pub fn is_jittable(func: &CompiledFunction) -> bool {
    // Arguments are laid into local slots `0..num_args`, so we need at least
    // that many local slots for the lowering to be valid.
    if func.num_locals < func.num_args {
        return false;
    }
    func.instructions.iter().all(|insn| match insn {
        Instruction::PushConst(v) => const_fits_i64(v),
        Instruction::LoadLocal(slot) | Instruction::StoreLocal(slot) => *slot < func.num_locals,
        other => stack_delta(other).is_some(),
    })
}

/// Whether a constant can be represented as a native `i64`.
fn const_fits_i64(v: &Value) -> bool {
    match v {
        Value::Int(i) => (i128::from(i64::MIN)..=i128::from(i64::MAX)).contains(i),
        Value::Uint(u) => *u <= i64::MAX as u128,
        Value::Bool(_) => true,
        _ => false,
    }
}

/// Net operand-stack effect of a JIT-able instruction, or `None` if the
/// instruction is outside the supported subset.
fn stack_delta(insn: &Instruction) -> Option<i32> {
    use Instruction::*;
    let delta = match insn {
        PushConst(Value::Int(_) | Value::Uint(_) | Value::Bool(_)) => 1,
        Pop => -1,
        Dup => 1,
        Swap => 0,
        Add | Sub | Mul | Div | Rem => -1,
        Neg | Not => 0,
        Eq | Ne | Lt | Le | Gt | Ge => -1,
        BitAnd | BitOr | BitXor | Shl | Shr => -1,
        LoadLocal(_) => 1,
        StoreLocal(_) => -1,
        Jump(_) => 0,
        JumpIf(_) | JumpIfNot(_) => -1,
        Return | Halt => 0,
        Nop => 0,
        _ => return None,
    };
    Some(delta)
}

/// Number of operand-stack slots an instruction pops (for underflow checks).
fn required_pops(insn: &Instruction) -> i32 {
    use Instruction::*;
    match insn {
        Add | Sub | Mul | Div | Rem | Eq | Ne | Lt | Le | Gt | Ge | BitAnd | BitOr | BitXor
        | Shl | Shr => 2,
        Pop | StoreLocal(_) | JumpIf(_) | JumpIfNot(_) | Neg | Not => 1,
        _ => 0,
    }
}

/// Compute the operand-stack height at every program counter via a worklist
/// fixpoint.
///
/// Returns a vector of length `instructions.len() + 1`; the final slot is the
/// height at the implicit function-exit point. `None` marks an unreachable
/// program counter. A program counter reached with two different heights is a
/// hard error (the memory-stack lowering needs a single height per block).
fn compute_heights(func: &CompiledFunction) -> Result<Vec<Option<i32>>, JitError> {
    let n = func.instructions.len();
    let mut heights: Vec<Option<i32>> = vec![None; n + 1];
    let mut queue: VecDeque<usize> = VecDeque::new();
    heights[0] = Some(0);
    queue.push_back(0);

    while let Some(pc) = queue.pop_front() {
        if pc == n {
            continue; // function-exit pseudo-pc; no instruction to process
        }
        let h = heights[pc].expect("queued pc must have a height");
        let insn = &func.instructions[pc];

        if h < required_pops(insn) {
            return Err(JitError::Unsupported(format!(
                "operand stack underflow at pc {pc}"
            )));
        }

        // Propagate to successors, checking height consistency.
        let mut relax = |target: usize, height: i32| -> Result<(), JitError> {
            if target > n {
                return Err(JitError::Unsupported(format!(
                    "jump target {target} out of range"
                )));
            }
            match heights[target] {
                Some(existing) if existing != height => {
                    Err(JitError::InconsistentStackHeight(target))
                }
                Some(_) => Ok(()),
                None => {
                    heights[target] = Some(height);
                    queue.push_back(target);
                    Ok(())
                }
            }
        };

        match insn {
            Instruction::Jump(target) => relax(*target as usize, h)?,
            Instruction::JumpIf(target) | Instruction::JumpIfNot(target) => {
                let after = h - 1;
                relax(*target as usize, after)?;
                relax(pc + 1, after)?;
            }
            Instruction::Return | Instruction::Halt => { /* no successors */ }
            other => {
                let delta = stack_delta(other).ok_or_else(|| {
                    JitError::Unsupported(format!("instruction {other:?} at pc {pc}"))
                })?;
                relax(pc + 1, h + delta)?;
            }
        }
    }

    Ok(heights)
}

/// Lower `func` into Cranelift IR using `builder`.
///
/// The function's CLIF signature is assumed to already be set on the builder's
/// function: `num_args` × `i64` parameters and a single `i64` return.
pub fn build_function(
    func: &CompiledFunction,
    builder: &mut FunctionBuilder,
) -> Result<(), JitError> {
    let heights = compute_heights(func)?;
    let n = func.instructions.len();

    // ── Discover basic-block leaders ────────────────────────────────────
    let mut leader_set: BTreeSet<usize> = BTreeSet::new();
    // Loop headers: targets of backward jumps. These have a predecessor that
    // is emitted later in layout order, so they must be sealed last.
    let mut loop_headers: BTreeSet<usize> = BTreeSet::new();
    leader_set.insert(0);
    for (pc, insn) in func.instructions.iter().enumerate() {
        match insn {
            Instruction::Jump(t) | Instruction::JumpIf(t) | Instruction::JumpIfNot(t) => {
                let t = *t as usize;
                if t <= n {
                    leader_set.insert(t);
                    if t <= pc {
                        loop_headers.insert(t); // backward jump → loop header
                    }
                }
                leader_set.insert(pc + 1); // fall-through / post-jump boundary
            }
            _ => {}
        }
    }
    // Keep only reachable leaders (those with a known stack height).
    let leaders: Vec<usize> = leader_set
        .into_iter()
        .filter(|&pc| heights[pc].is_some())
        .collect();

    // ── Create one Cranelift block per reachable leader ─────────────────
    let mut pc_to_block: HashMap<usize, Block> = HashMap::new();
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    pc_to_block.insert(0, entry);
    for &pc in leaders.iter().skip(1) {
        pc_to_block.insert(pc, builder.create_block());
    }

    // ── Stack slots: operand stack + locals ─────────────────────────────
    let max_height = heights.iter().filter_map(|h| *h).max().unwrap_or(0);
    let op_entries = (max_height + 1).max(1) as u32;
    let op_slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        op_entries * 8,
        8,
    ));
    let local_entries = func.num_locals.max(1);
    let local_slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        local_entries * 8,
        8,
    ));

    let trusted = MemFlagsData::trusted();

    // `op_base` / `local_base` are SSA values defined in the entry block
    // (which dominates every other block) and reused throughout. They are
    // assigned when we emit the entry block (`k == 0`), which is always the
    // first leader, so they are initialised before any block can use them.
    let mut op_base_opt: Option<ClifValue> = None;
    let mut local_base_opt: Option<ClifValue> = None;

    // ── Emit each basic block ───────────────────────────────────────────
    for (k, &leader) in leaders.iter().enumerate() {
        let block = pc_to_block[&leader];
        builder.switch_to_block(block);
        // Loop headers have a back-edge predecessor that is emitted later in
        // layout order, so they must not be sealed until every block (and
        // therefore every predecessor) has been processed. Non-loop blocks
        // only have forward-edge predecessors, which are already in place.
        if !loop_headers.contains(&leader) {
            builder.seal_block(block);
        }

        // Prologue: emitted once, at the top of the entry block, before the
        // function body. Defines the operand-stack and locals base pointers.
        if k == 0 {
            let params: Vec<ClifValue> = builder.block_params(block).to_vec();
            let op_b = builder.ins().stack_addr(types::I64, op_slot, 0);
            let local_b = builder.ins().stack_addr(types::I64, local_slot, 0);

            // Zero-initialise every local slot (the VM starts locals as Null/0).
            for slot in 0..func.num_locals {
                let zero = builder.ins().iconst(types::I64, 0);
                builder
                    .ins()
                    .store(trusted, zero, local_b, (slot * 8) as i32);
            }
            // Lay arguments into local slots 0..num_args.
            for (i, param) in params.iter().enumerate() {
                if (i as u32) < func.num_args && (i as u32) < func.num_locals {
                    builder
                        .ins()
                        .store(trusted, *param, local_b, (i * 8) as i32);
                }
            }

            op_base_opt = Some(op_b);
            local_base_opt = Some(local_b);
        }
        let op_base = op_base_opt.expect("entry block is emitted first");
        let local_base = local_base_opt.expect("entry block is emitted first");

        let end = leaders.get(k + 1).copied().unwrap_or(n + 1);
        let mut terminated = false;

        for pc in leader..end.min(n) {
            let Some(h) = heights[pc] else {
                continue; // unreachable instruction (dead code), skip
            };
            let insn = &func.instructions[pc];

            // Helper offsets into the operand stack.
            let at = |slot: i32| (slot * 8) as i32;

            match insn {
                Instruction::PushConst(v) => {
                    let c = iconst(builder, v);
                    builder.ins().store(trusted, c, op_base, at(h));
                }
                Instruction::Pop => { /* height simply decreases */ }
                Instruction::Dup => {
                    let v = builder.ins().load(types::I64, trusted, op_base, at(h - 1));
                    builder.ins().store(trusted, v, op_base, at(h));
                }
                Instruction::Swap => {
                    let a = builder.ins().load(types::I64, trusted, op_base, at(h - 1));
                    let b = builder.ins().load(types::I64, trusted, op_base, at(h - 2));
                    builder.ins().store(trusted, b, op_base, at(h - 1));
                    builder.ins().store(trusted, a, op_base, at(h - 2));
                }
                Instruction::Add
                | Instruction::Sub
                | Instruction::Mul
                | Instruction::Div
                | Instruction::Rem
                | Instruction::BitAnd
                | Instruction::BitOr
                | Instruction::BitXor
                | Instruction::Shl
                | Instruction::Shr => {
                    let b = builder.ins().load(types::I64, trusted, op_base, at(h - 1));
                    let a = builder.ins().load(types::I64, trusted, op_base, at(h - 2));
                    let r = binop(builder, insn, a, b);
                    builder.ins().store(trusted, r, op_base, at(h - 2));
                }
                Instruction::Neg => {
                    let a = builder.ins().load(types::I64, trusted, op_base, at(h - 1));
                    let r = builder.ins().ineg(a);
                    builder.ins().store(trusted, r, op_base, at(h - 1));
                }
                Instruction::Not => {
                    let a = builder.ins().load(types::I64, trusted, op_base, at(h - 1));
                    let r = builder.ins().bnot(a);
                    builder.ins().store(trusted, r, op_base, at(h - 1));
                }
                Instruction::Eq
                | Instruction::Ne
                | Instruction::Lt
                | Instruction::Le
                | Instruction::Gt
                | Instruction::Ge => {
                    let b = builder.ins().load(types::I64, trusted, op_base, at(h - 1));
                    let a = builder.ins().load(types::I64, trusted, op_base, at(h - 2));
                    let cmp = builder.ins().icmp(int_cc(insn), a, b);
                    let r = builder.ins().uextend(types::I64, cmp);
                    builder.ins().store(trusted, r, op_base, at(h - 2));
                }
                Instruction::LoadLocal(slot) => {
                    let v = builder
                        .ins()
                        .load(types::I64, trusted, local_base, (*slot * 8) as i32);
                    builder.ins().store(trusted, v, op_base, at(h));
                }
                Instruction::StoreLocal(slot) => {
                    let v = builder.ins().load(types::I64, trusted, op_base, at(h - 1));
                    builder
                        .ins()
                        .store(trusted, v, local_base, (*slot * 8) as i32);
                }
                Instruction::Jump(target) => {
                    let dest = pc_to_block[&(*target as usize)];
                    builder.ins().jump(dest, &[]);
                    terminated = true;
                    break;
                }
                Instruction::JumpIf(target) => {
                    let c = builder.ins().load(types::I64, trusted, op_base, at(h - 1));
                    let zero = builder.ins().iconst(types::I64, 0);
                    let cond = builder.ins().icmp(IntCC::NotEqual, c, zero);
                    let then_b = pc_to_block[&(*target as usize)];
                    let else_b = pc_to_block[&(pc + 1)];
                    builder.ins().brif(cond, then_b, &[], else_b, &[]);
                    terminated = true;
                    break;
                }
                Instruction::JumpIfNot(target) => {
                    let c = builder.ins().load(types::I64, trusted, op_base, at(h - 1));
                    let zero = builder.ins().iconst(types::I64, 0);
                    let cond = builder.ins().icmp(IntCC::Equal, c, zero);
                    let then_b = pc_to_block[&(*target as usize)];
                    let else_b = pc_to_block[&(pc + 1)];
                    builder.ins().brif(cond, then_b, &[], else_b, &[]);
                    terminated = true;
                    break;
                }
                Instruction::Return | Instruction::Halt => {
                    let rv = return_value(builder, op_base, h);
                    builder.ins().return_(&[rv]);
                    terminated = true;
                    break;
                }
                Instruction::Nop => {}
                other => {
                    return Err(JitError::Unsupported(format!(
                        "instruction {other:?} at pc {pc}"
                    )));
                }
            }
        }

        if !terminated {
            // Block fell through without an explicit terminator.
            match leaders.get(k + 1) {
                Some(&next) => {
                    let dest = pc_to_block[&next];
                    builder.ins().jump(dest, &[]);
                }
                None => {
                    // Fell off the end of the function: implicit return of the
                    // operand-stack top at the exit height.
                    let exit_h = heights[n].unwrap_or(0);
                    let rv = return_value(builder, op_base, exit_h);
                    builder.ins().return_(&[rv]);
                }
            }
        }
    }

    // Seal the deferred loop headers now that every block — and thus every
    // predecessor, including back edges — has been emitted.
    for &leader in leaders.iter() {
        if loop_headers.contains(&leader) {
            builder.seal_block(pc_to_block[&leader]);
        }
    }

    Ok(())
}

/// Load the value to return: the operand-stack top, or `0` for an empty
/// stack (unit-returning function).
fn return_value(builder: &mut FunctionBuilder, op_base: ClifValue, height: i32) -> ClifValue {
    if height >= 1 {
        builder
            .ins()
            .load(types::I64, MemFlagsData::trusted(), op_base, ((height - 1) * 8) as i32)
    } else {
        builder.ins().iconst(types::I64, 0)
    }
}

/// Materialise an integer/boolean constant as an `i64`.
fn iconst(builder: &mut FunctionBuilder, v: &Value) -> ClifValue {
    let bits: i64 = match v {
        Value::Int(i) => *i as i64,
        Value::Uint(u) => *u as i64,
        Value::Bool(b) => i64::from(*b),
        _ => 0, // unreachable: is_jittable restricts PushConst operands
    };
    builder.ins().iconst(types::I64, bits)
}

/// Lower a binary arithmetic / bitwise instruction.
fn binop(
    builder: &mut FunctionBuilder,
    insn: &Instruction,
    a: ClifValue,
    b: ClifValue,
) -> ClifValue {
    match insn {
        Instruction::Add => builder.ins().iadd(a, b),
        Instruction::Sub => builder.ins().isub(a, b),
        Instruction::Mul => builder.ins().imul(a, b),
        Instruction::Div => builder.ins().sdiv(a, b),
        Instruction::Rem => builder.ins().srem(a, b),
        Instruction::BitAnd => builder.ins().band(a, b),
        Instruction::BitOr => builder.ins().bor(a, b),
        Instruction::BitXor => builder.ins().bxor(a, b),
        Instruction::Shl => builder.ins().ishl(a, b),
        Instruction::Shr => builder.ins().sshr(a, b),
        _ => unreachable!("binop called with non-binary instruction {insn:?}"),
    }
}

/// Map a comparison instruction to its signed integer condition code.
fn int_cc(insn: &Instruction) -> IntCC {
    match insn {
        Instruction::Eq => IntCC::Equal,
        Instruction::Ne => IntCC::NotEqual,
        Instruction::Lt => IntCC::SignedLessThan,
        Instruction::Le => IntCC::SignedLessThanOrEqual,
        Instruction::Gt => IntCC::SignedGreaterThan,
        Instruction::Ge => IntCC::SignedGreaterThanOrEqual,
        _ => unreachable!("int_cc called with non-comparison instruction {insn:?}"),
    }
}

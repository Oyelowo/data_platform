//! MIR → bytecode compiler.
//!
//! Compiles a MIR `Body` into a `CompiledFunction` (bytecode).
//! Walks basic blocks in layout order, emitting instructions for
//! each statement and terminator.

use yelang_mir::body::{BinOp, Body, Rvalue, Statement, UnOp};
use yelang_mir::ops::Operand;
use yelang_mir::place::Place;
use yelang_mir::terminator::TerminatorKind;

use crate::instruction::{CompiledFunction, Instruction};
use crate::value::Value;

/// Compile a MIR body into bytecode.
pub fn compile_mir(body: &Body) -> CompiledFunction {
    let mut compiler = MirCompiler::new(body);
    compiler.compile();
    compiler.finish()
}

struct MirCompiler<'a> {
    body: &'a Body,
    instructions: Vec<Instruction>,
    /// Map from MIR BasicBlock index to bytecode instruction index.
    block_offsets: Vec<usize>,
    /// Pending jumps that need to be patched with target offsets.
    pending_jumps: Vec<(usize, usize)>, // (instruction index, target block)
}

impl<'a> MirCompiler<'a> {
    fn new(body: &'a Body) -> Self {
        let block_count = body.basic_blocks.len();
        Self {
            body,
            instructions: Vec::new(),
            block_offsets: vec![0; block_count],
            pending_jumps: Vec::new(),
        }
    }

    fn compile(&mut self) {
        // Phase 1: compute block offsets.
        // Walk blocks in order, recording where each block starts.
        let block_count = self.body.basic_blocks.len();
        for i in 0..block_count {
            self.block_offsets[i] = self.instructions.len();
            let block_id = yelang_mir::BasicBlock::new(i as u32);
            if let Some(block) = self.body.basic_blocks.get(block_id) {
                // Emit statements.
                for stmt in &block.statements {
                    self.compile_statement(stmt);
                }
                // Emit terminator.
                self.compile_terminator(&block.terminator.kind);
            }
        }

        // Phase 2: patch pending jumps.
        for (instr_idx, target_block) in self.pending_jumps.clone() {
            if target_block < self.block_offsets.len() {
                let target_offset = self.block_offsets[target_block];
                match &mut self.instructions[instr_idx] {
                    Instruction::Jump(target) => *target = target_offset as u32,
                    Instruction::JumpIf(target) => *target = target_offset as u32,
                    Instruction::JumpIfNot(target) => *target = target_offset as u32,
                    _ => {}
                }
            }
        }
    }

    fn compile_statement(&mut self, stmt: &Statement) {
        match stmt {
            Statement::Assign(place, rvalue) => {
                self.compile_rvalue(rvalue);
                self.emit_store_place(place);
            }
            Statement::Nop => {
                self.emit(Instruction::Nop);
            }
        }
    }

    fn compile_rvalue(&mut self, rvalue: &Rvalue) {
        match rvalue {
            Rvalue::Use(operand) => {
                self.compile_operand(operand);
            }
            Rvalue::BinaryOp(op, lhs, rhs) => {
                self.compile_operand(lhs);
                self.compile_operand(rhs);
                let instr = match op {
                    BinOp::Add => Instruction::Add,
                    BinOp::Sub => Instruction::Sub,
                    BinOp::Mul => Instruction::Mul,
                    BinOp::Div => Instruction::Div,
                    BinOp::Rem => Instruction::Rem,
                    BinOp::BitAnd => Instruction::BitAnd,
                    BinOp::BitOr => Instruction::BitOr,
                    BinOp::BitXor => Instruction::BitXor,
                    BinOp::Shl => Instruction::Shl,
                    BinOp::Shr => Instruction::Shr,
                    BinOp::Eq => Instruction::Eq,
                    BinOp::Ne => Instruction::Ne,
                    BinOp::Lt => Instruction::Lt,
                    BinOp::Le => Instruction::Le,
                    BinOp::Gt => Instruction::Gt,
                    BinOp::Ge => Instruction::Ge,
                };
                self.emit(instr);
            }
            Rvalue::UnaryOp(op, operand) => {
                self.compile_operand(operand);
                let instr = match op {
                    UnOp::Not => Instruction::Not,
                    UnOp::Neg => Instruction::Neg,
                };
                self.emit(instr);
            }
            Rvalue::Ref(place) => {
                // For now, just load the place (no actual reference semantics).
                self.emit_load_place(place);
            }
            Rvalue::AddressOf(place) => {
                self.emit_load_place(place);
            }
            Rvalue::Aggregate(kind, operands) => {
                for operand in operands {
                    self.compile_operand(operand);
                }
                match kind {
                    yelang_mir::body::AggregateKind::Tuple => {
                        self.emit(Instruction::MakeTuple(operands.len() as u32));
                    }
                    yelang_mir::body::AggregateKind::Array(_) => {
                        self.emit(Instruction::MakeArray(operands.len() as u32));
                    }
                    yelang_mir::body::AggregateKind::Struct(def_id) => {
                        // Push field names as constants, then MakeStruct.
                        // For now, just make a struct with positional fields.
                        self.emit(Instruction::MakeStruct(def_id.raw() as u64, operands.len() as u32));
                    }
                    yelang_mir::body::AggregateKind::EnumVariant(def_id, variant_idx) => {
                        self.emit(Instruction::MakeEnumVariant(
                            def_id.raw() as u64,
                            *variant_idx,
                            operands.len() as u32,
                        ));
                    }
                }
            }
            Rvalue::Cast(operand, _ty) => {
                self.compile_operand(operand);
                // TODO: emit actual cast instruction based on target type.
                // For now, no-op (value stays as-is).
            }
            Rvalue::Repeat(operand, count) => {
                self.compile_operand(operand);
                // TODO: emit array repeat.
                // For now, just push the operand.
                let _ = count;
            }
            Rvalue::Len(place) => {
                self.emit_load_place(place);
                self.emit(Instruction::Len);
            }
        }
    }

    fn compile_operand(&mut self, operand: &Operand) {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                self.emit_load_place(place);
            }
            Operand::Constant(constant) => {
                let value = match &constant.value {
                    yelang_mir::ops::ConstValue::Int(i) => Value::Int(*i),
                    yelang_mir::ops::ConstValue::Uint(u) => Value::Uint(*u),
                    yelang_mir::ops::ConstValue::Float(f) => Value::Float(*f),
                    yelang_mir::ops::ConstValue::Bool(b) => Value::Bool(*b),
                    yelang_mir::ops::ConstValue::Char(c) => Value::Char(*c),
                    yelang_mir::ops::ConstValue::Str(s) => Value::Str(*s),
                    yelang_mir::ops::ConstValue::Unit => Value::Unit,
                    yelang_mir::ops::ConstValue::FnPtr(def_id) => {
                        Value::FnPtr(def_id.raw() as u64)
                    }
                };
                self.emit(Instruction::PushConst(value));
            }
        }
    }

    fn compile_terminator(&mut self, kind: &TerminatorKind) {
        match kind {
            TerminatorKind::Goto { target } => {
                let instr_idx = self.instructions.len();
                self.emit(Instruction::Jump(0)); // placeholder
                self.pending_jumps.push((instr_idx, target.raw() as usize));
            }
            TerminatorKind::SwitchInt { discr, targets } => {
                self.compile_operand(discr);
                // For each branch, emit a comparison + conditional jump.
                // Simplified: just jump to the otherwise block.
                let instr_idx = self.instructions.len();
                self.emit(Instruction::JumpIfNot(0)); // placeholder
                self.pending_jumps
                    .push((instr_idx, targets.otherwise.raw() as usize));
                // TODO: emit proper switch with all branches.
            }
            TerminatorKind::Return => {
                self.emit(Instruction::Return);
            }
            TerminatorKind::Call {
                func,
                args,
                destination,
                target,
            } => {
                // Push the function value.
                self.compile_operand(func);
                // Push arguments.
                for arg in args {
                    self.compile_operand(arg);
                }
                // Emit call.
                self.emit(Instruction::Call(args.len() as u32));
                // Store result in destination.
                self.emit_store_place(destination);
                // Jump to target block.
                let instr_idx = self.instructions.len();
                self.emit(Instruction::Jump(0)); // placeholder
                self.pending_jumps.push((instr_idx, target.raw() as usize));
            }
            TerminatorKind::Drop { place, target } => {
                // Drop is a no-op in Yelang (no destructors).
                let _ = place;
                let instr_idx = self.instructions.len();
                self.emit(Instruction::Jump(0));
                self.pending_jumps.push((instr_idx, target.raw() as usize));
            }
            TerminatorKind::Assert {
                cond,
                expected,
                target,
                ..
            } => {
                self.compile_operand(cond);
                if !expected {
                    self.emit(Instruction::Not);
                }
                let instr_idx = self.instructions.len();
                self.emit(Instruction::JumpIfNot(0));
                self.pending_jumps.push((instr_idx, target.raw() as usize));
            }
            TerminatorKind::Unreachable => {
                self.emit(Instruction::Halt);
            }
        }
    }

    fn emit_load_place(&mut self, place: &Place) {
        self.emit(Instruction::LoadLocal(place.local.raw()));
        // Apply projections.
        for proj in &place.projection {
            match proj {
                yelang_mir::place::Projection::Field(name, _) => {
                    self.emit(Instruction::LoadField(*name));
                }
                yelang_mir::place::Projection::Index(index_local) => {
                    self.emit(Instruction::LoadLocal(index_local.raw()));
                    self.emit(Instruction::Index);
                }
                yelang_mir::place::Projection::Deref => {
                    // Deref is a no-op in Yelang (no pointers).
                }
            }
        }
    }

    fn emit_store_place(&mut self, place: &Place) {
        if place.projection.is_empty() {
            self.emit(Instruction::StoreLocal(place.local.raw()));
        } else {
            // For projected places, we need to load the base, modify, and store back.
            // Simplified: just store to the base local.
            // TODO: handle field/index stores properly.
            self.emit(Instruction::StoreLocal(place.local.raw()));
        }
    }

    fn emit(&mut self, instr: Instruction) {
        self.instructions.push(instr);
    }

    fn finish(self) -> CompiledFunction {
        CompiledFunction {
            name: self.body.name,
            instructions: self.instructions,
            num_locals: self.body.locals.len() as u32,
            num_args: self.body.arg_count as u32,
        }
    }
}

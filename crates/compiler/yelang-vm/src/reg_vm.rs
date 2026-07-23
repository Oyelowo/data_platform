//! Register-based bytecode virtual machine.
//!
//! A register VM that executes [`RegInstruction`] bytecode. Compared to the
//! stack-based [`Vm`](crate::vm::Vm), operands live in a fixed register file
//! addressed by index, which avoids the push/pop traffic that dominates the
//! stack VM for our ~40-byte [`Value`]s (the Lua-style register design is
//! roughly 2× faster for values larger than 8 bytes).
//!
//! # Operands
//!
//! Arithmetic / comparison / bitwise instructions use **RK operands**: if the
//! high bit ([`RK_FLAG`](crate::reg_instruction::RK_FLAG)) of an operand is
//! set, the low 8 bits index into the current function's constant pool;
//! otherwise the operand is a register index. See
//! [`is_rk`](crate::reg_instruction::is_rk) and
//! [`rk_index`](crate::reg_instruction::rk_index).
//!
//! # Functions
//!
//! A [`RegProgram`] holds a flat list of [`RegFunction`]s. Each function owns
//! its register count, argument count, and constant pool. `Call` pushes a
//! [`RegCallFrame`] that snapshots the caller's registers, constants, and
//! instruction pointer; `Return` restores them and writes the result into the
//! caller's destination register.

use yelang_interner::Symbol;

use crate::reg_instruction::{is_rk, rk_index, RegInstruction};
use crate::storage::{EmptyStorage, StorageBackend};
use crate::value::Value;

/// A compiled register-based function.
#[derive(Debug, Clone)]
pub struct RegFunction {
    /// Optional symbolic name (for debugging).
    pub name: Option<String>,
    /// The instruction stream.
    pub instructions: Vec<RegInstruction>,
    /// Number of registers this function's frame allocates.
    pub num_registers: u8,
    /// Number of arguments (copied into registers `0..num_args` on call).
    pub num_args: u8,
    /// The function's constant pool (addressed by RK operands).
    pub constants: Vec<Value>,
}

/// A program: a flat list of functions plus an entry point.
#[derive(Debug, Clone, Default)]
pub struct RegProgram {
    /// The functions, indexed by id.
    pub functions: Vec<RegFunction>,
    /// The entry function id.
    pub entry: Option<u64>,
}

impl RegProgram {
    /// Create an empty program with no functions.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a function and return its id.
    pub fn add_function(&mut self, func: RegFunction) -> u64 {
        let id = self.functions.len() as u64;
        self.functions.push(func);
        id
    }

    /// Look up a function by id.
    pub fn get_function(&self, id: u64) -> Option<&RegFunction> {
        self.functions.get(id as usize)
    }
}

/// A call frame saved on the call stack when a function is invoked.
///
/// The frame snapshots the *caller's* execution state so that `Return` can
/// restore it and deliver the result into the caller's destination register.
#[derive(Debug, Clone)]
pub struct RegCallFrame {
    /// The caller's function id.
    pub function_id: u64,
    /// Instruction pointer to resume at in the caller.
    pub return_ip: usize,
    /// Register in the caller's frame that receives the return value.
    pub return_register: u8,
    /// The caller's register file.
    pub registers: Vec<Value>,
    /// The caller's constant pool.
    pub constants: Vec<Value>,
}

/// Register-based VM execution error.
#[derive(Debug, Clone, thiserror::Error)]
pub enum RegVmError {
    #[error("register out of bounds: {}", .0)]
    RegisterOutOfBounds(usize),
    #[error("constant out of bounds: {}", .0)]
    ConstantOutOfBounds(usize),
    #[error("type error: expected {}, got {}", .0, .1)]
    TypeError(String, String),
    #[error("index out of bounds: {} (len {})", .0, .1)]
    IndexOutOfBounds(usize, usize),
    #[error("division by zero")]
    DivisionByZero,
    #[error("function not found: {}", .0)]
    FunctionNotFound(u64),
    #[error("call stack overflow (depth {})", .0)]
    CallStackOverflow(usize),
    #[error("execution limit exceeded")]
    ExecutionLimitExceeded,
}

/// The register-based bytecode virtual machine.
pub struct RegVm {
    /// The current frame's register file.
    registers: Vec<Value>,
    /// The current frame's constant pool.
    constants: Vec<Value>,
    /// Saved caller frames.
    call_stack: Vec<RegCallFrame>,
    /// The current instruction pointer.
    ip: usize,
    /// Whether the VM is halted.
    halted: bool,
    /// The id of the function currently executing.
    current_function: u64,
    /// The value produced by the most recent top-level `Return`/`Halt`.
    return_value: Value,
    /// Maximum call depth (safety limit).
    max_call_depth: usize,
    /// The storage backend for query scans.
    storage: Box<dyn StorageBackend>,
}

impl RegVm {
    /// Create a new register VM with default limits and an empty backend.
    pub fn new() -> Self {
        Self {
            registers: Vec::with_capacity(64),
            constants: Vec::new(),
            call_stack: Vec::with_capacity(32),
            ip: 0,
            halted: false,
            current_function: 0,
            return_value: Value::Null,
            max_call_depth: 1_000,
            storage: Box::new(EmptyStorage),
        }
    }

    /// Create a register VM with a custom call-depth limit.
    pub fn with_limits(max_call_depth: usize) -> Self {
        Self {
            max_call_depth,
            ..Self::new()
        }
    }

    /// Create a register VM backed by the given storage backend.
    pub fn with_storage(storage: Box<dyn StorageBackend>) -> Self {
        Self {
            storage,
            ..Self::new()
        }
    }

    /// Replace the storage backend, returning the previous one.
    pub fn set_storage(&mut self, storage: Box<dyn StorageBackend>) -> Box<dyn StorageBackend> {
        std::mem::replace(&mut self.storage, storage)
    }

    /// Execute a program's entry function with no arguments.
    pub fn execute(&mut self, program: &RegProgram) -> Result<Value, RegVmError> {
        self.execute_with_args(program, &[])
    }

    /// Execute a program's entry function, passing `args` into registers
    /// `0..args.len()` (clamped to the function's `num_args`).
    pub fn execute_with_args(
        &mut self,
        program: &RegProgram,
        args: &[Value],
    ) -> Result<Value, RegVmError> {
        let entry = program.entry.ok_or(RegVmError::FunctionNotFound(0))?;
        let func = program
            .get_function(entry)
            .ok_or(RegVmError::FunctionNotFound(entry))?;

        // Set up the entry frame.
        self.current_function = entry;
        self.registers = vec![Value::Null; func.num_registers as usize];
        for (i, arg) in args.iter().enumerate() {
            if i < func.num_args as usize && i < self.registers.len() {
                self.registers[i] = arg.clone();
            }
        }
        self.constants = func.constants.clone();
        self.call_stack.clear();
        self.ip = 0;
        self.halted = false;
        self.return_value = Value::Null;

        // Main execution loop.
        let mut step_count = 0u64;
        let max_steps = 100_000_000u64; // safety limit

        while !self.halted {
            step_count += 1;
            if step_count > max_steps {
                return Err(RegVmError::ExecutionLimitExceeded);
            }

            // Fetch the next instruction (copying it out so the program
            // borrow ends before we mutably dispatch).
            let instruction = {
                let func = program
                    .get_function(self.current_function)
                    .ok_or(RegVmError::FunctionNotFound(self.current_function))?;
                if self.ip >= func.instructions.len() {
                    // Fell off the end — implicit return of register 0.
                    self.return_from_call(0);
                    continue;
                }
                let instr = func.instructions[self.ip];
                self.ip += 1;
                instr
            };

            self.execute_instruction(program, &instruction)?;
        }

        Ok(self.return_value.clone())
    }

    /// Execute a single instruction.
    fn execute_instruction(
        &mut self,
        program: &RegProgram,
        instruction: &RegInstruction,
    ) -> Result<(), RegVmError> {
        match *instruction {
            // ── Data movement ──────────────────────────────────────────
            RegInstruction::Move { a, b } => {
                let val = self.get_reg(b)?;
                self.set_reg(a, val)?;
            }
            RegInstruction::LoadK { a, bx } => {
                let val = self.get_const(bx)?;
                self.set_reg(a, val)?;
            }
            RegInstruction::LoadNil { a } => {
                self.set_reg(a, Value::Null)?;
            }
            RegInstruction::LoadBool { a, value } => {
                self.set_reg(a, Value::Bool(value))?;
            }

            // ── Arithmetic ─────────────────────────────────────────────
            RegInstruction::Add { a, b, c } => {
                let (x, y) = self.binary_operands(b, c)?;
                self.set_reg(a, binary_arith(&x, &y, |p, q| p + q, |p, q| p + q)?)?;
            }
            RegInstruction::Sub { a, b, c } => {
                let (x, y) = self.binary_operands(b, c)?;
                self.set_reg(a, binary_arith(&x, &y, |p, q| p - q, |p, q| p - q)?)?;
            }
            RegInstruction::Mul { a, b, c } => {
                let (x, y) = self.binary_operands(b, c)?;
                self.set_reg(a, binary_arith(&x, &y, |p, q| p * q, |p, q| p * q)?)?;
            }
            RegInstruction::Div { a, b, c } => {
                let (x, y) = self.binary_operands(b, c)?;
                check_divisor(&y)?;
                self.set_reg(a, binary_arith(&x, &y, |p, q| p / q, |p, q| p / q)?)?;
            }
            RegInstruction::Rem { a, b, c } => {
                let (x, y) = self.binary_operands(b, c)?;
                check_divisor(&y)?;
                self.set_reg(a, binary_arith(&x, &y, |p, q| p % q, |p, q| p % q)?)?;
            }
            RegInstruction::Neg { a, b } => {
                let val = self.get_rk(b)?;
                let result = match val {
                    Value::Int(i) => Value::Int(-i),
                    Value::Float(f) => Value::Float(-f),
                    _ => {
                        return Err(RegVmError::TypeError(
                            "numeric".into(),
                            format!("{}", val),
                        ))
                    }
                };
                self.set_reg(a, result)?;
            }
            RegInstruction::Not { a, b } => {
                let val = self.get_rk(b)?;
                let result = match val {
                    Value::Bool(b) => Value::Bool(!b),
                    Value::Int(i) => Value::Int(!i),
                    Value::Uint(u) => Value::Uint(!u),
                    _ => {
                        return Err(RegVmError::TypeError(
                            "bool or int".into(),
                            format!("{}", val),
                        ))
                    }
                };
                self.set_reg(a, result)?;
            }

            // ── Comparison ─────────────────────────────────────────────
            RegInstruction::Eq { a, b, c } => {
                let (x, y) = self.binary_operands(b, c)?;
                self.set_reg(a, Value::Bool(x == y))?;
            }
            RegInstruction::Ne { a, b, c } => {
                let (x, y) = self.binary_operands(b, c)?;
                self.set_reg(a, Value::Bool(x != y))?;
            }
            RegInstruction::Lt { a, b, c } => {
                let (x, y) = self.binary_operands(b, c)?;
                self.set_reg(a, Value::Bool(cmp_ordered(&x, &y, |p, q| p < q, |p, q| p < q)))?;
            }
            RegInstruction::Le { a, b, c } => {
                let (x, y) = self.binary_operands(b, c)?;
                self.set_reg(a, Value::Bool(cmp_ordered(&x, &y, |p, q| p <= q, |p, q| p <= q)))?;
            }
            RegInstruction::Gt { a, b, c } => {
                let (x, y) = self.binary_operands(b, c)?;
                self.set_reg(a, Value::Bool(cmp_ordered(&x, &y, |p, q| p > q, |p, q| p > q)))?;
            }
            RegInstruction::Ge { a, b, c } => {
                let (x, y) = self.binary_operands(b, c)?;
                self.set_reg(a, Value::Bool(cmp_ordered(&x, &y, |p, q| p >= q, |p, q| p >= q)))?;
            }

            // ── Bitwise ────────────────────────────────────────────────
            RegInstruction::BitAnd { a, b, c } => {
                let (x, y) = self.binary_operands(b, c)?;
                self.set_reg(a, binary_int(&x, &y, |p, q| p & q)?)?;
            }
            RegInstruction::BitOr { a, b, c } => {
                let (x, y) = self.binary_operands(b, c)?;
                self.set_reg(a, binary_int(&x, &y, |p, q| p | q)?)?;
            }
            RegInstruction::BitXor { a, b, c } => {
                let (x, y) = self.binary_operands(b, c)?;
                self.set_reg(a, binary_int(&x, &y, |p, q| p ^ q)?)?;
            }
            RegInstruction::Shl { a, b, c } => {
                let (x, y) = self.binary_operands(b, c)?;
                self.set_reg(a, binary_int(&x, &y, |p, q| p << q)?)?;
            }
            RegInstruction::Shr { a, b, c } => {
                let (x, y) = self.binary_operands(b, c)?;
                self.set_reg(a, binary_int(&x, &y, |p, q| p >> q)?)?;
            }

            // ── Field access ───────────────────────────────────────────
            RegInstruction::GetField { a, b, field } => {
                let name = self.get_const_symbol(field)?;
                let obj = self.get_reg(b)?;
                let val = obj.get_field(name).cloned().unwrap_or(Value::Null);
                self.set_reg(a, val)?;
            }
            RegInstruction::SetField { a, b, field } => {
                let name = self.get_const_symbol(field)?;
                let val = self.get_reg(a)?;
                let mut obj = self.get_reg(b)?;
                obj.set_field(name, val);
                self.set_reg(b, obj)?;
            }

            // ── Array operations ───────────────────────────────────────
            RegInstruction::GetIndex { a, b, c } => {
                let arr = self.get_reg(b)?;
                let idx_val = self.get_reg(c)?;
                let idx = idx_val.as_usize().ok_or_else(|| {
                    RegVmError::TypeError("integer index".into(), format!("{}", idx_val))
                })?;
                let elem = arr.index(idx).cloned().ok_or_else(|| {
                    RegVmError::IndexOutOfBounds(idx, arr.len().unwrap_or(0))
                })?;
                self.set_reg(a, elem)?;
            }
            RegInstruction::SetIndex { a, b, c } => {
                let val = self.get_reg(a)?;
                let idx_val = self.get_reg(c)?;
                let idx = idx_val.as_usize().ok_or_else(|| {
                    RegVmError::TypeError("integer index".into(), format!("{}", idx_val))
                })?;
                let mut arr = self.get_reg(b)?;
                if let Value::Array(elems) = &mut arr {
                    if idx < elems.len() {
                        elems[idx] = val;
                    }
                }
                self.set_reg(b, arr)?;
            }
            RegInstruction::Len { a, b } => {
                let val = self.get_reg(b)?;
                let len = val.len().ok_or_else(|| {
                    RegVmError::TypeError("array or string".into(), format!("{}", val))
                })?;
                self.set_reg(a, Value::Uint(len as u128))?;
            }

            // ── Construction ───────────────────────────────────────────
            RegInstruction::MakeArray { a, b, count } => {
                let elems = self.collect_regs(b, count);
                self.set_reg(a, Value::Array(elems))?;
            }
            RegInstruction::MakeTuple { a, b, count } => {
                let elems = self.collect_regs(b, count);
                self.set_reg(a, Value::Tuple(elems))?;
            }
            RegInstruction::MakeStruct { a, def_id, b, count } => {
                // Registers hold `count` (name, value) pairs:
                // R(b+2i) is the field name (a Str), R(b+2i+1) its value.
                let start = b as usize;
                let mut fields = Vec::with_capacity(count as usize);
                for i in 0..count as usize {
                    let name_val = self
                        .registers
                        .get(start + 2 * i)
                        .cloned()
                        .unwrap_or(Value::Null);
                    let val = self
                        .registers
                        .get(start + 2 * i + 1)
                        .cloned()
                        .unwrap_or(Value::Null);
                    if let Value::Str(sym) = name_val {
                        fields.push((sym, val));
                    }
                }
                self.set_reg(a, Value::Struct(def_id as u64, fields))?;
            }

            // ── Control flow ───────────────────────────────────────────
            RegInstruction::Jump { bx } => {
                self.ip = bx as usize;
            }
            RegInstruction::JumpIf { a, bx } => {
                let cond = self.get_reg(a)?;
                if cond.is_truthy() {
                    self.ip = bx as usize;
                }
            }
            RegInstruction::JumpIfNot { a, bx } => {
                let cond = self.get_reg(a)?;
                if !cond.is_truthy() {
                    self.ip = bx as usize;
                }
            }

            // ── Functions ──────────────────────────────────────────────
            RegInstruction::Call { a, b, num_args } => {
                let callee = self.get_reg(b)?;
                let func_id = match callee {
                    Value::FnPtr(id) => id,
                    Value::Closure(id, _) => id,
                    _ => {
                        return Err(RegVmError::TypeError(
                            "function".into(),
                            format!("{}", callee),
                        ))
                    }
                };

                if self.call_stack.len() >= self.max_call_depth {
                    return Err(RegVmError::CallStackOverflow(self.call_stack.len()));
                }

                // Gather arguments from R(b+1)..R(b+1+num_args).
                let base = b as usize + 1;
                let args: Vec<Value> = (0..num_args as usize)
                    .map(|i| self.registers.get(base + i).cloned().unwrap_or(Value::Null))
                    .collect();

                let func = program
                    .get_function(func_id)
                    .ok_or(RegVmError::FunctionNotFound(func_id))?;

                // Snapshot the caller's frame.
                self.call_stack.push(RegCallFrame {
                    function_id: self.current_function,
                    return_ip: self.ip, // already advanced past the Call
                    return_register: a,
                    registers: std::mem::take(&mut self.registers),
                    constants: std::mem::take(&mut self.constants),
                });

                // Install the callee's frame.
                self.current_function = func_id;
                self.registers = vec![Value::Null; func.num_registers as usize];
                for (i, arg) in args.iter().enumerate() {
                    if i < func.num_args as usize && i < self.registers.len() {
                        self.registers[i] = arg.clone();
                    }
                }
                self.constants = func.constants.clone();
                self.ip = 0;
            }
            RegInstruction::Return { a } => {
                self.return_from_call(a);
            }

            // ── Iteration ──────────────────────────────────────────────
            RegInstruction::IterInit { a, b } => {
                let val = self.get_reg(b)?;
                let items = match val {
                    Value::Array(elems) => elems,
                    Value::QueryResult(rows) => rows,
                    _ => {
                        return Err(RegVmError::TypeError(
                            "iterable".into(),
                            format!("{}", val),
                        ))
                    }
                };
                self.set_reg(a, Value::Iterator(items, 0))?;
            }
            RegInstruction::IterNext { a, b } => {
                let iter = self.get_reg(b)?;
                match iter {
                    Value::Iterator(items, idx) => {
                        if idx < items.len() {
                            let val = items[idx].clone();
                            self.set_reg(a, val)?;
                            self.set_reg(a.wrapping_add(1), Value::Bool(true))?;
                            self.set_reg(b, Value::Iterator(items, idx + 1))?;
                        } else {
                            self.set_reg(a, Value::Null)?;
                            self.set_reg(a.wrapping_add(1), Value::Bool(false))?;
                            self.set_reg(b, Value::Iterator(items, idx))?;
                        }
                    }
                    _ => {
                        return Err(RegVmError::TypeError(
                            "iterator".into(),
                            format!("{}", iter),
                        ))
                    }
                }
            }

            // ── Query operations ───────────────────────────────────────
            RegInstruction::QueryScan { a, table_id } => {
                let rows = self.storage.scan_table(table_id as u64);
                self.set_reg(a, Value::QueryResult(rows))?;
            }
            RegInstruction::QueryFilter { a, b, .. } => {
                // Predicate evaluation is not wired into the register VM yet
                // (the predicate would be a closure requiring re-entrant
                // execution); pass the input through, mirroring the stack VM.
                let qr = self.get_reg(b)?;
                self.set_reg(a, qr)?;
            }
            RegInstruction::QueryProject { a, b } => {
                // Projection metadata is not carried by the register
                // instruction; pass through (see the stack VM's TODO).
                let qr = self.get_reg(b)?;
                self.set_reg(a, qr)?;
            }
            RegInstruction::QueryJoin { a, b, .. } => {
                // A join needs a JoinSpec (keys/kind), which the register
                // instruction does not carry; pass the left input through.
                let qr = self.get_reg(b)?;
                self.set_reg(a, qr)?;
            }
            RegInstruction::QueryAggregate { a, b } => {
                // Aggregation keys are not carried by the instruction; pass
                // through (see the stack VM's TODO).
                let qr = self.get_reg(b)?;
                self.set_reg(a, qr)?;
            }
            RegInstruction::QuerySort { a, b } => {
                // Sort keys are not carried by the instruction; pass through.
                let qr = self.get_reg(b)?;
                self.set_reg(a, qr)?;
            }
            RegInstruction::QueryLimit { a, b, c } => {
                // R(A) = limit(R(B), skip=R(C), fetch=R(C+1)).
                let rows = into_rows(self.get_reg(b)?)?;
                let skip = self.get_reg(c)?.as_usize().unwrap_or(0);
                let fetch = self
                    .get_reg(c.wrapping_add(1))?
                    .as_usize()
                    .unwrap_or(rows.len());
                let result: Vec<Value> = rows.into_iter().skip(skip).take(fetch).collect();
                self.set_reg(a, Value::QueryResult(result))?;
            }

            // ── Aggregates ─────────────────────────────────────────────
            RegInstruction::AggSum { a, b } => {
                let val = self.get_reg(b)?;
                let sum = match val {
                    Value::Array(elems) | Value::QueryResult(elems) => {
                        let sum: i128 = elems.iter().filter_map(|v| v.as_int()).sum();
                        Value::Int(sum)
                    }
                    _ => Value::Int(0),
                };
                self.set_reg(a, sum)?;
            }
            RegInstruction::AggCount { a, b } => {
                let val = self.get_reg(b)?;
                let count = val.len().unwrap_or(0);
                self.set_reg(a, Value::Uint(count as u128))?;
            }
            RegInstruction::AggAvg { a, b } => {
                let val = self.get_reg(b)?;
                let avg = match val {
                    Value::Array(elems) | Value::QueryResult(elems) => {
                        let floats: Vec<f64> =
                            elems.iter().filter_map(|v| v.as_float()).collect();
                        if floats.is_empty() {
                            0.0
                        } else {
                            floats.iter().sum::<f64>() / floats.len() as f64
                        }
                    }
                    _ => 0.0,
                };
                self.set_reg(a, Value::Float(avg))?;
            }
            RegInstruction::AggMin { a, b } => {
                let val = self.get_reg(b)?;
                let min = match val {
                    Value::Array(elems) | Value::QueryResult(elems) => elems
                        .iter()
                        .filter_map(|v| v.as_int())
                        .min()
                        .map(Value::Int)
                        .unwrap_or(Value::Null),
                    _ => Value::Null,
                };
                self.set_reg(a, min)?;
            }
            RegInstruction::AggMax { a, b } => {
                let val = self.get_reg(b)?;
                let max = match val {
                    Value::Array(elems) | Value::QueryResult(elems) => elems
                        .iter()
                        .filter_map(|v| v.as_int())
                        .max()
                        .map(Value::Int)
                        .unwrap_or(Value::Null),
                    _ => Value::Null,
                };
                self.set_reg(a, max)?;
            }

            // ── Misc ───────────────────────────────────────────────────
            RegInstruction::Nop => {}
            RegInstruction::Halt => {
                // Capture register 0 as the result, then stop.
                self.return_value =
                    self.registers.first().cloned().unwrap_or(Value::Null);
                self.halted = true;
            }
        }

        Ok(())
    }

    // ── Helper methods ─────────────────────────────────────────────────

    /// Read a register, returning an error if out of bounds.
    fn get_reg(&self, idx: u8) -> Result<Value, RegVmError> {
        self.registers
            .get(idx as usize)
            .cloned()
            .ok_or(RegVmError::RegisterOutOfBounds(idx as usize))
    }

    /// Write a register, growing the register file if needed.
    fn set_reg(&mut self, idx: u8, val: Value) -> Result<(), RegVmError> {
        let idx = idx as usize;
        if idx >= self.registers.len() {
            self.registers.resize(idx + 1, Value::Null);
        }
        self.registers[idx] = val;
        Ok(())
    }

    /// Read a constant by index.
    fn get_const(&self, idx: u16) -> Result<Value, RegVmError> {
        self.constants
            .get(idx as usize)
            .cloned()
            .ok_or(RegVmError::ConstantOutOfBounds(idx as usize))
    }

    /// Read a constant expected to be an interned symbol (field name).
    fn get_const_symbol(&self, idx: u16) -> Result<Symbol, RegVmError> {
        match self.constants.get(idx as usize) {
            Some(Value::Str(s)) => Ok(*s),
            Some(other) => Err(RegVmError::TypeError(
                "string constant".into(),
                format!("{}", other),
            )),
            None => Err(RegVmError::ConstantOutOfBounds(idx as usize)),
        }
    }

    /// Resolve an RK operand: a constant (high bit set) or a register.
    fn get_rk(&self, operand: u16) -> Result<Value, RegVmError> {
        if is_rk(operand) {
            self.get_const(rk_index(operand) as u16)
        } else {
            self.get_reg(operand as u8)
        }
    }

    /// Resolve the two RK operands of a binary instruction.
    fn binary_operands(&self, b: u16, c: u16) -> Result<(Value, Value), RegVmError> {
        let x = self.get_rk(b)?;
        let y = self.get_rk(c)?;
        Ok((x, y))
    }

    /// Collect `count` consecutive registers starting at `b`.
    fn collect_regs(&self, b: u8, count: u8) -> Vec<Value> {
        let start = b as usize;
        (0..count as usize)
            .map(|i| self.registers.get(start + i).cloned().unwrap_or(Value::Null))
            .collect()
    }

    /// Return from the current function, delivering R(a) to the caller.
    ///
    /// If there is no caller (the entry function returned), the VM halts and
    /// stores the result as the program's return value.
    fn return_from_call(&mut self, a: u8) {
        let result = self.registers.get(a as usize).cloned().unwrap_or(Value::Null);
        match self.call_stack.pop() {
            Some(frame) => {
                self.current_function = frame.function_id;
                self.ip = frame.return_ip;
                self.registers = frame.registers;
                self.constants = frame.constants;
                let rr = frame.return_register as usize;
                if rr < self.registers.len() {
                    self.registers[rr] = result;
                }
            }
            None => {
                self.return_value = result;
                self.halted = true;
            }
        }
    }
}

impl Default for RegVm {
    fn default() -> Self {
        Self::new()
    }
}

// ── Free helpers ───────────────────────────────────────────────────────────

/// Apply a binary arithmetic operation to two values, promoting to float when
/// either operand is a float.
fn binary_arith(
    a: &Value,
    b: &Value,
    int_op: fn(i128, i128) -> i128,
    float_op: fn(f64, f64) -> f64,
) -> Result<Value, RegVmError> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(int_op(*x, *y))),
        (Value::Uint(x), Value::Uint(y)) => {
            Ok(Value::Uint(int_op(*x as i128, *y as i128) as u128))
        }
        (Value::Float(x), Value::Float(y)) => Ok(Value::Float(float_op(*x, *y))),
        (Value::Int(x), Value::Float(y)) => Ok(Value::Float(float_op(*x as f64, *y))),
        (Value::Float(x), Value::Int(y)) => Ok(Value::Float(float_op(*x, *y as f64))),
        _ => Err(RegVmError::TypeError(
            "numeric".into(),
            format!("{} and {}", a, b),
        )),
    }
}

/// Reject a zero divisor for division / remainder.
fn check_divisor(b: &Value) -> Result<(), RegVmError> {
    let is_zero = matches!(b, Value::Int(0) | Value::Uint(0) | Value::Float(0.0));
    if is_zero {
        Err(RegVmError::DivisionByZero)
    } else {
        Ok(())
    }
}

/// Ordered comparison: integers compare exactly, anything numeric falls back
/// to float comparison.
fn cmp_ordered(
    a: &Value,
    b: &Value,
    int_op: fn(i128, i128) -> bool,
    float_op: fn(f64, f64) -> bool,
) -> bool {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => int_op(*x, *y),
        (Value::Uint(x), Value::Uint(y)) => int_op(*x as i128, *y as i128),
        _ => match (a.as_float(), b.as_float()) {
            (Some(x), Some(y)) => float_op(x, y),
            _ => false,
        },
    }
}

/// Apply a bitwise integer operation to two values.
fn binary_int(
    a: &Value,
    b: &Value,
    op: fn(i128, i128) -> i128,
) -> Result<Value, RegVmError> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(op(*x, *y))),
        (Value::Uint(x), Value::Uint(y)) => Ok(Value::Uint(op(*x as i128, *y as i128) as u128)),
        _ => Err(RegVmError::TypeError(
            "integer".into(),
            format!("{} and {}", a, b),
        )),
    }
}

/// Unwrap a value into a row vector for query operators.
fn into_rows(value: Value) -> Result<Vec<Value>, RegVmError> {
    match value {
        Value::QueryResult(rows) | Value::Array(rows) => Ok(rows),
        other => Err(RegVmError::TypeError(
            "query result".into(),
            format!("{}", other),
        )),
    }
}

//! The Yelang bytecode virtual machine.
//!
//! Stack-based VM that executes compiled bytecode. Supports:
//! - Regular code (from MIR → bytecode)
//! - Query operations (from QIR → bytecode)
//! - Function calls with a call stack
//! - All Yelang value types

use std::cmp::Ordering;
use std::collections::HashMap;

use yelang_interner::Symbol;

use crate::instruction::{CompiledFunction, CompiledProgram, Instruction, WindowAgg, WindowFunc};
use crate::join::{JoinAlgorithm, JoinKind, JoinSpec};
use crate::storage::{EmptyStorage, StorageBackend};
use crate::traverse::{TraverseDirection, TraverseSpec};
use crate::value::Value;

/// A call frame on the call stack.
#[derive(Debug)]
struct CallFrame {
    /// The function being executed.
    _function_id: u64,
    /// The instruction pointer to return to after the call.
    return_ip: usize,
    /// The base index into the locals array for this frame.
    local_base: usize,
}

/// The Yelang bytecode virtual machine.
pub struct Vm {
    /// The value stack.
    stack: Vec<Value>,
    /// Local variables for all active call frames.
    locals: Vec<Value>,
    /// The call stack.
    call_stack: Vec<CallFrame>,
    /// The current instruction pointer.
    ip: usize,
    /// The current function being executed.
    current_function: Option<CompiledFunction>,
    /// Whether the VM is halted.
    halted: bool,
    /// Maximum stack depth (safety limit).
    max_stack_depth: usize,
    /// Maximum call depth (safety limit).
    max_call_depth: usize,
    /// The storage backend for query scans and link traversals.
    storage: Box<dyn StorageBackend>,
}

/// VM execution error.
#[derive(Debug, Clone, thiserror::Error)]
pub enum VmError {
    #[error("stack underflow")]
    StackUnderflow,
    #[error("stack overflow (depth {})", .0)]
    StackOverflow(usize),
    #[error("call stack overflow (depth {})", .0)]
    CallStackOverflow(usize),
    #[error("invalid local slot {}", .0)]
    InvalidLocal(u32),
    #[error("type error: expected {}, got {}", .0, .1)]
    TypeError(String, String),
    #[error("index out of bounds: {} (len {})", .0, .1)]
    IndexOutOfBounds(usize, usize),
    #[error("division by zero")]
    DivisionByZero,
    #[error("function not found: {}", .0)]
    FunctionNotFound(u64),
    #[error("no current function")]
    NoCurrentFunction,
    #[error("execution limit exceeded")]
    ExecutionLimitExceeded,
}

impl Vm {
    /// Create a new VM with default limits and an empty storage backend.
    pub fn new() -> Self {
        Self {
            stack: Vec::with_capacity(256),
            locals: Vec::with_capacity(64),
            call_stack: Vec::with_capacity(32),
            ip: 0,
            current_function: None,
            halted: false,
            max_stack_depth: 10_000,
            max_call_depth: 1_000,
            storage: Box::new(EmptyStorage),
        }
    }

    /// Create a VM with custom limits and an empty storage backend.
    pub fn with_limits(max_stack_depth: usize, max_call_depth: usize) -> Self {
        Self {
            max_stack_depth,
            max_call_depth,
            ..Self::new()
        }
    }

    /// Create a VM backed by the given storage backend.
    ///
    /// The backend supplies rows for `QueryScan` and the edge/target tables
    /// for `QueryTraverse`.
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

    /// Execute a compiled program and return the result.
    pub fn execute(&mut self, program: &CompiledProgram) -> Result<Value, VmError> {
        let entry = program.entry.ok_or(VmError::FunctionNotFound(0))?;
        let func = program
            .get_function(entry)
            .ok_or(VmError::FunctionNotFound(entry))?;

        self.call_function(func, &[])?;

        // Main execution loop.
        let mut step_count = 0u64;
        let max_steps = 100_000_000u64; // safety limit

        while !self.halted {
            step_count += 1;
            if step_count > max_steps {
                return Err(VmError::ExecutionLimitExceeded);
            }

            let func = self
                .current_function
                .as_ref()
                .ok_or(VmError::NoCurrentFunction)?;

            if self.ip >= func.instructions.len() {
                // Fell off the end of the function — implicit return.
                self.return_from_call()?;
                continue;
            }

            let instruction = func.instructions[self.ip].clone();
            self.ip += 1;

            self.execute_instruction(&instruction, program)?;
        }

        // The result is the top of the stack (if any).
        Ok(self.stack.pop().unwrap_or(Value::Unit))
    }

    /// Execute a single instruction.
    fn execute_instruction(
        &mut self,
        instruction: &Instruction,
        program: &CompiledProgram,
    ) -> Result<(), VmError> {
        match instruction {
            // ── Stack operations ───────────────────────────────────────
            Instruction::PushConst(val) => {
                self.push(val.clone())?;
            }
            Instruction::Pop => {
                self.pop()?;
            }
            Instruction::Dup => {
                let val = self.peek()?.clone();
                self.push(val)?;
            }
            Instruction::Swap => {
                let len = self.stack.len();
                if len < 2 {
                    return Err(VmError::StackUnderflow);
                }
                self.stack.swap(len - 1, len - 2);
            }

            // ── Arithmetic ─────────────────────────────────────────────
            Instruction::Add => self.binary_arith(|a, b| a + b, |a, b| a + b)?,
            Instruction::Sub => self.binary_arith(|a, b| a - b, |a, b| a - b)?,
            Instruction::Mul => self.binary_arith(|a, b| a * b, |a, b| a * b)?,
            Instruction::Div => {
                // Check for division by zero.
                let b = self.peek()?;
                let is_zero = match b {
                    Value::Int(0) | Value::Uint(0) | Value::Float(0.0) => true,
                    _ => false,
                };
                if is_zero {
                    return Err(VmError::DivisionByZero);
                }
                self.binary_arith(|a, b| a / b, |a, b| a / b)?;
            }
            Instruction::Rem => {
                let b = self.peek()?;
                let is_zero = match b {
                    Value::Int(0) | Value::Uint(0) => true,
                    _ => false,
                };
                if is_zero {
                    return Err(VmError::DivisionByZero);
                }
                self.binary_arith(|a, b| a % b, |a, b| a % b)?;
            }
            Instruction::Neg => {
                let val = self.pop()?;
                let result = match val {
                    Value::Int(i) => Value::Int(-i),
                    Value::Float(f) => Value::Float(-f),
                    _ => {
                        return Err(VmError::TypeError(
                            "numeric".into(),
                            format!("{}", val),
                        ))
                    }
                };
                self.push(result)?;
            }
            Instruction::Not => {
                let val = self.pop()?;
                let result = match val {
                    Value::Bool(b) => Value::Bool(!b),
                    Value::Int(i) => Value::Int(!i),
                    Value::Uint(u) => Value::Uint(!u),
                    _ => {
                        return Err(VmError::TypeError(
                            "bool or int".into(),
                            format!("{}", val),
                        ))
                    }
                };
                self.push(result)?;
            }

            // ── Comparison ─────────────────────────────────────────────
            Instruction::Eq => self.binary_cmp(|a, b| a == b)?,
            Instruction::Ne => self.binary_cmp(|a, b| a != b)?,
            Instruction::Lt => self.binary_cmp_ordered(|a, b| a < b)?,
            Instruction::Le => self.binary_cmp_ordered(|a, b| a <= b)?,
            Instruction::Gt => self.binary_cmp_ordered(|a, b| a > b)?,
            Instruction::Ge => self.binary_cmp_ordered(|a, b| a >= b)?,

            // ── Bitwise ────────────────────────────────────────────────
            Instruction::BitAnd => self.binary_int_op(|a, b| a & b)?,
            Instruction::BitOr => self.binary_int_op(|a, b| a | b)?,
            Instruction::BitXor => self.binary_int_op(|a, b| a ^ b)?,
            Instruction::Shl => self.binary_int_op(|a, b| a << b)?,
            Instruction::Shr => self.binary_int_op(|a, b| a >> b)?,

            // ── Local variables ────────────────────────────────────────
            Instruction::LoadLocal(slot) => {
                let base = self
                    .call_stack
                    .last()
                    .map(|f| f.local_base)
                    .unwrap_or(0);
                let idx = base + *slot as usize;
                let val = self
                    .locals
                    .get(idx)
                    .cloned()
                    .unwrap_or(Value::Null);
                self.push(val)?;
            }
            Instruction::StoreLocal(slot) => {
                let val = self.pop()?;
                let base = self
                    .call_stack
                    .last()
                    .map(|f| f.local_base)
                    .unwrap_or(0);
                let idx = base + *slot as usize;
                if idx >= self.locals.len() {
                    self.locals.resize(idx + 1, Value::Null);
                }
                self.locals[idx] = val;
            }

            // ── Field access ───────────────────────────────────────────
            Instruction::LoadField(name) => {
                let val = self.pop()?;
                let field = val.get_field(*name).cloned().ok_or_else(|| {
                    VmError::TypeError(
                        format!("struct with field {:?}", name),
                        format!("{}", val),
                    )
                })?;
                self.push(field)?;
            }
            Instruction::StoreField(name) => {
                let val = self.pop()?;
                let mut obj = self.pop()?;
                obj.set_field(*name, val);
                self.push(obj)?;
            }

            // ── Array operations ───────────────────────────────────────
            Instruction::Index => {
                let idx_val = self.pop()?;
                let arr = self.pop()?;
                let idx = idx_val.as_usize().ok_or_else(|| {
                    VmError::TypeError("integer index".into(), format!("{}", idx_val))
                })?;
                let elem = arr.index(idx).cloned().ok_or_else(|| {
                    VmError::IndexOutOfBounds(idx, arr.len().unwrap_or(0))
                })?;
                self.push(elem)?;
            }
            Instruction::StoreIndex => {
                let val = self.pop()?;
                let idx_val = self.pop()?;
                let mut arr = self.pop()?;
                let idx = idx_val.as_usize().ok_or_else(|| {
                    VmError::TypeError("integer index".into(), format!("{}", idx_val))
                })?;
                if let Value::Array(elems) = &mut arr {
                    if idx < elems.len() {
                        elems[idx] = val;
                    }
                }
                self.push(arr)?;
            }
            Instruction::Len => {
                let val = self.pop()?;
                let len = val.len().ok_or_else(|| {
                    VmError::TypeError("array or string".into(), format!("{}", val))
                })?;
                self.push(Value::Uint(len as u128))?;
            }

            // ── Construction ───────────────────────────────────────────
            Instruction::MakeArray(n) => {
                let n = *n as usize;
                if self.stack.len() < n {
                    return Err(VmError::StackUnderflow);
                }
                let start = self.stack.len() - n;
                let elems: Vec<Value> = self.stack.drain(start..).collect();
                self.push(Value::Array(elems))?;
            }
            Instruction::MakeTuple(n) => {
                let n = *n as usize;
                if self.stack.len() < n {
                    return Err(VmError::StackUnderflow);
                }
                let start = self.stack.len() - n;
                let elems: Vec<Value> = self.stack.drain(start..).collect();
                self.push(Value::Tuple(elems))?;
            }
            Instruction::MakeStruct(def_id, n) => {
                let n = *n as usize;
                if self.stack.len() < n * 2 {
                    return Err(VmError::StackUnderflow);
                }
                let start = self.stack.len() - n * 2;
                let pairs: Vec<Value> = self.stack.drain(start..).collect();
                let mut fields = Vec::with_capacity(n);
                for chunk in pairs.chunks(2) {
                    if let (Value::Str(name), val) = (&chunk[0], &chunk[1]) {
                        fields.push((*name, val.clone()));
                    }
                }
                self.push(Value::Struct(*def_id, fields))?;
            }
            Instruction::MakeEnumVariant(def_id, variant_idx, n) => {
                let n = *n as usize;
                if self.stack.len() < n {
                    return Err(VmError::StackUnderflow);
                }
                let start = self.stack.len() - n;
                let vals: Vec<Value> = self.stack.drain(start..).collect();
                self.push(Value::EnumVariant(*def_id, *variant_idx, vals))?;
            }

            // ── Option / Result ────────────────────────────────────────
            Instruction::MakeSome => {
                let val = self.pop()?;
                self.push(Value::Option(Some(Box::new(val))))?;
            }
            Instruction::MakeNone => {
                self.push(Value::Option(None))?;
            }
            Instruction::MakeOk => {
                let val = self.pop()?;
                self.push(Value::Result(Ok(Box::new(val))))?;
            }
            Instruction::MakeErr => {
                let val = self.pop()?;
                self.push(Value::Result(Err(Box::new(val))))?;
            }

            // ── Control flow ───────────────────────────────────────────
            Instruction::Jump(target) => {
                self.ip = *target as usize;
            }
            Instruction::JumpIf(target) => {
                let cond = self.pop()?;
                if cond.is_truthy() {
                    self.ip = *target as usize;
                }
            }
            Instruction::JumpIfNot(target) => {
                let cond = self.pop()?;
                if !cond.is_truthy() {
                    self.ip = *target as usize;
                }
            }

            // ── Functions ──────────────────────────────────────────────
            Instruction::Call(num_args) => {
                let func_val = self.pop()?;
                let func_id = match func_val {
                    Value::FnPtr(id) => id,
                    Value::Closure(id, _) => id,
                    _ => {
                        return Err(VmError::TypeError(
                            "function".into(),
                            format!("{}", func_val),
                        ))
                    }
                };
                let func = program
                    .get_function(func_id)
                    .ok_or(VmError::FunctionNotFound(func_id))?;

                // Collect arguments from the stack.
                let n = *num_args as usize;
                if self.stack.len() < n {
                    return Err(VmError::StackUnderflow);
                }
                let start = self.stack.len() - n;
                let args: Vec<Value> = self.stack.drain(start..).collect();

                self.call_function(func, &args)?;
            }
            Instruction::Return => {
                self.return_from_call()?;
            }

            // ── Iteration ──────────────────────────────────────────────
            Instruction::IterInit => {
                let val = self.pop()?;
                let items = match val {
                    Value::Array(elems) => elems,
                    Value::QueryResult(rows) => rows,
                    _ => {
                        return Err(VmError::TypeError(
                            "iterable".into(),
                            format!("{}", val),
                        ))
                    }
                };
                self.push(Value::Iterator(items, 0))?;
            }
            Instruction::IterNext => {
                let iter = self.pop()?;
                match iter {
                    Value::Iterator(items, idx) => {
                        if idx < items.len() {
                            let val = items[idx].clone();
                            self.push(Value::Iterator(items, idx + 1))?;
                            self.push(val)?;
                            self.push(Value::Bool(true))?;
                        } else {
                            self.push(Value::Iterator(items, idx))?;
                            self.push(Value::Null)?;
                            self.push(Value::Bool(false))?;
                        }
                    }
                    _ => {
                        return Err(VmError::TypeError(
                            "iterator".into(),
                            format!("{}", iter),
                        ))
                    }
                }
            }

            // ── Query operations ───────────────────────────────────────
            Instruction::QueryScan(table_id) => {
                // Scan the table from the storage backend.
                let rows = self.storage.scan_table(*table_id);
                self.push(Value::QueryResult(rows))?;
            }
            Instruction::QueryFilter => {
                // TODO: apply filter predicate to QueryResult.
                // For now, pass through.
                let _pred = self.pop()?;
                let qr = self.pop()?;
                self.push(qr)?;
            }
            Instruction::QueryProject(_fields) => {
                // TODO: project fields from QueryResult.
                let qr = self.pop()?;
                self.push(qr)?;
            }
            Instruction::QueryJoin(spec) => {
                // Stack: [..., left, right] — right is on top.
                let right = self.pop()?;
                let left = self.pop()?;
                let left_rows = into_rows(left)?;
                let right_rows = into_rows(right)?;
                let result = execute_join(&left_rows, &right_rows, spec);
                self.push(Value::QueryResult(result))?;
            }
            Instruction::QueryAggregate(_keys) => {
                // TODO: aggregate QueryResult.
                let qr = self.pop()?;
                self.push(qr)?;
            }
            Instruction::QuerySort(_keys) => {
                // TODO: sort QueryResult.
                let qr = self.pop()?;
                self.push(qr)?;
            }
            Instruction::QueryLimit => {
                // TODO: limit QueryResult.
                let _fetch = self.pop()?;
                let _skip = self.pop()?;
                let qr = self.pop()?;
                self.push(qr)?;
            }
            Instruction::QueryTraverse(spec) => {
                let qr = self.pop()?;
                let rows = into_rows(qr)?;
                // Pull the edge and target tables from storage (owned copies,
                // so the storage borrow ends before we mutate the stack).
                let edges = self.storage.scan_table(spec.edge_table);
                let targets = self.storage.scan_table(spec.target_table);
                let result = execute_traverse(&rows, &edges, &targets, spec);
                self.push(Value::QueryResult(result))?;
            }

            // ── Window operations ──────────────────────────────────────
            Instruction::Window {
                partition_by,
                order_by,
                func,
                output,
            } => {
                let qr = self.pop()?;
                let rows = into_rows(qr)?;
                let result =
                    execute_window(&rows, partition_by, order_by, func, *output);
                self.push(Value::QueryResult(result))?;
            }

            // ── Aggregate operations ───────────────────────────────────
            Instruction::AggSum => {
                let val = self.pop()?;
                let sum = match val {
                    Value::Array(elems) | Value::QueryResult(elems) => {
                        let sum: i128 = elems
                            .iter()
                            .filter_map(|v| v.as_int())
                            .sum();
                        Value::Int(sum)
                    }
                    _ => Value::Int(0),
                };
                self.push(sum)?;
            }
            Instruction::AggCount => {
                let val = self.pop()?;
                let count = val.len().unwrap_or(0);
                self.push(Value::Uint(count as u128))?;
            }
            Instruction::AggAvg => {
                let val = self.pop()?;
                let avg = match val {
                    Value::Array(elems) | Value::QueryResult(elems) => {
                        let floats: Vec<f64> = elems
                            .iter()
                            .filter_map(|v| v.as_float())
                            .collect();
                        if floats.is_empty() {
                            0.0
                        } else {
                            floats.iter().sum::<f64>() / floats.len() as f64
                        }
                    }
                    _ => 0.0,
                };
                self.push(Value::Float(avg))?;
            }
            Instruction::AggMin => {
                let val = self.pop()?;
                let min = match val {
                    Value::Array(elems) | Value::QueryResult(elems) => {
                        elems
                            .iter()
                            .filter_map(|v| v.as_int())
                            .min()
                            .map(Value::Int)
                            .unwrap_or(Value::Null)
                    }
                    _ => Value::Null,
                };
                self.push(min)?;
            }
            Instruction::AggMax => {
                let val = self.pop()?;
                let max = match val {
                    Value::Array(elems) | Value::QueryResult(elems) => {
                        elems
                            .iter()
                            .filter_map(|v| v.as_int())
                            .max()
                            .map(Value::Int)
                            .unwrap_or(Value::Null)
                    }
                    _ => Value::Null,
                };
                self.push(max)?;
            }

            // ── Misc ───────────────────────────────────────────────────
            Instruction::Nop => {}
            Instruction::Halt => {
                self.halted = true;
            }
        }

        Ok(())
    }

    // ── Helper methods ─────────────────────────────────────────────────

    fn push(&mut self, val: Value) -> Result<(), VmError> {
        if self.stack.len() >= self.max_stack_depth {
            return Err(VmError::StackOverflow(self.stack.len()));
        }
        self.stack.push(val);
        Ok(())
    }

    fn pop(&mut self) -> Result<Value, VmError> {
        self.stack.pop().ok_or(VmError::StackUnderflow)
    }

    fn peek(&self) -> Result<&Value, VmError> {
        self.stack.last().ok_or(VmError::StackUnderflow)
    }

    fn call_function(
        &mut self,
        func: &CompiledFunction,
        args: &[Value],
    ) -> Result<(), VmError> {
        if self.call_stack.len() >= self.max_call_depth {
            return Err(VmError::CallStackOverflow(self.call_stack.len()));
        }

        let local_base = self.locals.len();
        // Allocate local slots.
        self.locals
            .resize(local_base + func.num_locals as usize, Value::Null);
        // Copy arguments into local slots.
        for (i, arg) in args.iter().enumerate() {
            if i < func.num_args as usize {
                self.locals[local_base + i] = arg.clone();
            }
        }

        // Save the current state.
        let return_ip = self.ip;
        let prev_function = self.current_function.take();

        // Push the call frame.
        self.call_stack.push(CallFrame {
            _function_id: 0, // TODO: track function IDs
            return_ip,
            local_base,
        });

        // Set up the new function.
        self.current_function = Some(func.clone());
        self.ip = 0;

        // Save the previous function for restoration on return.
        // We store it in the call frame's function_id field.
        // For now, we just set the current function.
        let _ = prev_function;

        Ok(())
    }

    fn return_from_call(&mut self) -> Result<(), VmError> {
        let frame = self.call_stack.pop().ok_or(VmError::NoCurrentFunction)?;

        // Restore the previous state.
        self.ip = frame.return_ip;
        self.locals.truncate(frame.local_base);

        // Restore the previous function.
        // For now, we just clear the current function.
        // The execution loop will handle the case where there's no function.
        if self.call_stack.is_empty() {
            self.halted = true;
        }

        Ok(())
    }

    fn binary_arith(
        &mut self,
        int_op: fn(i128, i128) -> i128,
        float_op: fn(f64, f64) -> f64,
    ) -> Result<(), VmError> {
        let b = self.pop()?;
        let a = self.pop()?;
        let result = match (&a, &b) {
            (Value::Int(x), Value::Int(y)) => Value::Int(int_op(*x, *y)),
            (Value::Uint(x), Value::Uint(y)) => {
                Value::Uint(int_op(*x as i128, *y as i128) as u128)
            }
            (Value::Float(x), Value::Float(y)) => Value::Float(float_op(*x, *y)),
            (Value::Int(x), Value::Float(y)) => Value::Float(float_op(*x as f64, *y)),
            (Value::Float(x), Value::Int(y)) => Value::Float(float_op(*x, *y as f64)),
            _ => {
                return Err(VmError::TypeError(
                    "numeric".into(),
                    format!("{} and {}", a, b),
                ))
            }
        };
        self.push(result)
    }

    fn binary_cmp(&mut self, op: fn(&Value, &Value) -> bool) -> Result<(), VmError> {
        let b = self.pop()?;
        let a = self.pop()?;
        self.push(Value::Bool(op(&a, &b)))
    }

    fn binary_cmp_ordered(
        &mut self,
        op: fn(i128, i128) -> bool,
    ) -> Result<(), VmError> {
        let b = self.pop()?;
        let a = self.pop()?;
        let result = match (&a, &b) {
            (Value::Int(x), Value::Int(y)) => op(*x, *y),
            (Value::Uint(x), Value::Uint(y)) => op(*x as i128, *y as i128),
            (Value::Float(x), Value::Float(y)) => {
                // Float comparison.
                match (x.partial_cmp(y), &op as *const _) {
                    (Some(ord), _) => match ord {
                        std::cmp::Ordering::Less => {
                            // Check if op is < or <=
                            op(0, 1) // true for < and <=
                        }
                        std::cmp::Ordering::Equal => op(0, 0),
                        std::cmp::Ordering::Greater => op(1, 0),
                    },
                    (None, _) => false, // NaN comparison
                }
            }
            _ => false,
        };
        self.push(Value::Bool(result))
    }

    fn binary_int_op(
        &mut self,
        op: fn(i128, i128) -> i128,
    ) -> Result<(), VmError> {
        let b = self.pop()?;
        let a = self.pop()?;
        let result = match (&a, &b) {
            (Value::Int(x), Value::Int(y)) => Value::Int(op(*x, *y)),
            (Value::Uint(x), Value::Uint(y)) => {
                Value::Uint(op(*x as i128, *y as i128) as u128)
            }
            _ => {
                return Err(VmError::TypeError(
                    "integer".into(),
                    format!("{} and {}", a, b),
                ))
            }
        };
        self.push(result)
    }
}

impl Default for Vm {
    fn default() -> Self {
        Self::new()
    }
}

// ── Query execution helpers ────────────────────────────────────────────────

/// Unwrap a stack value into a row vector for query operators.
fn into_rows(value: Value) -> Result<Vec<Value>, VmError> {
    match value {
        Value::QueryResult(rows) | Value::Array(rows) => Ok(rows),
        other => Err(VmError::TypeError(
            "query result".into(),
            format!("{}", other),
        )),
    }
}

/// Read a struct field, returning `Null` when the row is not a struct or the
/// field is absent.
fn field_of(row: &Value, name: Symbol) -> Value {
    row.get_field(name).cloned().unwrap_or(Value::Null)
}

/// Return a copy of `row` with an extra field appended. Non-struct rows are
/// wrapped in a fresh struct so the output column is never lost.
fn with_added_field(row: &Value, name: Symbol, value: Value) -> Value {
    match row {
        Value::Struct(def_id, fields) => {
            let mut new_fields = fields.clone();
            // Replace an existing field of the same name, else append.
            if let Some(slot) = new_fields.iter_mut().find(|(n, _)| *n == name) {
                slot.1 = value;
            } else {
                new_fields.push((name, value));
            }
            Value::Struct(*def_id, new_fields)
        }
        _ => Value::Struct(0, vec![(name, value)]),
    }
}

/// Total ordering between two runtime values.
///
/// Numeric values compare numerically; `Null` sorts before everything; other
/// types fall back to booleans then their display form so the ordering is
/// deterministic.
fn compare_values(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => Ordering::Less,
        (_, Value::Null) => Ordering::Greater,
        _ => match (a.as_float(), b.as_float()) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
            _ => match (a.as_bool(), b.as_bool()) {
                (Some(x), Some(y)) => x.cmp(&y),
                _ => a.to_string().cmp(&b.to_string()),
            },
        },
    }
}

/// Lexicographically compare two key tuples, applying per-key direction
/// (`ascending[i] == false` reverses the i-th key).
fn compare_key_tuples(a: &[Value], b: &[Value], ascending: &[bool]) -> Ordering {
    for i in 0..a.len().min(b.len()) {
        let mut ord = compare_values(&a[i], &b[i]);
        if !ascending.get(i).copied().unwrap_or(true) {
            ord = ord.reverse();
        }
        if ord != Ordering::Equal {
            return ord;
        }
    }
    a.len().cmp(&b.len())
}

/// Compute a window function over partitions and return the rows with the
/// `output` column added. Input row order is preserved.
fn execute_window(
    rows: &[Value],
    partition_by: &[Symbol],
    order_by: &[(Symbol, bool)],
    func: &WindowFunc,
    output: Symbol,
) -> Vec<Value> {
    // Group row indices into partitions, preserving first-seen order.
    let mut partitions: Vec<(Vec<Value>, Vec<usize>)> = Vec::new();
    for (idx, row) in rows.iter().enumerate() {
        let key: Vec<Value> = partition_by.iter().map(|k| field_of(row, *k)).collect();
        match partitions.iter_mut().find(|(pk, _)| *pk == key) {
            Some((_, members)) => members.push(idx),
            None => partitions.push((key, vec![idx])),
        }
    }

    let ascending: Vec<bool> = order_by.iter().map(|(_, asc)| *asc).collect();

    // Compute (original_index, output_row) for every row, then restore order.
    let mut computed: Vec<(usize, Value)> = Vec::with_capacity(rows.len());

    for (_key, mut members) in partitions {
        // Order the partition by the ORDER BY keys.
        members.sort_by(|&a, &b| {
            let ka: Vec<Value> = order_by.iter().map(|(k, _)| field_of(&rows[a], *k)).collect();
            let kb: Vec<Value> = order_by.iter().map(|(k, _)| field_of(&rows[b], *k)).collect();
            compare_key_tuples(&ka, &kb, &ascending)
        });

        let n = members.len();

        // Precompute the ordered key tuples for ranking comparisons.
        let sorted_keys: Vec<Vec<Value>> = members
            .iter()
            .map(|&i| order_by.iter().map(|(k, _)| field_of(&rows[i], *k)).collect())
            .collect();

        // Precompute RANK / DENSE_RANK for the whole partition.
        let mut ranks = vec![0usize; n];
        let mut dense_ranks = vec![0usize; n];
        if n > 0 {
            ranks[0] = 1;
            dense_ranks[0] = 1;
            for i in 1..n {
                let same =
                    compare_key_tuples(&sorted_keys[i - 1], &sorted_keys[i], &ascending)
                        == Ordering::Equal;
                if same {
                    ranks[i] = ranks[i - 1];
                    dense_ranks[i] = dense_ranks[i - 1];
                } else {
                    ranks[i] = i + 1; // gap after ties
                    dense_ranks[i] = dense_ranks[i - 1] + 1;
                }
            }
        }

        // Precompute a whole-partition aggregate value if needed.
        let partition_agg: Option<Value> = match func {
            WindowFunc::Aggregate(agg, field) => {
                let vals: Vec<Value> =
                    members.iter().map(|&i| field_of(&rows[i], *field)).collect();
                Some(compute_window_agg(*agg, &vals))
            }
            _ => None,
        };

        for (pos, &row_idx) in members.iter().enumerate() {
            let value = match func {
                WindowFunc::RowNumber => Value::Uint((pos + 1) as u128),
                WindowFunc::Rank => Value::Uint(ranks[pos] as u128),
                WindowFunc::DenseRank => Value::Uint(dense_ranks[pos] as u128),
                WindowFunc::Lag(field, offset) => {
                    if pos >= *offset {
                        field_of(&rows[members[pos - *offset]], *field)
                    } else {
                        Value::Null
                    }
                }
                WindowFunc::Lead(field, offset) => {
                    if pos + *offset < n {
                        field_of(&rows[members[pos + *offset]], *field)
                    } else {
                        Value::Null
                    }
                }
                WindowFunc::Aggregate(_, _) => {
                    partition_agg.clone().unwrap_or(Value::Null)
                }
            };
            let new_row = with_added_field(&rows[row_idx], output, value);
            computed.push((row_idx, new_row));
        }
    }

    // Restore the original input row order.
    computed.sort_by_key(|(idx, _)| *idx);
    computed.into_iter().map(|(_, row)| row).collect()
}

/// Reduce a slice of field values with a window aggregate.
fn compute_window_agg(agg: WindowAgg, vals: &[Value]) -> Value {
    match agg {
        WindowAgg::Count => Value::Uint(vals.len() as u128),
        WindowAgg::Sum => Value::Int(vals.iter().filter_map(|v| v.as_int()).sum()),
        WindowAgg::Avg => {
            let floats: Vec<f64> = vals.iter().filter_map(|v| v.as_float()).collect();
            if floats.is_empty() {
                Value::Float(0.0)
            } else {
                Value::Float(floats.iter().sum::<f64>() / floats.len() as f64)
            }
        }
        WindowAgg::Min => vals
            .iter()
            .filter_map(|v| v.as_int())
            .min()
            .map(Value::Int)
            .unwrap_or(Value::Null),
        WindowAgg::Max => vals
            .iter()
            .filter_map(|v| v.as_int())
            .max()
            .map(Value::Int)
            .unwrap_or(Value::Null),
    }
}

/// Nested-loop link traversal.
///
/// For each input row, find matching edges and resolve them to target rows,
/// collecting the matches into a nested array stored under `spec.output`.
fn execute_traverse(
    rows: &[Value],
    edges: &[Value],
    targets: &[Value],
    spec: &TraverseSpec,
) -> Vec<Value> {
    let mut result = Vec::with_capacity(rows.len());

    for row in rows {
        let source_val = field_of(row, spec.source_key);
        let mut matched: Vec<Value> = Vec::new();

        for edge in edges {
            let edge_src = field_of(edge, spec.source_column);
            let edge_tgt = field_of(edge, spec.target_column);

            // Which target-key value does this edge point at, given the
            // traversal direction? `None` means the edge does not match.
            let lookup: Option<Value> = match spec.direction {
                TraverseDirection::Out => {
                    if edge_src == source_val {
                        Some(edge_tgt)
                    } else {
                        None
                    }
                }
                TraverseDirection::In => {
                    if edge_tgt == source_val {
                        Some(edge_src)
                    } else {
                        None
                    }
                }
                TraverseDirection::Both => {
                    if edge_src == source_val {
                        Some(edge_tgt)
                    } else if edge_tgt == source_val {
                        Some(edge_src)
                    } else {
                        None
                    }
                }
            };

            let Some(lookup) = lookup else {
                continue;
            };

            for target in targets {
                if field_of(target, spec.target_key) == lookup {
                    matched.push(target.clone());
                }
            }
        }

        result.push(with_added_field(row, spec.output, Value::Array(matched)));
    }

    result
}

// ── Join execution helpers ─────────────────────────────────────────────────

/// A hashable, equality-comparable representation of a runtime [`Value`].
///
/// `Value` itself is not `Hash` (it contains `f64`), so join keys are projected
/// into this form before being used as `HashMap` keys. Numeric variants keep
/// their distinct tags so that `Int(1)` and `Uint(1)` hash consistently with
/// how the nested-loop path compares them (via `Value` equality).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum HashValue {
    Unit,
    Null,
    Bool(bool),
    Int(i128),
    Uint(u128),
    /// Stored as raw bits so that `NaN`/`-0.0` edge cases still hash.
    Float(u64),
    Char(char),
    Str(Symbol),
    /// Any composite value falls back to its display form.
    Other(String),
}

impl HashValue {
    fn from_value(value: &Value) -> Self {
        match value {
            Value::Unit => HashValue::Unit,
            Value::Null => HashValue::Null,
            Value::Bool(b) => HashValue::Bool(*b),
            Value::Int(i) => HashValue::Int(*i),
            Value::Uint(u) => HashValue::Uint(*u),
            Value::Float(f) => HashValue::Float(f.to_bits()),
            Value::Char(c) => HashValue::Char(*c),
            Value::Str(s) => HashValue::Str(*s),
            other => HashValue::Other(other.to_string()),
        }
    }
}

/// A hashable join key: the projected key-column values of a single row.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct HashKey(Vec<HashValue>);

/// Extract a row's join key (the tuple of `keys` field values) in hashable
/// form. Missing fields project to [`Value::Null`].
fn extract_key(row: &Value, keys: &[Symbol]) -> HashKey {
    HashKey(keys.iter().map(|k| HashValue::from_value(&field_of(row, *k))).collect())
}

/// Build the probe-side hash table: right join key → indices of the right rows
/// carrying that key.
fn build_hash_table(right: &[Value], right_keys: &[Symbol]) -> HashMap<HashKey, Vec<usize>> {
    let mut table: HashMap<HashKey, Vec<usize>> = HashMap::new();
    for (idx, row) in right.iter().enumerate() {
        table.entry(extract_key(row, right_keys)).or_default().push(idx);
    }
    table
}

/// Merge a matched left/right row pair into a single output row.
///
/// Left fields come first; right fields are appended unless a field of the same
/// name already exists (the left value wins on collision). A [`Value::Null`]
/// side (outer-join padding with no schema) contributes nothing.
fn combine_rows(left: &Value, right: &Value) -> Value {
    match (left, right) {
        (Value::Struct(def_id, left_fields), Value::Struct(_, right_fields)) => {
            let mut fields = left_fields.clone();
            for (name, val) in right_fields {
                if !fields.iter().any(|(n, _)| n == name) {
                    fields.push((*name, val.clone()));
                }
            }
            Value::Struct(*def_id, fields)
        }
        (Value::Null, _) => right.clone(),
        (_, Value::Null) => left.clone(),
        // Non-struct rows: keep both sides as a pair so neither is lost.
        _ => Value::Tuple(vec![left.clone(), right.clone()]),
    }
}

/// Build a null-padded row mirroring `template`'s field names, used to outer-
/// join the side that had no match. Returns [`Value::Null`] when there is no
/// template to derive a schema from.
fn null_row_like(template: Option<&Value>) -> Value {
    match template {
        Some(Value::Struct(def_id, fields)) => Value::Struct(
            *def_id,
            fields.iter().map(|(name, _)| (*name, Value::Null)).collect(),
        ),
        _ => Value::Null,
    }
}

/// Execute a join between two row collections per `spec`.
///
/// Equi-joins with a usable key on each side run as a hash build/probe when
/// [`JoinAlgorithm::Hash`] is requested; everything else (cross joins,
/// non-equi joins, keyless specs) runs as a nested loop. Both paths produce
/// identical output for the same inputs.
fn execute_join(left: &[Value], right: &[Value], spec: &JoinSpec) -> Vec<Value> {
    // Cross join: unconditional cartesian product, no predicate.
    if spec.kind == JoinKind::Cross {
        let mut out = Vec::with_capacity(left.len().saturating_mul(right.len()));
        for l in left {
            for r in right {
                out.push(combine_rows(l, r));
            }
        }
        return out;
    }

    let equi = spec.is_equi();
    let use_hash = equi && spec.algorithm == JoinAlgorithm::Hash;
    let hash_table = use_hash.then(|| build_hash_table(right, &spec.right_keys));

    // Null padding templates for outer joins.
    let null_right = null_row_like(right.first());
    let null_left = null_row_like(left.first());

    // For Right/Full joins, track which right rows found a match.
    let track_right = matches!(spec.kind, JoinKind::Right | JoinKind::Full);
    let mut right_matched = vec![false; right.len()];

    let mut out: Vec<Value> = Vec::new();

    for l in left {
        // Indices into `right` that match this left row.
        let matches: Vec<usize> = match &hash_table {
            Some(table) => table
                .get(&extract_key(l, &spec.left_keys))
                .cloned()
                .unwrap_or_default(),
            None => nested_loop_matches(l, right, spec, equi),
        };

        if track_right {
            for &ri in &matches {
                right_matched[ri] = true;
            }
        }

        match spec.kind {
            JoinKind::Inner => {
                for ri in matches {
                    out.push(combine_rows(l, &right[ri]));
                }
            }
            JoinKind::Left | JoinKind::Full => {
                if matches.is_empty() {
                    out.push(combine_rows(l, &null_right));
                } else {
                    for ri in matches {
                        out.push(combine_rows(l, &right[ri]));
                    }
                }
            }
            JoinKind::Right => {
                // Emit matched pairs only; unmatched left rows are dropped.
                // Unmatched right rows are appended after the loop.
                for ri in matches {
                    out.push(combine_rows(l, &right[ri]));
                }
            }
            JoinKind::Semi => {
                if !matches.is_empty() {
                    out.push(l.clone());
                }
            }
            JoinKind::Anti => {
                if matches.is_empty() {
                    out.push(l.clone());
                }
            }
            JoinKind::Cross => unreachable!("cross join handled above"),
        }
    }

    // Right/Full outer: emit unmatched right rows padded with null left columns.
    if track_right {
        for (ri, matched) in right_matched.iter().enumerate() {
            if !matched {
                out.push(combine_rows(&null_left, &right[ri]));
            }
        }
    }

    out
}

/// Nested-loop match scan: indices of the right rows that match `left_row`.
///
/// With `equi` true the rows match on join-key equality; otherwise every right
/// row matches (a keyless inner join degenerates to a cartesian product).
fn nested_loop_matches(
    left_row: &Value,
    right: &[Value],
    spec: &JoinSpec,
    equi: bool,
) -> Vec<usize> {
    if equi {
        let left_key = extract_key(left_row, &spec.left_keys);
        right
            .iter()
            .enumerate()
            .filter(|(_, r)| extract_key(r, &spec.right_keys) == left_key)
            .map(|(i, _)| i)
            .collect()
    } else {
        (0..right.len()).collect()
    }
}

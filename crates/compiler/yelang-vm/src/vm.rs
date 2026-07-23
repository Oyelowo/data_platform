//! The Yelang bytecode virtual machine.
//!
//! Stack-based VM that executes compiled bytecode. Supports:
//! - Regular code (from MIR → bytecode)
//! - Query operations (from QIR → bytecode)
//! - Function calls with a call stack
//! - All Yelang value types

use crate::instruction::{CompiledFunction, CompiledProgram, Instruction};
use crate::value::Value;

/// A call frame on the call stack.
#[derive(Debug)]
struct CallFrame {
    /// The function being executed.
    function_id: u64,
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
    /// Create a new VM with default limits.
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
        }
    }

    /// Create a VM with custom limits.
    pub fn with_limits(max_stack_depth: usize, max_call_depth: usize) -> Self {
        Self {
            max_stack_depth,
            max_call_depth,
            ..Self::new()
        }
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
            Instruction::QueryScan(_table_id) => {
                // TODO: connect to storage engine.
                // For now, push an empty QueryResult.
                self.push(Value::QueryResult(vec![]))?;
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
            Instruction::QueryJoin => {
                // TODO: join two QueryResults.
                let _pred = self.pop()?;
                let right = self.pop()?;
                let left = self.pop()?;
                // For now, return left.
                let _ = right;
                self.push(left)?;
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
            Instruction::QueryTraverse => {
                // TODO: traverse links.
                let qr = self.pop()?;
                self.push(qr)?;
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
            function_id: 0, // TODO: track function IDs
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

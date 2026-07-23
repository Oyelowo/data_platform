//! End-to-end tests: compile bytecode to native code and check the results.
//!
//! These exercise the real Cranelift pipeline (declare → lower → define →
//! finalize → call) on the host architecture, so they double as a check that
//! the JIT ABI / relocation model is correct for the platform.

use yelang_jit::backend::JitBackend;
use yelang_vm::{CompiledFunction, Instruction, Value};

fn func(instructions: Vec<Instruction>, num_locals: u32, num_args: u32) -> CompiledFunction {
    CompiledFunction {
        name: None,
        instructions,
        num_locals,
        num_args,
    }
}

#[test]
fn native_constant_multiplication() {
    // fn() -> i64 { 6 * 7 }
    let f = func(
        vec![
            Instruction::PushConst(Value::Int(6)),
            Instruction::PushConst(Value::Int(7)),
            Instruction::Mul,
            Instruction::Return,
        ],
        0,
        0,
    );

    let mut jit = JitBackend::new().unwrap();
    assert!(jit.is_jittable(&f));
    assert_eq!(jit.execute(&f).unwrap(), Value::Int(42));
}

#[test]
fn native_arithmetic_and_bitwise() {
    // fn() -> i64 { ((20 - 8) / 3) + (0b1100 & 0b1010) - (-2) }
    //           = (12 / 3) + 8 + 2 = 4 + 8 + 2 = 14
    let f = func(
        vec![
            Instruction::PushConst(Value::Int(20)),
            Instruction::PushConst(Value::Int(8)),
            Instruction::Sub, // 12
            Instruction::PushConst(Value::Int(3)),
            Instruction::Div, // 4
            Instruction::PushConst(Value::Int(0b1100)),
            Instruction::PushConst(Value::Int(0b1010)),
            Instruction::BitAnd, // 8
            Instruction::Add,    // 12
            Instruction::PushConst(Value::Int(-2)),
            Instruction::Neg, // 2
            Instruction::Add, // 14
            Instruction::Return,
        ],
        0,
        0,
    );

    let mut jit = JitBackend::new().unwrap();
    assert_eq!(jit.execute(&f).unwrap(), Value::Int(14));
}

#[test]
fn native_comparison_returns_bool_as_int() {
    // fn() -> i64 { 10 > 3 }  => 1
    let gt = func(
        vec![
            Instruction::PushConst(Value::Int(10)),
            Instruction::PushConst(Value::Int(3)),
            Instruction::Gt,
            Instruction::Return,
        ],
        0,
        0,
    );
    // fn() -> i64 { 10 == 3 } => 0
    let eq = func(
        vec![
            Instruction::PushConst(Value::Int(10)),
            Instruction::PushConst(Value::Int(3)),
            Instruction::Eq,
            Instruction::Return,
        ],
        0,
        0,
    );

    let mut jit = JitBackend::new().unwrap();
    assert_eq!(jit.execute(&gt).unwrap(), Value::Int(1));
    assert_eq!(jit.execute(&eq).unwrap(), Value::Int(0));
}

#[test]
fn native_loop_sums_one_to_ten() {
    // fn() -> i64 { let mut sum = 0; let mut i = 1;
    //               while i <= 10 { sum += i; i += 1; } sum }
    // Exercises locals, a backward jump (loop), and conditional control flow.
    use Instruction::*;
    let f = func(
        vec![
            PushConst(Value::Int(0)), // pc 0
            StoreLocal(0),            // pc 1: sum = 0
            PushConst(Value::Int(1)), // pc 2
            StoreLocal(1),            // pc 3: i = 1
            LoadLocal(1),             // pc 4: loop head, push i
            PushConst(Value::Int(10)), // pc 5
            Le,                       // pc 6: i <= 10
            JumpIfNot(17),            // pc 7: exit if false
            LoadLocal(0),             // pc 8: sum
            LoadLocal(1),             // pc 9: i
            Add,                      // pc 10
            StoreLocal(0),            // pc 11: sum = sum + i
            LoadLocal(1),             // pc 12: i
            PushConst(Value::Int(1)), // pc 13
            Add,                      // pc 14: i + 1
            StoreLocal(1),            // pc 15: i = i + 1
            Jump(4),                  // pc 16: back to loop head
            LoadLocal(0),             // pc 17: push sum
            Return,                   // pc 18
        ],
        2,
        0,
    );

    let mut jit = JitBackend::new().unwrap();
    assert!(jit.is_jittable(&f));
    assert_eq!(jit.execute(&f).unwrap(), Value::Int(55));
}

#[test]
fn native_function_with_arguments() {
    // fn(a, b) -> i64 { a * a + b }
    use Instruction::*;
    let f = func(
        vec![
            LoadLocal(0), // a
            LoadLocal(0), // a
            Mul,          // a * a
            LoadLocal(1), // b
            Add,          // a*a + b
            Return,
        ],
        2,
        2,
    );

    let mut jit = JitBackend::new().unwrap();
    assert_eq!(jit.execute_with_args(&f, &[3, 4]).unwrap(), Value::Int(13));
    assert_eq!(jit.execute_with_args(&f, &[10, 5]).unwrap(), Value::Int(105));
}

#[test]
fn native_parameterised_loop() {
    // fn(n) -> i64 { sum of 1..=n }
    use Instruction::*;
    let f = func(
        vec![
            PushConst(Value::Int(0)), // pc 0
            StoreLocal(1),            // pc 1: sum = 0  (local 0 is the arg n)
            PushConst(Value::Int(1)), // pc 2
            StoreLocal(2),            // pc 3: i = 1
            LoadLocal(2),             // pc 4: loop head
            LoadLocal(0),             // pc 5: n
            Le,                       // pc 6: i <= n
            JumpIfNot(17),            // pc 7
            LoadLocal(1),             // pc 8
            LoadLocal(2),             // pc 9
            Add,                      // pc 10
            StoreLocal(1),            // pc 11
            LoadLocal(2),             // pc 12
            PushConst(Value::Int(1)), // pc 13
            Add,                      // pc 14
            StoreLocal(2),            // pc 15
            Jump(4),                  // pc 16
            LoadLocal(1),             // pc 17
            Return,                   // pc 18
        ],
        3,
        1,
    );

    let mut jit = JitBackend::new().unwrap();
    assert_eq!(jit.execute_with_args(&f, &[5]).unwrap(), Value::Int(15));
    assert_eq!(jit.execute_with_args(&f, &[100]).unwrap(), Value::Int(5050));
}

#[test]
fn unsupported_bytecode_falls_back_to_interpreter() {
    // Floats are outside the JIT subset, so execute() must transparently run
    // the interpreter and still produce the right answer.
    let f = func(
        vec![
            Instruction::PushConst(Value::Float(2.5)),
            Instruction::PushConst(Value::Float(1.5)),
            Instruction::Add,
            Instruction::Return,
        ],
        0,
        0,
    );

    let mut jit = JitBackend::new().unwrap();
    assert!(!jit.is_jittable(&f));
    assert_eq!(jit.execute(&f).unwrap(), Value::Float(4.0));
}

#[test]
fn compile_function_returns_callable_pointer() {
    let f = func(
        vec![
            Instruction::PushConst(Value::Int(21)),
            Instruction::PushConst(Value::Int(2)),
            Instruction::Mul,
            Instruction::Return,
        ],
        0,
        0,
    );

    let mut jit = JitBackend::new().unwrap();
    let ptr = jit.compile_function(&f).unwrap();
    assert!(!ptr.is_null());
    // The pointer is live for the backend's lifetime; call it directly.
    let result = unsafe {
        let native: extern "C" fn() -> i64 = std::mem::transmute(ptr);
        native()
    };
    assert_eq!(result, 42);
}

#[test]
fn flying_start_profiler_gates_compilation() {
    use yelang_jit::profiling::Profiler;

    let mut profiler = Profiler::with_threshold(3);
    let func_id = 7u64;

    // Cold: not yet a JIT candidate.
    profiler.record(func_id);
    profiler.record(func_id);
    assert!(!profiler.should_jit(func_id));

    // Third execution crosses the threshold.
    profiler.record(func_id);
    assert!(profiler.should_jit(func_id));
    assert_eq!(profiler.next_to_compile(), Some(func_id));

    // Once compiled, it is no longer queued.
    profiler.mark_jitted(func_id);
    assert!(!profiler.should_jit(func_id));
    assert!(profiler.is_jitted(func_id));
}

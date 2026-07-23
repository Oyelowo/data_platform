//! VM tests: arithmetic, control flow, functions, construction, queries.

use yelang_vm::{CompiledFunction, CompiledProgram, Instruction, Value, Vm};

fn run(instructions: Vec<Instruction>) -> Value {
    let func = CompiledFunction {
        name: None,
        instructions,
        num_locals: 10,
        num_args: 0,
    };
    let mut program = CompiledProgram::new();
    let id = program.add_function(func);
    program.entry = Some(id);
    let mut vm = Vm::new();
    vm.execute(&program).expect("execution failed")
}

// ===========================================================================
// Arithmetic
// ===========================================================================

#[test]
fn integer_addition() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(3)),
        Instruction::PushConst(Value::Int(4)),
        Instruction::Add,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Int(7));
}

#[test]
fn integer_subtraction() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(10)),
        Instruction::PushConst(Value::Int(3)),
        Instruction::Sub,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Int(7));
}

#[test]
fn integer_multiplication() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(6)),
        Instruction::PushConst(Value::Int(7)),
        Instruction::Mul,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Int(42));
}

#[test]
fn float_addition() {
    let result = run(vec![
        Instruction::PushConst(Value::Float(1.5)),
        Instruction::PushConst(Value::Float(2.5)),
        Instruction::Add,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Float(4.0));
}

#[test]
fn mixed_int_float() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(3)),
        Instruction::PushConst(Value::Float(1.5)),
        Instruction::Add,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Float(4.5));
}

#[test]
fn negation() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(5)),
        Instruction::Neg,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Int(-5));
}

#[test]
fn division_by_zero_errors() {
    let func = CompiledFunction {
        name: None,
        instructions: vec![
            Instruction::PushConst(Value::Int(1)),
            Instruction::PushConst(Value::Int(0)),
            Instruction::Div,
            Instruction::Halt,
        ],
        num_locals: 0,
        num_args: 0,
    };
    let mut program = CompiledProgram::new();
    let id = program.add_function(func);
    program.entry = Some(id);
    let mut vm = Vm::new();
    assert!(vm.execute(&program).is_err());
}

// ===========================================================================
// Comparison
// ===========================================================================

#[test]
fn equality() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(5)),
        Instruction::PushConst(Value::Int(5)),
        Instruction::Eq,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn inequality() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(5)),
        Instruction::PushConst(Value::Int(3)),
        Instruction::Ne,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn less_than() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(3)),
        Instruction::PushConst(Value::Int(5)),
        Instruction::Lt,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Bool(true));
}

// ===========================================================================
// Local variables
// ===========================================================================

#[test]
fn store_and_load_local() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(42)),
        Instruction::StoreLocal(0),
        Instruction::LoadLocal(0),
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Int(42));
}

#[test]
fn multiple_locals() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(10)),
        Instruction::StoreLocal(0),
        Instruction::PushConst(Value::Int(20)),
        Instruction::StoreLocal(1),
        Instruction::LoadLocal(0),
        Instruction::LoadLocal(1),
        Instruction::Add,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Int(30));
}

// ===========================================================================
// Control flow
// ===========================================================================

#[test]
fn unconditional_jump() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(1)),  // 0
        Instruction::Jump(3),                   // 1: skip to instruction 3
        Instruction::PushConst(Value::Int(2)),  // 2: skipped
        Instruction::Halt,                      // 3
    ]);
    assert_eq!(result, Value::Int(1));
}

#[test]
fn conditional_jump_taken() {
    let result = run(vec![
        Instruction::PushConst(Value::Bool(true)),  // 0
        Instruction::JumpIf(3),                      // 1: jump to 3 if true
        Instruction::PushConst(Value::Int(0)),       // 2: skipped
        Instruction::PushConst(Value::Int(1)),       // 3
        Instruction::Halt,                           // 4
    ]);
    assert_eq!(result, Value::Int(1));
}

#[test]
fn conditional_jump_not_taken() {
    let result = run(vec![
        Instruction::PushConst(Value::Bool(false)),  // 0
        Instruction::JumpIf(3),                       // 1: don't jump
        Instruction::PushConst(Value::Int(0)),        // 2: executed
        Instruction::Halt,                            // 3
    ]);
    assert_eq!(result, Value::Int(0));
}

// ===========================================================================
// Construction
// ===========================================================================

#[test]
fn make_array() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(1)),
        Instruction::PushConst(Value::Int(2)),
        Instruction::PushConst(Value::Int(3)),
        Instruction::MakeArray(3),
        Instruction::Halt,
    ]);
    assert_eq!(
        result,
        Value::Array(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
    );
}

#[test]
fn make_tuple() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(1)),
        Instruction::PushConst(Value::Bool(true)),
        Instruction::MakeTuple(2),
        Instruction::Halt,
    ]);
    assert_eq!(
        result,
        Value::Tuple(vec![Value::Int(1), Value::Bool(true)])
    );
}

#[test]
fn make_some() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(42)),
        Instruction::MakeSome,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Option(Some(Box::new(Value::Int(42)))));
}

#[test]
fn make_none() {
    let result = run(vec![
        Instruction::MakeNone,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Option(None));
}

// ===========================================================================
// Array operations
// ===========================================================================

#[test]
fn array_index() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(10)),
        Instruction::PushConst(Value::Int(20)),
        Instruction::PushConst(Value::Int(30)),
        Instruction::MakeArray(3),
        Instruction::PushConst(Value::Uint(1)),
        Instruction::Index,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Int(20));
}

#[test]
fn array_len() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(1)),
        Instruction::PushConst(Value::Int(2)),
        Instruction::PushConst(Value::Int(3)),
        Instruction::MakeArray(3),
        Instruction::Len,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Uint(3));
}

// ===========================================================================
// Iteration
// ===========================================================================

#[test]
fn iterator_basic() {
    let result = run(vec![
        // Create array [1, 2, 3]
        Instruction::PushConst(Value::Int(1)),
        Instruction::PushConst(Value::Int(2)),
        Instruction::PushConst(Value::Int(3)),
        Instruction::MakeArray(3),
        // Initialize iterator
        Instruction::IterInit,
        // Get first element
        Instruction::IterNext,
        // Stack: [iterator, value, has_next]
        // Pop has_next and value, keep iterator
        Instruction::Pop,  // pop has_next
        // value is on top
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Int(1));
}

// ===========================================================================
// Aggregate operations
// ===========================================================================

#[test]
fn aggregate_sum() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(1)),
        Instruction::PushConst(Value::Int(2)),
        Instruction::PushConst(Value::Int(3)),
        Instruction::MakeArray(3),
        Instruction::AggSum,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Int(6));
}

#[test]
fn aggregate_count() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(1)),
        Instruction::PushConst(Value::Int(2)),
        Instruction::PushConst(Value::Int(3)),
        Instruction::MakeArray(3),
        Instruction::AggCount,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Uint(3));
}

#[test]
fn aggregate_avg() {
    let result = run(vec![
        Instruction::PushConst(Value::Float(1.0)),
        Instruction::PushConst(Value::Float(2.0)),
        Instruction::PushConst(Value::Float(3.0)),
        Instruction::MakeArray(3),
        Instruction::AggAvg,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Float(2.0));
}

#[test]
fn aggregate_min() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(3)),
        Instruction::PushConst(Value::Int(1)),
        Instruction::PushConst(Value::Int(2)),
        Instruction::MakeArray(3),
        Instruction::AggMin,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Int(1));
}

#[test]
fn aggregate_max() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(3)),
        Instruction::PushConst(Value::Int(1)),
        Instruction::PushConst(Value::Int(2)),
        Instruction::MakeArray(3),
        Instruction::AggMax,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Int(3));
}

// ===========================================================================
// Bitwise operations
// ===========================================================================

#[test]
fn bitwise_and() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(0b1100)),
        Instruction::PushConst(Value::Int(0b1010)),
        Instruction::BitAnd,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Int(0b1000));
}

#[test]
fn bitwise_or() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(0b1100)),
        Instruction::PushConst(Value::Int(0b1010)),
        Instruction::BitOr,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Int(0b1110));
}

#[test]
fn bitwise_shift() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(1)),
        Instruction::PushConst(Value::Int(4)),
        Instruction::Shl,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Int(16));
}

// ===========================================================================
// Stack operations
// ===========================================================================

#[test]
fn dup_and_add() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(5)),
        Instruction::Dup,
        Instruction::Add,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Int(10));
}

#[test]
fn swap() {
    let result = run(vec![
        Instruction::PushConst(Value::Int(1)),
        Instruction::PushConst(Value::Int(2)),
        Instruction::Swap,
        // Stack: [2, 1], top is 1
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Int(1));
}

// ===========================================================================
// Boolean logic
// ===========================================================================

#[test]
fn not_true() {
    let result = run(vec![
        Instruction::PushConst(Value::Bool(true)),
        Instruction::Not,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Bool(false));
}

#[test]
fn not_false() {
    let result = run(vec![
        Instruction::PushConst(Value::Bool(false)),
        Instruction::Not,
        Instruction::Halt,
    ]);
    assert_eq!(result, Value::Bool(true));
}

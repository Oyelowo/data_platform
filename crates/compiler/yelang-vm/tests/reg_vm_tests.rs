//! Register VM tests: arithmetic, comparison, locals, control flow, arrays,
//! function calls, plus iteration, aggregates, fields, and query ops.

use yelang_interner::Interner;
use yelang_vm::reg_instruction::make_rk;
use yelang_vm::{
    InMemoryStorage, RegFunction, RegInstruction, RegProgram, RegVm, RegVmError, Value,
};

/// Run a single-function program (no args) and return the result.
fn run(instructions: Vec<RegInstruction>, constants: Vec<Value>, num_registers: u8) -> Value {
    let func = RegFunction {
        name: None,
        instructions,
        num_registers,
        num_args: 0,
        constants,
    };
    let mut program = RegProgram::new();
    let id = program.add_function(func);
    program.entry = Some(id);
    let mut vm = RegVm::new();
    vm.execute(&program).expect("execution failed")
}

// ===========================================================================
// Arithmetic
// ===========================================================================

#[test]
fn arithmetic_add() {
    let result = run(
        vec![
            RegInstruction::Add { a: 0, b: make_rk(0), c: make_rk(1) },
            RegInstruction::Return { a: 0 },
        ],
        vec![Value::Int(3), Value::Int(4)],
        2,
    );
    assert_eq!(result, Value::Int(7));
}

#[test]
fn arithmetic_sub() {
    let result = run(
        vec![
            RegInstruction::Sub { a: 0, b: make_rk(0), c: make_rk(1) },
            RegInstruction::Return { a: 0 },
        ],
        vec![Value::Int(10), Value::Int(3)],
        2,
    );
    assert_eq!(result, Value::Int(7));
}

#[test]
fn arithmetic_mul() {
    let result = run(
        vec![
            RegInstruction::Mul { a: 0, b: make_rk(0), c: make_rk(1) },
            RegInstruction::Return { a: 0 },
        ],
        vec![Value::Int(6), Value::Int(7)],
        2,
    );
    assert_eq!(result, Value::Int(42));
}

#[test]
fn arithmetic_div() {
    let result = run(
        vec![
            RegInstruction::Div { a: 0, b: make_rk(0), c: make_rk(1) },
            RegInstruction::Return { a: 0 },
        ],
        vec![Value::Int(20), Value::Int(4)],
        2,
    );
    assert_eq!(result, Value::Int(5));
}

#[test]
fn arithmetic_float_add() {
    let result = run(
        vec![
            RegInstruction::Add { a: 0, b: make_rk(0), c: make_rk(1) },
            RegInstruction::Return { a: 0 },
        ],
        vec![Value::Float(1.5), Value::Float(2.5)],
        2,
    );
    assert_eq!(result, Value::Float(4.0));
}

#[test]
fn arithmetic_mixed_register_and_constant() {
    // R0 = 10 (register), then R1 = R0 + K0(5) = 15.
    let result = run(
        vec![
            RegInstruction::LoadK { a: 0, bx: 1 },
            RegInstruction::Add { a: 1, b: 0, c: make_rk(0) },
            RegInstruction::Return { a: 1 },
        ],
        vec![Value::Int(5), Value::Int(10)],
        2,
    );
    assert_eq!(result, Value::Int(15));
}

#[test]
fn division_by_zero_errors() {
    let func = RegFunction {
        name: None,
        instructions: vec![
            RegInstruction::Div { a: 0, b: make_rk(0), c: make_rk(1) },
            RegInstruction::Return { a: 0 },
        ],
        num_registers: 2,
        num_args: 0,
        constants: vec![Value::Int(1), Value::Int(0)],
    };
    let mut program = RegProgram::new();
    let id = program.add_function(func);
    program.entry = Some(id);
    let mut vm = RegVm::new();
    let result = vm.execute(&program);
    assert!(matches!(result, Err(RegVmError::DivisionByZero)));
}

// ===========================================================================
// Comparison
// ===========================================================================

#[test]
fn comparison_lt_true() {
    let result = run(
        vec![
            RegInstruction::Lt { a: 0, b: make_rk(0), c: make_rk(1) },
            RegInstruction::Return { a: 0 },
        ],
        vec![Value::Int(3), Value::Int(5)],
        1,
    );
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn comparison_ge_false() {
    let result = run(
        vec![
            RegInstruction::Ge { a: 0, b: make_rk(0), c: make_rk(1) },
            RegInstruction::Return { a: 0 },
        ],
        vec![Value::Int(3), Value::Int(5)],
        1,
    );
    assert_eq!(result, Value::Bool(false));
}

#[test]
fn comparison_eq() {
    let result = run(
        vec![
            RegInstruction::Eq { a: 0, b: make_rk(0), c: make_rk(1) },
            RegInstruction::Return { a: 0 },
        ],
        vec![Value::Int(7), Value::Int(7)],
        1,
    );
    assert_eq!(result, Value::Bool(true));
}

// ===========================================================================
// Local variables (Move, LoadK)
// ===========================================================================

#[test]
fn local_variables_move_loadk() {
    let result = run(
        vec![
            RegInstruction::LoadK { a: 0, bx: 0 },    // R0 = 7
            RegInstruction::Move { a: 1, b: 0 },       // R1 = R0 = 7
            RegInstruction::Add { a: 2, b: 0, c: 1 },  // R2 = R0 + R1 = 14
            RegInstruction::Return { a: 2 },
        ],
        vec![Value::Int(7)],
        3,
    );
    assert_eq!(result, Value::Int(14));
}

#[test]
fn load_nil_and_bool() {
    let result = run(
        vec![
            RegInstruction::LoadNil { a: 0 },
            RegInstruction::LoadBool { a: 1, value: true },
            RegInstruction::Return { a: 1 },
        ],
        vec![],
        2,
    );
    assert_eq!(result, Value::Bool(true));
}

// ===========================================================================
// Control flow (Jump, JumpIf, JumpIfNot)
// ===========================================================================

#[test]
fn control_flow_jump_if_taken() {
    // 0: R0 = true
    // 1: JumpIfNot R0 -> 4 (else)
    // 2: R1 = K0 (1)    then-branch
    // 3: Jump -> 5
    // 4: R1 = K1 (99)   else-branch
    // 5: Return R1
    let result = run(
        vec![
            RegInstruction::LoadBool { a: 0, value: true },
            RegInstruction::JumpIfNot { a: 0, bx: 4 },
            RegInstruction::LoadK { a: 1, bx: 0 },
            RegInstruction::Jump { bx: 5 },
            RegInstruction::LoadK { a: 1, bx: 1 },
            RegInstruction::Return { a: 1 },
        ],
        vec![Value::Int(1), Value::Int(99)],
        2,
    );
    assert_eq!(result, Value::Int(1));
}

#[test]
fn control_flow_jump_if_not_taken() {
    let result = run(
        vec![
            RegInstruction::LoadBool { a: 0, value: false },
            RegInstruction::JumpIfNot { a: 0, bx: 4 },
            RegInstruction::LoadK { a: 1, bx: 0 },
            RegInstruction::Jump { bx: 5 },
            RegInstruction::LoadK { a: 1, bx: 1 },
            RegInstruction::Return { a: 1 },
        ],
        vec![Value::Int(1), Value::Int(99)],
        2,
    );
    assert_eq!(result, Value::Int(99));
}

#[test]
fn control_flow_unconditional_jump() {
    // Jump over a poisoned load.
    let result = run(
        vec![
            RegInstruction::Jump { bx: 2 },
            RegInstruction::LoadK { a: 0, bx: 1 }, // skipped (99)
            RegInstruction::LoadK { a: 0, bx: 0 }, // R0 = 5
            RegInstruction::Return { a: 0 },
        ],
        vec![Value::Int(5), Value::Int(99)],
        1,
    );
    assert_eq!(result, Value::Int(5));
}

// ===========================================================================
// Array construction and indexing
// ===========================================================================

#[test]
fn array_construction_and_indexing() {
    // R0=10, R1=20, R2=30, R3=[R0,R1,R2], R4=1, R5=R3[R4]=20.
    let result = run(
        vec![
            RegInstruction::LoadK { a: 0, bx: 0 },
            RegInstruction::LoadK { a: 1, bx: 1 },
            RegInstruction::LoadK { a: 2, bx: 2 },
            RegInstruction::MakeArray { a: 3, b: 0, count: 3 },
            RegInstruction::LoadK { a: 4, bx: 3 },
            RegInstruction::GetIndex { a: 5, b: 3, c: 4 },
            RegInstruction::Return { a: 5 },
        ],
        vec![Value::Int(10), Value::Int(20), Value::Int(30), Value::Int(1)],
        6,
    );
    assert_eq!(result, Value::Int(20));
}

#[test]
fn array_set_index_and_len() {
    // Build [1,2,3], set index 0 = 99, then read length.
    let result = run(
        vec![
            RegInstruction::LoadK { a: 0, bx: 0 },
            RegInstruction::LoadK { a: 1, bx: 1 },
            RegInstruction::LoadK { a: 2, bx: 2 },
            RegInstruction::MakeArray { a: 3, b: 0, count: 3 },
            RegInstruction::LoadK { a: 5, bx: 4 },   // R5 = 99 (new value)
            RegInstruction::LoadK { a: 4, bx: 3 },   // R4 = 0 (index)
            RegInstruction::SetIndex { a: 5, b: 3, c: 4 }, // R3[0] = 99
            RegInstruction::GetIndex { a: 6, b: 3, c: 4 }, // R6 = R3[0] = 99
            RegInstruction::Return { a: 6 },
        ],
        vec![Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(0), Value::Int(99)],
        7,
    );
    assert_eq!(result, Value::Int(99));
}

#[test]
fn array_len() {
    let result = run(
        vec![
            RegInstruction::LoadK { a: 0, bx: 0 },
            RegInstruction::LoadK { a: 1, bx: 1 },
            RegInstruction::MakeArray { a: 2, b: 0, count: 2 },
            RegInstruction::Len { a: 3, b: 2 },
            RegInstruction::Return { a: 3 },
        ],
        vec![Value::Int(1), Value::Int(2)],
        4,
    );
    assert_eq!(result, Value::Uint(2));
}

// ===========================================================================
// Function calls
// ===========================================================================

#[test]
fn function_call_add() {
    // Entry calls function 1 with args 5 and 7; function 1 returns their sum.
    let entry = RegFunction {
        name: Some("entry".into()),
        instructions: vec![
            RegInstruction::LoadK { a: 1, bx: 0 }, // R1 = FnPtr(1)
            RegInstruction::LoadK { a: 2, bx: 1 }, // R2 = 5
            RegInstruction::LoadK { a: 3, bx: 2 }, // R3 = 7
            RegInstruction::Call { a: 0, b: 1, num_args: 2 }, // R0 = f(R2, R3)
            RegInstruction::Return { a: 0 },
        ],
        num_registers: 4,
        num_args: 0,
        constants: vec![Value::FnPtr(1), Value::Int(5), Value::Int(7)],
    };
    let add_fn = RegFunction {
        name: Some("add".into()),
        instructions: vec![
            RegInstruction::Add { a: 2, b: 0, c: 1 }, // R2 = R0 + R1
            RegInstruction::Return { a: 2 },
        ],
        num_registers: 3,
        num_args: 2,
        constants: vec![],
    };

    let mut program = RegProgram::new();
    let entry_id = program.add_function(entry);
    let _add_id = program.add_function(add_fn);
    program.entry = Some(entry_id);

    let mut vm = RegVm::new();
    let result = vm.execute(&program).expect("execution failed");
    assert_eq!(result, Value::Int(12));
}

#[test]
fn nested_function_calls() {
    // Entry calls f(3); f(x) calls g(x) and adds 1; g(x) returns x * 2.
    // Result: (3 * 2) + 1 = 7.
    let entry = RegFunction {
        name: Some("entry".into()),
        instructions: vec![
            RegInstruction::LoadK { a: 1, bx: 0 }, // R1 = FnPtr(1) = f
            RegInstruction::LoadK { a: 2, bx: 1 }, // R2 = 3
            RegInstruction::Call { a: 0, b: 1, num_args: 1 }, // R0 = f(3)
            RegInstruction::Return { a: 0 },
        ],
        num_registers: 3,
        num_args: 0,
        constants: vec![Value::FnPtr(1), Value::Int(3)],
    };
    // f(x): R0 = x; call g(x) into R1; return R1 + 1.
    let f_fn = RegFunction {
        name: Some("f".into()),
        instructions: vec![
            RegInstruction::LoadK { a: 1, bx: 0 }, // R1 = FnPtr(2) = g
            // Move arg R0 into R2 so the call reads args from R2.. ; callee g
            // takes one arg at R(b+1).
            RegInstruction::Move { a: 2, b: 0 },   // R2 = x
            RegInstruction::Call { a: 3, b: 1, num_args: 1 }, // R3 = g(R2)
            RegInstruction::Add { a: 4, b: 3, c: make_rk(1) }, // R4 = R3 + 1
            RegInstruction::Return { a: 4 },
        ],
        num_registers: 5,
        num_args: 1,
        constants: vec![Value::FnPtr(2), Value::Int(1)],
    };
    // g(x): return x * 2.
    let g_fn = RegFunction {
        name: Some("g".into()),
        instructions: vec![
            RegInstruction::Mul { a: 1, b: 0, c: make_rk(0) }, // R1 = R0 * 2
            RegInstruction::Return { a: 1 },
        ],
        num_registers: 2,
        num_args: 1,
        constants: vec![Value::Int(2)],
    };

    let mut program = RegProgram::new();
    let entry_id = program.add_function(entry); // 0
    let _f = program.add_function(f_fn); // 1
    let _g = program.add_function(g_fn); // 2
    program.entry = Some(entry_id);

    let mut vm = RegVm::new();
    let result = vm.execute(&program).expect("execution failed");
    assert_eq!(result, Value::Int(7));
}

// ===========================================================================
// Bitwise
// ===========================================================================

#[test]
fn bitwise_and() {
    let result = run(
        vec![
            RegInstruction::BitAnd { a: 0, b: make_rk(0), c: make_rk(1) },
            RegInstruction::Return { a: 0 },
        ],
        vec![Value::Int(0b1100), Value::Int(0b1010)],
        1,
    );
    assert_eq!(result, Value::Int(0b1000));
}

// ===========================================================================
// Iteration
// ===========================================================================

#[test]
fn iteration_loop_sum() {
    // Sum [1,2,3] with an iterator loop.
    // 0: R0 = [1,2,3]
    // 1: R1 = iter(R0)
    // 2: R2 = 0 (accumulator)
    // 3: IterNext R3, R4 = next(R1)   (R3 = value, R4 = has_next)
    // 4: JumpIfNot R4 -> 7
    // 5: R2 = R2 + R3
    // 6: Jump -> 3
    // 7: Return R2
    let result = run(
        vec![
            RegInstruction::LoadK { a: 0, bx: 0 },
            RegInstruction::IterInit { a: 1, b: 0 },
            RegInstruction::LoadK { a: 2, bx: 1 },
            RegInstruction::IterNext { a: 3, b: 1 },
            RegInstruction::JumpIfNot { a: 4, bx: 7 },
            RegInstruction::Add { a: 2, b: 2, c: 3 },
            RegInstruction::Jump { bx: 3 },
            RegInstruction::Return { a: 2 },
        ],
        vec![
            Value::Array(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
            Value::Int(0),
        ],
        5,
    );
    assert_eq!(result, Value::Int(6));
}

// ===========================================================================
// Aggregates
// ===========================================================================

#[test]
fn aggregate_sum() {
    let result = run(
        vec![
            RegInstruction::LoadK { a: 0, bx: 0 },
            RegInstruction::AggSum { a: 1, b: 0 },
            RegInstruction::Return { a: 1 },
        ],
        vec![Value::Array(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(4),
        ])],
        2,
    );
    assert_eq!(result, Value::Int(10));
}

// ===========================================================================
// Field access
// ===========================================================================

#[test]
fn struct_make_and_get_field() {
    let interner = Interner::new();
    let name = interner.intern("age");
    // 0: R0 = Str("age") (field name)
    // 1: R1 = 42 (value)
    // 2: R2 = Struct(1, [(age, 42)])
    // 3: R3 = R2.age = 42
    let result = run(
        vec![
            RegInstruction::LoadK { a: 0, bx: 0 },
            RegInstruction::LoadK { a: 1, bx: 1 },
            RegInstruction::MakeStruct { a: 2, def_id: 1, b: 0, count: 1 },
            RegInstruction::GetField { a: 3, b: 2, field: 0 },
            RegInstruction::Return { a: 3 },
        ],
        vec![Value::Str(name), Value::Int(42)],
        4,
    );
    assert_eq!(result, Value::Int(42));
}

// ===========================================================================
// Query operations (scan + limit) against a storage backend
// ===========================================================================

#[test]
fn query_scan_and_limit() {
    let mut storage = InMemoryStorage::new();
    storage.insert_rows(
        1,
        vec![
            Value::Int(10),
            Value::Int(20),
            Value::Int(30),
            Value::Int(40),
            Value::Int(50),
        ],
    );

    let func = RegFunction {
        name: None,
        instructions: vec![
            RegInstruction::QueryScan { a: 0, table_id: 1 },
            RegInstruction::LoadK { a: 1, bx: 0 }, // skip = 1
            RegInstruction::LoadK { a: 2, bx: 1 }, // fetch = 2
            RegInstruction::QueryLimit { a: 3, b: 0, c: 1 },
            RegInstruction::Len { a: 4, b: 3 },
            RegInstruction::Return { a: 4 },
        ],
        num_registers: 5,
        num_args: 0,
        constants: vec![Value::Int(1), Value::Int(2)],
    };
    let mut program = RegProgram::new();
    let id = program.add_function(func);
    program.entry = Some(id);

    let mut vm = RegVm::with_storage(Box::new(storage));
    let result = vm.execute(&program).expect("execution failed");
    assert_eq!(result, Value::Uint(2));
}

// ===========================================================================
// Misc
// ===========================================================================

#[test]
fn halt_returns_register_zero() {
    let result = run(
        vec![
            RegInstruction::LoadK { a: 0, bx: 0 },
            RegInstruction::Halt,
        ],
        vec![Value::Int(5)],
        1,
    );
    assert_eq!(result, Value::Int(5));
}

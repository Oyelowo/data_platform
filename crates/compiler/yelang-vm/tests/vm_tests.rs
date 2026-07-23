//! VM tests: arithmetic, control flow, functions, construction, queries.

use yelang_interner::Interner;
use yelang_vm::{
    CompiledFunction, CompiledProgram, InMemoryStorage, Instruction, StorageBackend,
    TraverseDirection, TraverseSpec, Value, Vm, WindowAgg, WindowFunc,
};

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

/// Run bytecode against a VM backed by the given storage backend.
fn run_with_storage(instructions: Vec<Instruction>, storage: Box<dyn StorageBackend>) -> Value {
    let func = CompiledFunction {
        name: None,
        instructions,
        num_locals: 10,
        num_args: 0,
    };
    let mut program = CompiledProgram::new();
    let id = program.add_function(func);
    program.entry = Some(id);
    let mut vm = Vm::with_storage(storage);
    vm.execute(&program).expect("execution failed")
}

/// Unwrap a QueryResult, panicking with a helpful message otherwise.
fn into_rows(value: Value) -> Vec<Value> {
    match value {
        Value::QueryResult(rows) => rows,
        other => panic!("expected QueryResult, got {}", other),
    }
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

// ===========================================================================
// Storage-backed query scans (Task 1)
// ===========================================================================

#[test]
fn query_scan_reads_from_storage() {
    let interner = Interner::new();
    let id = interner.intern("id");
    let name = interner.intern("name");

    let rows = vec![
        Value::Struct(1, vec![(id, Value::Int(1)), (name, Value::Int(10))]),
        Value::Struct(1, vec![(id, Value::Int(2)), (name, Value::Int(20))]),
    ];

    let mut storage = InMemoryStorage::new();
    storage.insert_table(42, vec![id, name], rows.clone());

    let result = run_with_storage(
        vec![Instruction::QueryScan(42), Instruction::Halt],
        Box::new(storage),
    );
    assert_eq!(result, Value::QueryResult(rows));
}

#[test]
fn query_scan_unknown_table_is_empty() {
    let storage = InMemoryStorage::new();
    let result = run_with_storage(
        vec![Instruction::QueryScan(999), Instruction::Halt],
        Box::new(storage),
    );
    assert_eq!(result, Value::QueryResult(vec![]));
}

#[test]
fn query_scan_default_vm_has_empty_storage() {
    // Vm::new() uses EmptyStorage, so any scan yields no rows.
    let result = run(vec![Instruction::QueryScan(1), Instruction::Halt]);
    assert_eq!(result, Value::QueryResult(vec![]));
}

#[test]
fn in_memory_storage_reports_columns() {
    let interner = Interner::new();
    let a = interner.intern("a");
    let b = interner.intern("b");
    let mut storage = InMemoryStorage::new();
    storage.insert_table(7, vec![a, b], vec![]);
    assert_eq!(storage.table_columns(7), vec![a, b]);
    assert!(storage.table_columns(8).is_empty());
}

// ===========================================================================
// Window functions (Task 2)
// ===========================================================================

/// Build the three-row test table used by the window tests:
/// partition `dept` (1, 1, 2) ordered by `salary`.
fn window_rows(interner: &Interner) -> (Vec<Value>, yelang_interner::Symbol, yelang_interner::Symbol) {
    let dept = interner.intern("dept");
    let salary = interner.intern("salary");
    let rows = vec![
        Value::Struct(1, vec![(dept, Value::Int(1)), (salary, Value::Int(300))]),
        Value::Struct(1, vec![(dept, Value::Int(1)), (salary, Value::Int(100))]),
        Value::Struct(1, vec![(dept, Value::Int(2)), (salary, Value::Int(50))]),
    ];
    (rows, dept, salary)
}

#[test]
fn window_row_number() {
    let interner = Interner::new();
    let (rows, dept, salary) = window_rows(&interner);
    let rn = interner.intern("rn");

    let result = run(vec![
        Instruction::PushConst(Value::QueryResult(rows)),
        Instruction::Window {
            partition_by: vec![dept],
            order_by: vec![(salary, true)],
            func: WindowFunc::RowNumber,
            output: rn,
        },
        Instruction::Halt,
    ]);

    let out = into_rows(result);
    assert_eq!(out.len(), 3);
    // Input order preserved. Within dept 1, salary 300 sorts after 100 → rn 2;
    // salary 100 → rn 1. Dept 2 is alone → rn 1.
    assert_eq!(out[0].get_field(rn), Some(&Value::Uint(2)));
    assert_eq!(out[1].get_field(rn), Some(&Value::Uint(1)));
    assert_eq!(out[2].get_field(rn), Some(&Value::Uint(1)));
}

#[test]
fn window_rank_with_ties() {
    let interner = Interner::new();
    let dept = interner.intern("dept");
    let salary = interner.intern("salary");
    let rank = interner.intern("rank");
    // Salaries 100, 100, 200 in one partition → ranks 1, 1, 3 (gap).
    let rows = vec![
        Value::Struct(1, vec![(dept, Value::Int(1)), (salary, Value::Int(100))]),
        Value::Struct(1, vec![(dept, Value::Int(1)), (salary, Value::Int(100))]),
        Value::Struct(1, vec![(dept, Value::Int(1)), (salary, Value::Int(200))]),
    ];

    let result = run(vec![
        Instruction::PushConst(Value::QueryResult(rows)),
        Instruction::Window {
            partition_by: vec![dept],
            order_by: vec![(salary, true)],
            func: WindowFunc::Rank,
            output: rank,
        },
        Instruction::Halt,
    ]);

    let out = into_rows(result);
    assert_eq!(out[0].get_field(rank), Some(&Value::Uint(1)));
    assert_eq!(out[1].get_field(rank), Some(&Value::Uint(1)));
    assert_eq!(out[2].get_field(rank), Some(&Value::Uint(3)));
}

#[test]
fn window_dense_rank_no_gaps() {
    let interner = Interner::new();
    let dept = interner.intern("dept");
    let salary = interner.intern("salary");
    let dr = interner.intern("dr");
    let rows = vec![
        Value::Struct(1, vec![(dept, Value::Int(1)), (salary, Value::Int(100))]),
        Value::Struct(1, vec![(dept, Value::Int(1)), (salary, Value::Int(100))]),
        Value::Struct(1, vec![(dept, Value::Int(1)), (salary, Value::Int(200))]),
    ];

    let result = run(vec![
        Instruction::PushConst(Value::QueryResult(rows)),
        Instruction::Window {
            partition_by: vec![dept],
            order_by: vec![(salary, true)],
            func: WindowFunc::DenseRank,
            output: dr,
        },
        Instruction::Halt,
    ]);

    let out = into_rows(result);
    assert_eq!(out[0].get_field(dr), Some(&Value::Uint(1)));
    assert_eq!(out[1].get_field(dr), Some(&Value::Uint(1)));
    assert_eq!(out[2].get_field(dr), Some(&Value::Uint(2)));
}

#[test]
fn window_lag_previous_row() {
    let interner = Interner::new();
    let dept = interner.intern("dept");
    let salary = interner.intern("salary");
    let prev = interner.intern("prev");
    // Already in ascending salary order within the single partition.
    let rows = vec![
        Value::Struct(1, vec![(dept, Value::Int(1)), (salary, Value::Int(100))]),
        Value::Struct(1, vec![(dept, Value::Int(1)), (salary, Value::Int(200))]),
        Value::Struct(1, vec![(dept, Value::Int(1)), (salary, Value::Int(300))]),
    ];

    let result = run(vec![
        Instruction::PushConst(Value::QueryResult(rows)),
        Instruction::Window {
            partition_by: vec![dept],
            order_by: vec![(salary, true)],
            func: WindowFunc::Lag(salary, 1),
            output: prev,
        },
        Instruction::Halt,
    ]);

    let out = into_rows(result);
    assert_eq!(out[0].get_field(prev), Some(&Value::Null));
    assert_eq!(out[1].get_field(prev), Some(&Value::Int(100)));
    assert_eq!(out[2].get_field(prev), Some(&Value::Int(200)));
}

#[test]
fn window_lead_next_row() {
    let interner = Interner::new();
    let dept = interner.intern("dept");
    let salary = interner.intern("salary");
    let nxt = interner.intern("nxt");
    let rows = vec![
        Value::Struct(1, vec![(dept, Value::Int(1)), (salary, Value::Int(100))]),
        Value::Struct(1, vec![(dept, Value::Int(1)), (salary, Value::Int(200))]),
        Value::Struct(1, vec![(dept, Value::Int(1)), (salary, Value::Int(300))]),
    ];

    let result = run(vec![
        Instruction::PushConst(Value::QueryResult(rows)),
        Instruction::Window {
            partition_by: vec![dept],
            order_by: vec![(salary, true)],
            func: WindowFunc::Lead(salary, 1),
            output: nxt,
        },
        Instruction::Halt,
    ]);

    let out = into_rows(result);
    assert_eq!(out[0].get_field(nxt), Some(&Value::Int(200)));
    assert_eq!(out[1].get_field(nxt), Some(&Value::Int(300)));
    assert_eq!(out[2].get_field(nxt), Some(&Value::Null));
}

#[test]
fn window_aggregate_sum_over_partition() {
    let interner = Interner::new();
    let (rows, dept, salary) = window_rows(&interner);
    let total = interner.intern("total");

    let result = run(vec![
        Instruction::PushConst(Value::QueryResult(rows)),
        Instruction::Window {
            partition_by: vec![dept],
            order_by: vec![(salary, true)],
            func: WindowFunc::Aggregate(WindowAgg::Sum, salary),
            output: total,
        },
        Instruction::Halt,
    ]);

    let out = into_rows(result);
    // Dept 1 sums to 400 (300 + 100); dept 2 sums to 50.
    assert_eq!(out[0].get_field(total), Some(&Value::Int(400)));
    assert_eq!(out[1].get_field(total), Some(&Value::Int(400)));
    assert_eq!(out[2].get_field(total), Some(&Value::Int(50)));
}

#[test]
fn window_aggregate_count_over_partition() {
    let interner = Interner::new();
    let (rows, dept, salary) = window_rows(&interner);
    let cnt = interner.intern("cnt");

    let result = run(vec![
        Instruction::PushConst(Value::QueryResult(rows)),
        Instruction::Window {
            partition_by: vec![dept],
            order_by: vec![(salary, true)],
            func: WindowFunc::Aggregate(WindowAgg::Count, salary),
            output: cnt,
        },
        Instruction::Halt,
    ]);

    let out = into_rows(result);
    assert_eq!(out[0].get_field(cnt), Some(&Value::Uint(2)));
    assert_eq!(out[1].get_field(cnt), Some(&Value::Uint(2)));
    assert_eq!(out[2].get_field(cnt), Some(&Value::Uint(1)));
}

// ===========================================================================
// Link traversal (Task 3)
// ===========================================================================

/// Build storage with a `writes` edge table (id 10) and a `books` target
/// table (id 20), returning the interned column symbols.
fn traverse_storage(
    interner: &Interner,
) -> (
    InMemoryStorage,
    yelang_interner::Symbol,
    yelang_interner::Symbol,
    yelang_interner::Symbol,
    yelang_interner::Symbol,
) {
    let id = interner.intern("id");
    let from = interner.intern("_from");
    let to = interner.intern("_to");
    let title = interner.intern("title");

    let mut storage = InMemoryStorage::new();
    // Edge table: author 1 wrote books 101 & 102; author 2 wrote book 103.
    storage.insert_rows(
        10,
        vec![
            Value::Struct(2, vec![(from, Value::Int(1)), (to, Value::Int(101))]),
            Value::Struct(2, vec![(from, Value::Int(1)), (to, Value::Int(102))]),
            Value::Struct(2, vec![(from, Value::Int(2)), (to, Value::Int(103))]),
        ],
    );
    // Target table: books keyed by id.
    storage.insert_rows(
        20,
        vec![
            Value::Struct(3, vec![(id, Value::Int(101)), (title, Value::Int(1))]),
            Value::Struct(3, vec![(id, Value::Int(102)), (title, Value::Int(2))]),
            Value::Struct(3, vec![(id, Value::Int(103)), (title, Value::Int(3))]),
        ],
    );

    (storage, id, from, to, title)
}

#[test]
fn traverse_out_produces_nested_arrays() {
    let interner = Interner::new();
    let (storage, id, from, to, _title) = traverse_storage(&interner);
    let books = interner.intern("books");

    let authors = vec![
        Value::Struct(1, vec![(id, Value::Int(1))]),
        Value::Struct(1, vec![(id, Value::Int(2))]),
    ];

    let spec = TraverseSpec {
        edge_table: 10,
        source_column: from,
        target_column: to,
        target_table: 20,
        direction: TraverseDirection::Out,
        source_key: id,
        target_key: id,
        output: books,
    };

    let result = run_with_storage(
        vec![
            Instruction::PushConst(Value::QueryResult(authors)),
            Instruction::QueryTraverse(spec),
            Instruction::Halt,
        ],
        Box::new(storage),
    );

    let out = into_rows(result);
    assert_eq!(out.len(), 2);

    // Author 1 → books 101 & 102.
    let b0 = out[0].get_field(books).expect("missing books field");
    assert_eq!(b0.len(), Some(2));
    if let Value::Array(matches) = b0 {
        assert_eq!(matches[0].get_field(id), Some(&Value::Int(101)));
        assert_eq!(matches[1].get_field(id), Some(&Value::Int(102)));
    } else {
        panic!("expected nested array");
    }

    // Author 2 → book 103.
    let b1 = out[1].get_field(books).expect("missing books field");
    assert_eq!(b1.len(), Some(1));
}

#[test]
fn traverse_in_follows_reverse_links() {
    let interner = Interner::new();
    let id = interner.intern("id");
    let from = interner.intern("_from");
    let to = interner.intern("_to");
    let authors_field = interner.intern("authors");

    let mut storage = InMemoryStorage::new();
    storage.insert_rows(
        10,
        vec![
            Value::Struct(2, vec![(from, Value::Int(1)), (to, Value::Int(101))]),
            Value::Struct(2, vec![(from, Value::Int(2)), (to, Value::Int(101))]),
        ],
    );
    // Author (target) table id 30.
    storage.insert_rows(
        30,
        vec![
            Value::Struct(4, vec![(id, Value::Int(1))]),
            Value::Struct(4, vec![(id, Value::Int(2))]),
        ],
    );

    // Start from book 101, traverse incoming edges back to authors.
    let books = vec![Value::Struct(3, vec![(id, Value::Int(101))])];

    let spec = TraverseSpec {
        edge_table: 10,
        source_column: from,
        target_column: to,
        target_table: 30,
        direction: TraverseDirection::In,
        source_key: id,
        target_key: id,
        output: authors_field,
    };

    let result = run_with_storage(
        vec![
            Instruction::PushConst(Value::QueryResult(books)),
            Instruction::QueryTraverse(spec),
            Instruction::Halt,
        ],
        Box::new(storage),
    );

    let out = into_rows(result);
    assert_eq!(out.len(), 1);
    let matched = out[0].get_field(authors_field).expect("missing authors field");
    // Book 101 has two incoming edges → authors 1 and 2.
    assert_eq!(matched.len(), Some(2));
}

#[test]
fn traverse_no_matches_yields_empty_array() {
    let interner = Interner::new();
    let (storage, id, from, to, _title) = traverse_storage(&interner);
    let books = interner.intern("books");

    // Author 999 wrote nothing.
    let authors = vec![Value::Struct(1, vec![(id, Value::Int(999))])];

    let spec = TraverseSpec {
        edge_table: 10,
        source_column: from,
        target_column: to,
        target_table: 20,
        direction: TraverseDirection::Out,
        source_key: id,
        target_key: id,
        output: books,
    };

    let result = run_with_storage(
        vec![
            Instruction::PushConst(Value::QueryResult(authors)),
            Instruction::QueryTraverse(spec),
            Instruction::Halt,
        ],
        Box::new(storage),
    );

    let out = into_rows(result);
    assert_eq!(out.len(), 1);
    let matched = out[0].get_field(books).expect("missing books field");
    assert_eq!(matched.len(), Some(0));
}

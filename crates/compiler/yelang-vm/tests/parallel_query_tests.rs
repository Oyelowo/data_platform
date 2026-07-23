//! Parallel query tests: morsel-parallel filter, aggregate, empty input, and
//! the register-VM pipeline connection.

use yelang_vm::{
    execute_aggregate_parallel, execute_query_parallel, execute_query_parallel_with_morsel_size,
    execute_reg_vm_parallel, RegFunction, RegInstruction, RegProgram, Value,
};

/// Sum a slice of integer values.
fn sum_ints(tuples: &[Value]) -> i128 {
    tuples.iter().filter_map(|v| v.as_int()).sum()
}

// ===========================================================================
// Parallel filter
// ===========================================================================

#[test]
fn parallel_filter() {
    // 100 tuples split into 10-tuple morsels; keep the even ones.
    let tuples: Vec<Value> = (1..=100).map(Value::Int).collect();
    let result = execute_query_parallel_with_morsel_size(
        tuples,
        |morsel| {
            morsel
                .iter()
                .filter(|v| v.as_int().map_or(false, |i| i % 2 == 0))
                .cloned()
                .collect()
        },
        4,
        10,
    );

    // Morsel completion order is nondeterministic, so compare as a sorted set.
    let mut got: Vec<i128> = result.iter().filter_map(|v| v.as_int()).collect();
    got.sort_unstable();
    let expected: Vec<i128> = (1..=100).filter(|i| i % 2 == 0).collect();
    assert_eq!(got, expected);
}

#[test]
fn parallel_map_concatenates_all() {
    // Identity pipeline preserves every tuple (count-wise).
    let tuples: Vec<Value> = (1..=57).map(Value::Int).collect();
    let result = execute_query_parallel(tuples, |morsel| morsel.to_vec(), 4);
    assert_eq!(result.len(), 57);
    assert_eq!(sum_ints(&result), (1..=57).sum::<i128>());
}

// ===========================================================================
// Parallel aggregate (partial sum per morsel, merge)
// ===========================================================================

#[test]
fn parallel_aggregate_sum() {
    let tuples: Vec<Value> = (1..=100).map(Value::Int).collect();
    let total: i128 = execute_aggregate_parallel(
        tuples,
        |morsel| sum_ints(morsel), // partial sum per morsel
        |a, b| a + b,              // merge partials
        4,
    );
    assert_eq!(total, 5050);
}

#[test]
fn parallel_aggregate_count() {
    let tuples: Vec<Value> = (0..250).map(Value::Int).collect();
    let count: usize = execute_aggregate_parallel(
        tuples,
        |morsel| morsel.len(),
        |a, b| a + b,
        8,
    );
    assert_eq!(count, 250);
}

// ===========================================================================
// Empty input
// ===========================================================================

#[test]
fn parallel_empty_query() {
    let result = execute_query_parallel(Vec::new(), |morsel| morsel.to_vec(), 4);
    assert!(result.is_empty());
}

#[test]
fn parallel_empty_aggregate() {
    let total: i128 = execute_aggregate_parallel(
        Vec::new(),
        |morsel| sum_ints(morsel),
        |a, b| a + b,
        4,
    );
    assert_eq!(total, 0);
}

// ===========================================================================
// Connection to the register VM
// ===========================================================================

#[test]
fn parallel_reg_vm_pipeline() {
    // Each morsel is summed by a freshly built register-VM program (AggSum),
    // returning one partial sum per morsel; the partials sum to the total.
    // 25_000 tuples at the default 10K morsel size → 3 morsels.
    let tuples: Vec<Value> = (1..=25_000).map(Value::Int).collect();
    let partials = execute_reg_vm_parallel(
        tuples,
        |_morsel| {
            let func = RegFunction {
                name: None,
                instructions: vec![
                    // R0 = morsel array (passed as the entry arg).
                    RegInstruction::AggSum { a: 1, b: 0 },
                    RegInstruction::Return { a: 1 },
                ],
                num_registers: 2,
                num_args: 1,
                constants: vec![],
            };
            let mut program = RegProgram::new();
            let id = program.add_function(func);
            program.entry = Some(id);
            program
        },
        4,
    );

    let total: i128 = partials.iter().filter_map(|v| v.as_int()).sum();
    assert_eq!(total, (1..=25_000).sum::<i128>());
    // At least one partial per morsel was produced.
    assert!(!partials.is_empty());
}

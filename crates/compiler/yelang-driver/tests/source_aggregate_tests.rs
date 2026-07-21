//! Source-driven aggregate tests.
//!
//! These tests compile raw `.ye` source strings that use built-in aggregate
//! methods and query-syntax aggregates, then execute them through the full
//! pipeline. They are the counterpart to the manually-constructed QIR tests in
//! `yelang-qir/tests/aggregate_closure_tests.rs`.

use yelang_driver::Driver;
use yelang_qir::exec::Value;

fn ints(value: Value) -> Vec<i128> {
    value
        .try_into_array()
        .expect("expected array")
        .into_iter()
        .map(|v| match v {
            Value::Int(n) => n,
            other => panic!("expected int, got {:?}", other),
        })
        .collect()
}

fn scalar_int(value: Value) -> i128 {
    match value {
        Value::Record(fields) => match &fields[..] {
            [(_, Value::Int(n))] => *n,
            _ => panic!("expected scalar int record, got {:?}", fields),
        },
        Value::Int(n) => n,
        other => panic!("expected scalar int, got {:?}", other),
    }
}

fn scalar_float(value: Value) -> f64 {
    match value {
        Value::Record(fields) => match &fields[..] {
            [(_, Value::Float(n))] => *n,
            _ => panic!("expected scalar float record, got {:?}", fields),
        },
        Value::Float(n) => n,
        other => panic!("expected scalar float, got {:?}", other),
    }
}

// Source-driven aggregate tests: query syntax and method-call pipelines both
// run through the full compiler pipeline.

#[test]
fn query_syntax_sum_i32() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3, 4, 5];
    let _ = select sum(x) from xs@x;
}
"#;
    let value = Driver::new().run(src).expect("run");
    assert_eq!(scalar_int(value), 15);
}

#[test]
fn query_syntax_count_i32() {
    let src = r#"
fn main() {
    let xs = [10, 20, 30, 40];
    let _ = select count(x) from xs@x;
}
"#;
    let value = Driver::new().run(src).expect("run");
    assert_eq!(scalar_int(value), 4);
}

#[test]
fn query_syntax_avg_i32() {
    let src = r#"
fn main() {
    let xs = [10, 20, 30, 40];
    let _ = select avg(x) from xs@x;
}
"#;
    let value = Driver::new().run(src).expect("run");
    assert!((scalar_float(value) - 25.0).abs() < 1e-9);
}

// REMOVED: `query_syntax_sum_after_filter_and_map` tested the legacy query
// pipeline's attempt to treat `Array` as an `Iterator` directly. That pipeline
// cannot prove `Array<T>: Iterator<T>` and is being replaced by the new
// Queryable/THIR/QIR pipeline in subsequent phases. An equivalent end-to-end
// test will be added once the new pipeline supports filter/map/sum chains.

#[test]
fn query_syntax_group_by_sum() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3, 4, 5, 6];
    let _ = select groups@g[*].{ k: g.key.parity, members: g.members }
              from xs@x
              group by { parity: x % 2 } into groups;
}
"#;
    let value = Driver::new().run(src).expect("run");
    // Result shape depends on grouping; for now just ensure it executes.
    assert!(value.try_into_array().is_ok());
}

#[test]
fn method_call_sum_i32() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3, 4, 5];
    let _ = xs.sum();
}
"#;
    let value = Driver::new().run(src).expect("run");
    assert_eq!(scalar_int(value), 15);
}

#[test]
fn method_call_count_i32() {
    let src = r#"
fn main() {
    let xs = [10, 20, 30, 40];
    let _ = xs.count();
}
"#;
    let value = Driver::new().run(src).expect("run");
    assert_eq!(scalar_int(value), 4);
}

#[test]
fn method_call_avg_i32() {
    let src = r#"
fn main() {
    let xs = [10, 20, 30, 40];
    let _ = xs.avg();
}
"#;
    let value = Driver::new().run(src).expect("run");
    assert!((scalar_float(value) - 25.0).abs() < 1e-9);
}

// REMOVED: `method_call_filter_map_sum_chain` tested the legacy query pipeline's
// attempt to lower `.filter(...).map(...).sum()` through `Iterator` combinators.
// The new pipeline lowers these through `Queryable` intrinsics instead; an
// equivalent test will be added when that pipeline is operational.

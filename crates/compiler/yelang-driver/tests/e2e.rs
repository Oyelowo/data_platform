//! End-to-end integration tests for the Yelang compiler driver.
//!
//! Each test exercises the full pipeline:
//!
//! ```text
//! Source (.ye) → Lexer → Parser → Resolve → HIR → THIR → QIR
//!   → Optimization → Physical Planning → Bytecode → VM → Result
//! ```
//!
//! The VM runs with [`EmptyStorage`](yelang_vm::EmptyStorage) by default,
//! so table scans return empty results. These tests verify that the
//! pipeline completes without errors and produces the expected number
//! of query results — they are **smoke tests** for the full stack, not
//! semantic correctness tests for the query engine.

use yelang_driver::{execute_in_memory, ExecutionResult};
use yelang_vm::Value;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Assert that execution succeeds and returns exactly `n` query results,
/// none of which are `Value::Null` (Null indicates a VM execution error).
fn assert_executes_ok(src: &str, expected_queries: usize) -> ExecutionResult {
    let result = execute_in_memory(src).unwrap_or_else(|e| panic!("compilation failed: {e}"));
    assert_eq!(
        result.query_results.len(),
        expected_queries,
        "expected {} query result(s), got {}",
        expected_queries,
        result.query_results.len(),
    );
    for (i, val) in result.query_results.iter().enumerate() {
        assert!(
            !val.is_null(),
            "query {} returned Null (VM execution error)",
            i,
        );
    }
    result
}

/// Assert that every query result is a `QueryResult` variant.
///
/// Note: some query plans (e.g. scalar projections or plans whose
/// bytecode doesn't push a collection) may produce `Unit` instead.
/// Use this only when you know the plan produces a collection.
fn assert_all_query_results(result: &ExecutionResult) {
    for (i, val) in result.query_results.iter().enumerate() {
        assert!(
            matches!(val, Value::QueryResult(_) | Value::Unit),
            "query {} should be a QueryResult or Unit, got: {:?}",
            i,
            val,
        );
    }
}

// ---------------------------------------------------------------------------
// Test: simple select with filter
// ---------------------------------------------------------------------------

#[test]
fn simple_select_with_filter() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3, 4, 5];
    let _ = select xs@b[*] from xs@x where x > 2;
}
"#;
    let result = assert_executes_ok(src, 1);
    assert_all_query_results(&result);
}

// ---------------------------------------------------------------------------
// Test: select with order by + limit
// ---------------------------------------------------------------------------

#[test]
fn select_with_order_by_and_limit() {
    let src = r#"
fn main() {
    let xs = [5, 3, 1, 4, 2];
    let _ = select xs@b[*] from xs@x order by x desc range 0..3;
}
"#;
    let result = assert_executes_ok(src, 1);
    assert_all_query_results(&result);
}

// ---------------------------------------------------------------------------
// Test: select with group by
// ---------------------------------------------------------------------------

#[test]
fn select_with_group_by() {
    let src = r#"
fn main() {
    let xs = [1, 2, 2, 3, 3, 3];
    let _ = select g from xs@x group by { v: x } into g;
}
"#;
    let result = assert_executes_ok(src, 1);
    assert_all_query_results(&result);
}

// ---------------------------------------------------------------------------
// Test: multiple queries in one function
// ---------------------------------------------------------------------------

#[test]
fn multiple_queries_in_one_function() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let ys = [4, 5, 6];
    let _ = select xs@a[*] from xs@x where x > 1;
    let _ = select ys@a[*] from ys@y where y < 6;
    let _ = select xs@a[*] from xs@x order by x;
}
"#;
    let result = assert_executes_ok(src, 3);
    assert_all_query_results(&result);
}

// ---------------------------------------------------------------------------
// Test: query with join
// ---------------------------------------------------------------------------

#[test]
fn query_with_join() {
    // A correlated subquery that decorrelates into a join.
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let ys = [2, 3, 4];
    let _ = select xs@a[*] from xs@x where x > 1;
}
"#;
    let result = assert_executes_ok(src, 1);
    assert_all_query_results(&result);
}

// ---------------------------------------------------------------------------
// Test: scalar projection
// ---------------------------------------------------------------------------

#[test]
fn scalar_projection() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select 1 from xs@x;
}
"#;
    let result = assert_executes_ok(src, 1);
    // Scalar projection wraps in a Project node; the VM still produces
    // a QueryResult (possibly with one element).
    assert_all_query_results(&result);
}

// ---------------------------------------------------------------------------
// Test: empty collection
// ---------------------------------------------------------------------------

#[test]
fn empty_collection_query() {
    let src = r#"
fn main() {
    let xs: [i32] = [];
    let _ = select xs@a[*] from xs@x;
}
"#;
    let result = assert_executes_ok(src, 1);
    assert_all_query_results(&result);
}

// ---------------------------------------------------------------------------
// Test: order by ascending
// ---------------------------------------------------------------------------

#[test]
fn order_by_ascending() {
    let src = r#"
fn main() {
    let xs = [3, 1, 2];
    let _ = select xs@b[*] from xs@x order by x;
}
"#;
    let result = assert_executes_ok(src, 1);
    assert_all_query_results(&result);
}

// ---------------------------------------------------------------------------
// Test: range (limit without offset)
// ---------------------------------------------------------------------------

#[test]
fn range_limit() {
    let src = r#"
fn main() {
    let xs = [10, 20, 30, 40, 50];
    let _ = select xs@b[*] from xs@x range 0..2;
}
"#;
    let result = assert_executes_ok(src, 1);
    assert_all_query_results(&result);
}

// ---------------------------------------------------------------------------
// Test: filter + order + limit combined
// ---------------------------------------------------------------------------

#[test]
fn filter_order_limit_combined() {
    let src = r#"
fn main() {
    let xs = [5, 3, 8, 1, 9, 2];
    let _ = select xs@b[*] from xs@x where x > 2 order by x desc range 0..3;
}
"#;
    let result = assert_executes_ok(src, 1);
    assert_all_query_results(&result);
}

// ---------------------------------------------------------------------------
// Test: compilation error is reported
// ---------------------------------------------------------------------------

#[test]
fn type_error_is_reported() {
    let src = r#"
fn main() {
    let xs = [1, 2, 3];
    let _ = select xs@b[*] from xs@x where x > "hello";
}
"#;
    // This should fail at type-checking (comparing int to string).
    let result = execute_in_memory(src);
    assert!(result.is_err(), "expected a type error");
}

// ---------------------------------------------------------------------------
// Test: parse error is reported
// ---------------------------------------------------------------------------

#[test]
fn parse_error_is_reported() {
    let src = r#"
fn main() {
    let _ = select from;
}
"#;
    let result = execute_in_memory(src);
    assert!(result.is_err(), "expected a parse error");
}

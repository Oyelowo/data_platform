//! End-to-end tests for the Yelang query pipeline using the public driver API.
//!
//! These tests parse real `.ye` source (stdlib prelude + user code), run the
//! full frontend and QIR pipeline, and assert on the executed result.

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

#[test]
fn stdlib_parses_and_compiles() {
    let src = r#"
fn main() {
    let users = [1, 2, 3];
    let _ = select u from users@u;
}
"#;
    let compiled = Driver::new().compile(src).expect("compile");
    assert!(compiled.plan.root.is_some());
}

#[test]
fn e2e_filter_and_map() {
    let src = r#"
fn main() {
    let users = [1, 2, 3, 4, 5];
    let _ = select u + 10 from users@u where u > 2;
}
"#;
    let result = Driver::new().run(src).expect("run");
    assert_eq!(ints(result), vec![13, 14, 15]);
}

#[test]
fn e2e_order_by_and_range() {
    let src = r#"
fn main() {
    let users = [5, 1, 4, 2, 3];
    let _ = select u from users@u order by u asc range 1..3;
}
"#;
    let result = Driver::new().run(src).expect("run");
    // Sorted: [1, 2, 3, 4, 5]; range 1..3 -> offset 1, limit 3 -> [2, 3, 4]
    assert_eq!(ints(result), vec![2, 3, 4]);
}

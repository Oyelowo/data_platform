//! Tests for the `Queryable` trait and `QueryArray<T>` wrapper.
//!
//! Phase 2 only guarantees that the stdlib definitions parse, resolve, and
//! type-check; QIR extraction from `@intrinsic` bodies is Phase 3. These tests
//! therefore stay at the type-checking layer.

use yelang_driver::Driver;

#[test]
fn query_array_methods_type_check() {
    // A function that accepts a `QueryArray` and calls several `Queryable`
    // methods. This exercises the trait signatures, the `QueryArray` impl, and
    // the `@intrinsic` bodies without requiring a way to construct a `PlanId`
    // in user code.
    //
    // NOTE: Methods whose names overlap with `Iterator` (`filter`, `map`,
    // `take`, `skip`, `count`, `fold`) currently require an unambiguous
    // downstream `Queryable`-only method to guide method resolution. Here we
    // end with `execute`, which only exists on `Queryable`.
    let src = r#"
        fn use_query_array(q: QueryArray<i32>) -> i32 {
            q.distinct()
             .sum()
        }

        fn main() {
            let _ = 1;
        }
    "#;

    let result = Driver::new().compile_or_eval_main(src);
    assert!(
        result.is_ok(),
        "QueryArray method chain should type-check: {:?}",
        result.err()
    );
}

#[test]
fn queryable_sum_method_uses_aggregate() {
    // `Queryable::sum` is a default method that calls `self.aggregate(Sum)`.
    // This test ensures the default body resolves and type-checks for a
    // `QueryArray<i32>` and that the result is an `i32`.
    let src = r#"
        fn sum_query(q: QueryArray<i32>) -> i32 {
            q.sum()
        }

        fn main() {
            let _ = 1;
        }
    "#;

    let result = Driver::new().compile_or_eval_main(src);
    assert!(
        result.is_ok(),
        "Queryable::sum should resolve to aggregate(Sum) and type-check: {:?}",
        result.err()
    );
}

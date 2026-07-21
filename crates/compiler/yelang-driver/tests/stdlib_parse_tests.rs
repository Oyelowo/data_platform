//! Integration tests that exercise the core stdlib through the driver.
//!
//! These tests load `stdlib/core/src/{iter,aggregate,aggregate_impls,query}.ye`
//! via the normal prelude mechanism and assert that the driver can parse,
//! resolve, lower, and type-check the combined prelude together with trivial
//! user code.

use yelang_driver::Driver;
use yelang_resolve::lang_items::LangItem;

#[test]
fn stdlib_loads_without_errors() {
    // The driver concatenates iter.ye, aggregate.ye, aggregate_impls.ye, and
    // query.ye in order before appending the user source. A trivial main body
    // is enough to prove the whole prelude pipeline succeeds.
    let result = Driver::new().compile_or_eval_main("fn main() { let _ = 1; }");
    assert!(
        result.is_ok(),
        "core stdlib should parse, resolve, lower, and type-check without errors: {:?}",
        result.err()
    );
}

#[test]
fn stdlib_lang_items_registered() {
    let compiled = Driver::new()
        .compile_or_eval_main("fn main() { let _ = 1; }")
        .expect("stdlib should load");

    assert!(
        compiled.tcx.lang_item(LangItem::Queryable).is_some(),
        "Queryable lang item should be registered"
    );
    assert!(
        compiled.tcx.lang_item(LangItem::Aggregate).is_some(),
        "Aggregate lang item should be registered"
    );
    assert!(
        compiled.tcx.lang_item(LangItem::Iterator).is_some(),
        "Iterator lang item should be registered"
    );
    assert!(
        compiled.tcx.lang_item(LangItem::IntoIterator).is_some(),
        "IntoIterator lang item should be registered"
    );
}

#[test]
fn stdlib_aggregate_methods_are_available() {
    let src = r#"
        fn main() {
            let xs = [1, 2, 3];
            let _ = xs.sum();
        }
    "#;
    let result = Driver::new().compile_or_eval_main(src);
    assert!(
        result.is_ok(),
        "stdlib aggregate methods should resolve and type-check: {:?}",
        result.err()
    );
}

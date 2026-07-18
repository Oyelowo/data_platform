//! Property-based tests for the v2 in-place B+ tree engine.
//!
//! The default proptest case count is reduced because each case exercises the
//! full transactional path (begin / put / commit with fsync). The count can be
//! overridden via the `PROPTEST_CASES` environment variable for deeper local or
//! CI runs.

use std::sync::Once;

use storage_btree::{BtreeEngine, BtreeOptions};
use storage_testkit::property;

static SET_PROPTEST_CASES: Once = Once::new();

fn ensure_proptest_cases() {
    SET_PROPTEST_CASES.call_once(|| {
        if std::env::var("PROPTEST_CASES").is_err() {
            // Default: small enough for a normal `cargo test --all-targets` run,
            // large enough to catch simple regressions. Override with
            // `PROPTEST_CASES=256` for deeper coverage.
            // SAFETY: this is a single-threaded test setup block; no other
            // thread reads `PROPTEST_CASES` before this call completes.
            unsafe { std::env::set_var("PROPTEST_CASES", "32") };
        }
    });
}

fn factory() -> BtreeEngine {
    let dir = Box::leak(Box::new(tempfile::tempdir().unwrap()));
    BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap()
}

#[test]
fn run_property_tests() {
    ensure_proptest_cases();
    property::run(factory);
}

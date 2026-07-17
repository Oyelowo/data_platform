//! Conformance tests for `storage-bwtree`.

use storage_bwtree::{BwTreeEngine, BwTreeOptions};

fn factory() -> BwTreeEngine {
    let dir = Box::leak(Box::new(tempfile::tempdir().unwrap()));
    BwTreeEngine::open(dir.path(), BwTreeOptions::default()).unwrap()
}

#[test]
fn conformance_suite() {
    storage_testkit::conformance::run(factory);
}

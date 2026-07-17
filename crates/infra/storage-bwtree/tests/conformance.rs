//! Conformance tests for `storage-bwtree`.

use std::sync::Arc;

use storage_bwtree::{BwTreeEngine, BwTreeOptions};
use storage_traits::Engine;

fn factory() -> BwTreeEngine {
    let dir = Box::leak(Box::new(tempfile::tempdir().unwrap()));
    BwTreeEngine::open(dir.path(), BwTreeOptions::default()).unwrap()
}

#[test]
fn conformance_suite() {
    storage_testkit::conformance::run(factory);
}

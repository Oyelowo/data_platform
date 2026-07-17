//! Property-based tests for `storage-bwtree`.

use storage_bwtree::{BwTreeEngine, BwTreeOptions};
use storage_traits::Engine;

fn factory() -> BwTreeEngine {
    let dir = Box::leak(Box::new(tempfile::tempdir().unwrap()));
    BwTreeEngine::open(dir.path(), BwTreeOptions::default()).unwrap()
}

#[test]
fn property_suite() {
    storage_testkit::property::run(factory);
}

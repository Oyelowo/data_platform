//! Property-based tests for `storage-btree`.

use storage_btree::{BtreeEngine, BtreeOptions};
use storage_testkit::property;

fn factory() -> BtreeEngine {
    let dir = Box::leak(Box::new(tempfile::tempdir().unwrap()));
    BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap()
}

#[test]
fn run_property_tests() {
    property::run(factory);
}

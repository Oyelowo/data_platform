//! `storage-testkit` conformance suite for the v2 in-place B+ tree engine.

use storage_btree::{BtreeEngine, BtreeOptions};
use storage_testkit::conformance;

fn factory() -> BtreeEngine {
    let dir = Box::leak(Box::new(tempfile::tempdir().unwrap()));
    BtreeEngine::open(dir.path(), BtreeOptions::default()).unwrap()
}

#[test]
fn run_conformance() {
    conformance::run(factory);
}

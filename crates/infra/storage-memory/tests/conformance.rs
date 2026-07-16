//! Conformance and property-based tests for [`storage_memory::MemoryEngine`].

#![allow(clippy::needless_borrows_for_generic_args)]

use storage_memory::MemoryEngine;
use storage_testkit::{conformance, property};

fn fresh_engine() -> MemoryEngine {
    MemoryEngine::new()
}

#[test]
fn conformance_suite() {
    conformance::run(&fresh_engine);
}

#[test]
fn property_direct_ops_match_model() {
    property::operation_sequence::direct_ops_match_model(&fresh_engine);
}

#[test]
fn property_scans_are_sorted() {
    property::operation_sequence::scans_are_sorted(&fresh_engine);
}

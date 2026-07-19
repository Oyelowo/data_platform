//! storage-testkit conformance suite for `storage-art`.

use storage_art::ArtMap;
use storage_art::ArtMapOptions;
#[test]
fn conformance_suite() {
    storage_testkit::conformance::run::<ArtMap, _>(|| ArtMap::new(ArtMapOptions::default()));
}

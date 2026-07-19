//! Memory-safety smoke test intended for Miri.

use bytes::Bytes;
use storage_art::{ArtMap, ArtMapOptions};

#[test]
fn miri_crud_smoke() {
    let map = ArtMap::new(ArtMapOptions::default());
    map.insert(b"hello", b"world").unwrap();
    map.insert(b"foo", b"bar").unwrap();
    assert_eq!(map.get(b"hello"), Some(Bytes::from_static(b"world")));
    assert_eq!(
        map.remove(b"foo").unwrap(),
        Some(Bytes::from_static(b"bar"))
    );
    assert_eq!(map.get(b"foo"), None);

    let snapshot = map.snapshot().unwrap();
    let decoded = storage_art::snapshot::decode(&snapshot).unwrap();
    assert_eq!(decoded.get(b"hello"), Some(Bytes::from_static(b"world")));
}

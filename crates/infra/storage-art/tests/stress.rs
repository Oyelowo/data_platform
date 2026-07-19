//! Heavy randomized stress test for `ArtMap`.

use std::collections::BTreeSet;

use bytes::Bytes;
use storage_art::{ArtMap, ArtMapOptions};

#[test]
fn stress_random_keys() {
    let map = ArtMap::new(ArtMapOptions::default());
    let mut keys = BTreeSet::new();
    for i in 0..10_000 {
        let key = format!("{:08x}", i * 7);
        let value = format!("value{}", i);
        map.insert(key.as_bytes(), value.as_bytes()).unwrap();
        keys.insert(key);
    }

    let mut cursor = map.range(None, None);
    let mut count = 0;
    for expected in &keys {
        let (k, v) = cursor.next().unwrap().unwrap();
        assert_eq!(&k[..], expected.as_bytes());
        assert_eq!(&v[..], format!("value{}", count).as_bytes());
        count += 1;
    }
    assert_eq!(count, keys.len());
    assert!(cursor.next().is_none());
}

#[test]
fn stress_insert_remove_cycle() {
    let map = ArtMap::new(ArtMapOptions::default());
    for round in 0..10 {
        for i in 0..1_000 {
            let key = format!("key{:04}", i);
            map.insert(key.as_bytes(), format!("v{}-{}", round, i).as_bytes())
                .unwrap();
        }
        for i in 0..1_000 {
            let key = format!("key{:04}", i);
            let expected = Bytes::from(format!("v{}-{}", round, i));
            assert_eq!(map.get(key.as_bytes()), Some(expected));
        }
        for i in 0..1_000 {
            let key = format!("key{:04}", i);
            map.remove(key.as_bytes()).unwrap();
        }
        for i in 0..1_000 {
            let key = format!("key{:04}", i);
            assert_eq!(map.get(key.as_bytes()), None);
        }
    }
    assert!(map.is_empty());
}
